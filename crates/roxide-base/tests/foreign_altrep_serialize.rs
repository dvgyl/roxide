#![forbid(unsafe_code)]

//! Compare foreign ALTREP streams with GNU R 4.6.2 output.

use roxide_base::serialize::{serialize_bytes, unserialize_bytes};
use roxide_eval::{AltOp, AltReply, Ctx, CtxExt, EvalResult, NamespaceLoader, NativeHooks, Unwind};
use roxide_gc::{GcPolicy, SexpId, Trace, Tracer};
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi};

const CLASS: &str = "foreign_vector";
const PACKAGE: &str = "c4_missing_package";
const WARNING: &str = "cannot unserialize ALTVEC object of class 'foreign_vector' from package 'c4_missing_package'; returning length zero vector";

/// GNU R 4.6.2 output for an integer ALTREP with state `c(7L, 2L, 9L)`.
const XDR: &str = concat!(
    "580a000000030004060200030500000000055554462d38000001ee",
    "0000000200000001000400090000000e666f726569676e5f766563746f72",
    "0000000200000001000400090000001263345f6d697373696e675f7061636b616765",
    "000000020000000d000000010000000d000000fe",
    "0000000d00000003000000070000000200000009",
    "00000402000000010004000900000005636c617373",
    "0000001000000001000400090000000a746573745f636c617373",
    "000004020000000100040009000000056c6162656c0000000d000000010000002a000000fe",
);

const ASCII: &[u8] = b"A\n3\n263682\n197888\n5\nUTF-8\n494\n2\n1\n262153\n14\nforeign_vector\n2\n1\n262153\n18\nc4_missing_package\n2\n13\n1\n13\n254\n13\n3\n7\n2\n9\n1026\n1\n262153\n5\nclass\n16\n1\n262153\n10\ntest_class\n1026\n1\n262153\n5\nlabel\n13\n1\n42\n254\n";

const NAMED_ASCII: &[u8] = concat!(
    "A\n3\n263682\n197888\n5\nUTF-8\n238\n2\n1\n262153\n14\nforeign_vector\n",
    "2\n1\n262153\n18\nc4_missing_package\n2\n13\n1\n13\n254\n13\n3\n7\n2\n9\n",
    "1026\n1\n262153\n5\nnames\n16\n3\n262153\n1\na\n262153\n1\nb\n262153\n1\nc\n254\n",
)
.as_bytes();

const COMPACT_ASCII: &[u8] = b"A\n3\n263682\n197888\n5\nUTF-8\n238\n2\n1\n262153\n14\ncompact_intseq\n2\n1\n262153\n4\nbase\n2\n13\n1\n13\n254\n14\n3\n10\n1\n1\n254\n";

#[derive(Default)]
struct Methods {
    serialized: usize,
    restored: usize,
    materialized: usize,
    loads: usize,
    restore_after_load: bool,
    loaded: bool,
    missing_class: bool,
    flags: Option<(i32, i32)>,
    ignore_attributes: bool,
    protect_depth: usize,
}

impl Trace for Methods {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for Methods {}

struct Hook;

impl NativeHooks for Hook {
    fn altrep(&self, ctx: &mut Ctx, op: AltOp) -> EvalResult<AltReply> {
        let heap = &mut ctx.session_mut().heap;
        let mark = heap.protect_mark();
        heap.try_protect(SEXP_NIL).unwrap();
        let depth = heap.protect_depth();
        heap.unprotect_to(mark);
        let methods = ctx.ext::<Methods>();
        methods.protect_depth = methods.protect_depth.max(depth);
        ctx.collect_gc();
        Ok(match op {
            AltOp::SerializedState(value) => {
                ctx.ext::<Methods>().serialized += 1;
                let info = ctx.session().foreign_alt(value)?.unwrap();
                let mode = ctx.session().int(info.data2)?.get(0);
                let state = match mode {
                    1 => None,
                    2 => Some(SEXP_NIL),
                    _ => Some(info.data1),
                };
                AltReply::Value(state)
            }
            AltOp::Unserialize {
                class,
                package,
                ty,
                state,
                attrs,
                objf,
                levels,
            } => {
                let matches = ctx.session().sym_name(class)? == CLASS
                    && ctx.session().sym_name(package)? == PACKAGE;
                let methods = ctx.ext::<Methods>();
                methods.restored += 1;
                if !matches
                    || methods.missing_class
                    || (methods.restore_after_load && !methods.loaded)
                {
                    return Ok(AltReply::Value(None));
                }
                methods.flags = Some((objf, levels));
                let ignore_attributes = methods.ignore_attributes;
                let descriptor = ctx.session_mut().alloc_alt_class(class, package, 0)?;
                let value = ctx
                    .session_mut()
                    .alloc_foreign_alt(descriptor, ty, state, SEXP_NIL)?;
                if !ignore_attributes {
                    let mut attrs = attrs;
                    let mut entries = Vec::new();
                    while attrs != SEXP_NIL {
                        let (attr, rest, tag) = ctx.session().cons_parts(attrs)?;
                        entries.push((ctx.session().sexp_as_sym(tag)?, attr));
                        attrs = rest;
                    }
                    ctx.altrep_len(value)?;
                    let session = ctx.session_mut();
                    session.replace_attributes(value, entries)?;
                    session.set_object_flag(value, objf != 0)?;
                    session.set_gp(value, levels as u16)?;
                }
                AltReply::Value(Some(value))
            }
            AltOp::Materialize(value) => {
                ctx.ext::<Methods>().materialized += 1;
                let state = ctx.session().foreign_alt(value)?.unwrap().data1;
                let values: Vec<_> = ctx.session().int(state)?.iter().collect();
                let dense = ctx.session_mut().alloc_int_from(&values)?;
                assert!(ctx.session_mut().adopt_foreign_alt_payload(value, dense)?);
                AltReply::Unit
            }
            AltOp::Length(value) => {
                let dense = ctx.session().foreign_alt(value)?.unwrap().data1;
                AltReply::Len(ctx.session().xlength(dense)?)
            }
            _ => AltReply::Value(None),
        })
    }
}

struct Loader;

impl NamespaceLoader for Loader {
    fn load(&self, ctx: &mut Ctx, name: &str, call: SexpId) -> EvalResult<SexpId> {
        ctx.collect_gc();
        assert_eq!(name, PACKAGE);
        let methods = ctx.ext::<Methods>();
        methods.loads += 1;
        if methods.restore_after_load {
            methods.loaded = true;
            Ok(GLOBAL_ENV)
        } else {
            Err(ctx.stop(call, "test namespace lookup failed"))
        }
    }
}

fn context() -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).unwrap();
    roxide_base::register_all(&mut ctx).unwrap();
    ctx
}

fn evaluated<T>(result: EvalResult<T>, ctx: &Ctx) -> T {
    result.unwrap_or_else(|error| panic!("{error:?}: {}", ctx.last_error_message()))
}

fn fixture() -> Vec<u8> {
    XDR.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn foreign(ctx: &mut Ctx, values: &[i32], mode: i32) -> SexpId {
    let session = ctx.session_mut();
    let class = session.intern(CLASS).unwrap();
    let package = session.intern(PACKAGE).unwrap();
    let descriptor = session.alloc_alt_class(class, package, 0).unwrap();
    let state = session.alloc_int_from(values).unwrap();
    let mode = session.scalar_int(mode).unwrap();
    let value = session
        .alloc_foreign_alt(descriptor, SexpType::Int, state, mode)
        .unwrap();
    let class = session.intern("class").unwrap();
    let class_value = session.scalar_str("test_class").unwrap();
    session.set_attrib(value, class, class_value).unwrap();
    let label = session.intern("label").unwrap();
    let label_value = session.scalar_int(42).unwrap();
    session.set_attrib(value, label, label_value).unwrap();
    ctx.protect(value);
    value
}

fn define(ctx: &mut Ctx, name: &str, value: SexpId) {
    let name = ctx.session_mut().intern(name).unwrap();
    ctx.session_mut()
        .env_define(GLOBAL_ENV, name, value)
        .unwrap();
}

fn get(ctx: &mut Ctx, name: &str) -> SexpId {
    let name = ctx.session_mut().intern(name).unwrap();
    ctx.get_var(name, GLOBAL_ENV).unwrap().unwrap()
}

fn check_attrs(ctx: &mut Ctx, value: SexpId) {
    let session = ctx.session_mut();
    assert!(session.is_object(value).unwrap());
    let label = session.intern("label").unwrap();
    assert_eq!(session.int(session.attr(value, label)).unwrap().get(0), 42);
}

#[test]
fn v3_uses_serialized_state_before_and_after_materialization() {
    let mut ctx = context();
    ctx.set_native_hooks(&Hook);
    let source = foreign(&mut ctx, &[7, 2, 9], 0);
    for owned in [false, true] {
        if owned {
            ctx.resolve_altrep(source).unwrap();
        }
        let bytes = evaluated(serialize_bytes(&mut ctx, source, 3, None), &ctx);
        assert_eq!(bytes, fixture());
        assert_eq!(
            ctx.session().foreign_alt(source).unwrap().unwrap().owned,
            owned
        );

        let restored = evaluated(unserialize_bytes(&mut ctx, &bytes, None), &ctx);
        ctx.protect(restored);
        assert!(!ctx.session().foreign_alt(restored).unwrap().unwrap().owned);
        assert_eq!(ctx.ext::<Methods>().flags, Some((1, 0)));
        check_attrs(&mut ctx, restored);
        ctx.resolve_altrep(restored).unwrap();
        assert_eq!(
            ctx.session().int(restored).unwrap().as_slice(),
            Some(&[7, 2, 9][..])
        );
    }
    assert_eq!(ctx.ext::<Methods>().serialized, 2);
    assert_eq!(ctx.ext::<Methods>().restored, 2);
    assert_eq!(ctx.ext::<Methods>().materialized, 3);
}

#[test]
fn v2_and_missing_state_write_dense_vectors() {
    for (version, mode, serialized) in [(2, 0, 0), (3, 1, 1)] {
        let mut ctx = context();
        ctx.set_native_hooks(&Hook);
        let source = foreign(&mut ctx, &[7, 2, 9], mode);
        let bytes = evaluated(serialize_bytes(&mut ctx, source, version, None), &ctx);
        let restored = evaluated(unserialize_bytes(&mut ctx, &bytes, None), &ctx);
        assert!(!ctx.session().is_altrep(restored).unwrap());
        assert_eq!(
            ctx.session().int(restored).unwrap().as_slice(),
            Some(&[7, 2, 9][..])
        );
        check_attrs(&mut ctx, restored);
        assert_eq!(ctx.ext::<Methods>().serialized, serialized);
        assert_eq!(ctx.ext::<Methods>().materialized, 1);
        assert_eq!(ctx.ext::<Methods>().restored, 0);
    }
}

#[test]
fn nil_serialized_state_is_written_as_custom_state() {
    let mut ctx = context();
    ctx.set_native_hooks(&Hook);
    let source = foreign(&mut ctx, &[7, 2, 9], 2);
    define(&mut ctx, "source", source);
    let result = ctx.run_global(
        "<serialize>",
        "bytes <- serialize(source, NULL, ascii = TRUE)",
    );
    evaluated(result, &ctx);
    let expected = std::str::from_utf8(ASCII)
        .unwrap()
        .replace("254\n13\n3\n7\n2\n9\n", "254\n254\n");
    let bytes = get(&mut ctx, "bytes");
    assert_eq!(
        ctx.session().raw(bytes).unwrap().as_slice(),
        Some(expected.as_bytes())
    );
    assert_eq!(ctx.ext::<Methods>().serialized, 1);
    assert_eq!(ctx.ext::<Methods>().materialized, 0);
}

#[test]
fn v3_dense_fallback_keeps_a_sequence_dense() {
    let mut ctx = context();
    ctx.set_native_hooks(&Hook);
    let value = foreign(&mut ctx, &[7, 8, 9, 10], 1);
    ctx.session_mut()
        .replace_attributes(value, Vec::new())
        .unwrap();
    let bytes = evaluated(serialize_bytes(&mut ctx, value, 3, None), &ctx);
    let restored = evaluated(unserialize_bytes(&mut ctx, &bytes, None), &ctx);
    assert!(!ctx.session().is_altrep(restored).unwrap());
    assert_eq!(
        ctx.session().int(restored).unwrap().as_slice(),
        Some(&[7, 8, 9, 10][..])
    );
}

#[test]
fn r_entry_points_preserve_lazy_state_and_connection_position() {
    let mut ctx = context();
    ctx.set_native_hooks(&Hook);
    let source = foreign(&mut ctx, &[7, 2, 9], 0);
    define(&mut ctx, "source", source);
    let result = ctx.run_global(
        "<serialize>",
        r#"
        ascii <- serialize(source, NULL, ascii = TRUE)
        compact <- serialize(1:10, NULL, ascii = TRUE)
        bytes <- c(ascii, serialize(42L, NULL))
        con <- rawConnection(bytes, "rb")
        restored <- unserialize(con)
        next_value <- unserialize(con)
        close(con)
    "#,
    );
    evaluated(result, &ctx);
    for (name, expected) in [("ascii", ASCII), ("compact", COMPACT_ASCII)] {
        let bytes = get(&mut ctx, name);
        assert_eq!(ctx.session().raw(bytes).unwrap().as_slice(), Some(expected));
    }
    let restored = get(&mut ctx, "restored");
    check_attrs(&mut ctx, restored);
    assert!(!ctx.session().foreign_alt(restored).unwrap().unwrap().owned);
    let next = get(&mut ctx, "next_value");
    assert_eq!(ctx.session().int(next).unwrap().get(0), 42);
    assert_eq!(ctx.ext::<Methods>().serialized, 1);
    assert_eq!(ctx.ext::<Methods>().restored, 1);
    assert_eq!(ctx.ext::<Methods>().materialized, 0);
}

#[test]
fn unknown_class_warns_and_discards_metadata_like_gnu_r() {
    let mut ctx = context();
    ctx.set_native_hooks(&Hook);
    ctx.set_namespace_loader(&Loader);
    ctx.ext::<Methods>().missing_class = true;
    let raw = ctx.session_mut().alloc_raw_from(NAMED_ASCII).unwrap();
    define(&mut ctx, "stream", raw);
    let result = ctx.run_global(
        "<unserialize>",
        r#"
        seen <- character()
        origin <- NULL
        result <- withCallingHandlers(unserialize(stream), warning = function(w) {
            seen <<- c(seen, conditionMessage(w))
            origin <<- conditionCall(w)
            invokeRestart("muffleWarning")
        })
    "#,
    );
    evaluated(result, &ctx);
    let value = get(&mut ctx, "result");
    assert_eq!(ctx.session().type_of(value).unwrap(), SexpType::Int);
    assert_eq!(ctx.session().xlength(value).unwrap(), 0);
    assert!(ctx.session().attrs(value).unwrap().is_empty());
    assert!(!ctx.session().is_object(value).unwrap());
    let message = get(&mut ctx, "seen");
    let chars = ctx.session().str_slice(message).unwrap();
    assert_eq!(chars.len(), 1);
    assert_eq!(
        ctx.session().char_bytes(chars[0]).unwrap(),
        WARNING.as_bytes()
    );
    let call = get(&mut ctx, "origin");
    assert_eq!(
        ctx.deparse_lines(call, Default::default()).unwrap(),
        ["unserialize(stream)"]
    );
    assert_eq!(ctx.ext::<Methods>().loads, 1);
    assert_eq!(ctx.ext::<Methods>().restored, 2);
}

#[test]
fn unserialize_ex_owns_attributes_and_flags() {
    let mut ctx = context();
    ctx.set_native_hooks(&Hook);
    ctx.ext::<Methods>().ignore_attributes = true;
    let levels = 2;
    let flags = 494 | (levels << 12);
    let stream = std::str::from_utf8(ASCII)
        .unwrap()
        .replacen("494\n", &format!("{flags}\n"), 1);
    let restored = evaluated(unserialize_bytes(&mut ctx, stream.as_bytes(), None), &ctx);
    assert_eq!(ctx.ext::<Methods>().flags, Some((1, levels)));
    assert!(ctx.session().attrs(restored).unwrap().is_empty());
    assert!(!ctx.session().is_object(restored).unwrap());
    assert_eq!(ctx.session().gp(restored).unwrap(), 0);
}

#[test]
fn large_lists_leave_room_for_native_protect() {
    const LEN: usize = 56_000;
    let mut ctx = context();
    ctx.set_native_hooks(&Hook);
    let foreign = foreign(&mut ctx, &[7, 2, 9], 0);
    let session = ctx.session_mut();
    let source = session.alloc_list(LEN + 1).unwrap();
    for i in 0..LEN {
        let child = session.alloc_int_from(&[i as i32, 0]).unwrap();
        session.list_set(source, i, child).unwrap();
    }
    session.list_set(source, LEN, foreign).unwrap();
    let depth = ctx.session().heap.protect_depth();
    let bytes = evaluated(serialize_bytes(&mut ctx, source, 3, None), &ctx);
    let restored = evaluated(unserialize_bytes(&mut ctx, &bytes, None), &ctx);
    assert_eq!(ctx.session().heap.protect_depth(), depth);
    assert!(ctx.ext::<Methods>().protect_depth < 100);
    assert_eq!(ctx.session().xlength(restored).unwrap(), LEN + 1);
    for i in 0..LEN {
        let child = ctx.session().list_get(restored, i).unwrap();
        assert_eq!(
            ctx.session().int(child).unwrap().as_slice(),
            Some(&[i as i32, 0][..])
        );
    }
    let restored_foreign = ctx.session().list_get(restored, LEN).unwrap();
    assert!(ctx.session().is_altrep(restored_foreign).unwrap());
    check_attrs(&mut ctx, restored_foreign);
}

#[test]
fn connection_reads_across_buffer_boundaries() {
    let mut ctx = context();
    ctx.set_native_hooks(&Hook);
    let source = foreign(&mut ctx, &[7, 2, 9], 0);
    define(&mut ctx, "source", source);
    let result = ctx.run_global(
        "<serialize>",
        r#"
        out <- file("streams.bin", "wb")
        serialize(raw(30000L), out, version = 2)
        serialize(raw(31000L), out)
        serialize(list(raw(10000L), source), out, ascii = TRUE)
        serialize(42L, out)
        close(out)
        input <- file("streams.bin", "rb")
        dense <- unserialize(input)
        dense_v3 <- unserialize(input)
        restored <- unserialize(input)[[2L]]
        next_value <- unserialize(input)
        close(input)
    "#,
    );
    evaluated(result, &ctx);
    for (name, len) in [("dense", 30_000), ("dense_v3", 31_000)] {
        let dense = get(&mut ctx, name);
        assert_eq!(ctx.session().xlength(dense).unwrap(), len);
    }
    let restored = get(&mut ctx, "restored");
    check_attrs(&mut ctx, restored);
    let next = get(&mut ctx, "next_value");
    assert_eq!(ctx.session().int(next).unwrap().get(0), 42);
}

#[test]
fn namespace_load_retries_the_hook_with_live_state_and_attributes() {
    let mut ctx = context();
    ctx.set_native_hooks(&Hook);
    ctx.set_namespace_loader(&Loader);
    ctx.ext::<Methods>().restore_after_load = true;
    let restored = evaluated(unserialize_bytes(&mut ctx, &fixture(), None), &ctx);
    check_attrs(&mut ctx, restored);
    assert_eq!(ctx.ext::<Methods>().loads, 1);
    assert_eq!(ctx.ext::<Methods>().restored, 2);
}

#[test]
fn malformed_streams_fail_before_the_foreign_callback() {
    let mut ctx = context();
    ctx.set_native_hooks(&Hook);
    let bytes = fixture();
    for length in [0, 23, 70, bytes.len() - 1] {
        assert!(unserialize_bytes(&mut ctx, &bytes[..length], None).is_err());
    }
    let invalid_attrs = ASCII.strip_suffix(b"254\n").unwrap();
    let mut malformed = invalid_attrs.to_vec();
    malformed.extend_from_slice(b"13\n1\n5\n");
    assert!(unserialize_bytes(&mut ctx, &malformed, None).is_err());
    assert_eq!(ctx.ext::<Methods>().restored, 0);
}

#[test]
fn warning_as_error_stops_unserialization() {
    let mut ctx = context();
    ctx.opts_mut().warn = 2;
    let error = unserialize_bytes(&mut ctx, &fixture(), None).unwrap_err();
    assert!(matches!(error, Unwind::TopLevel));
}

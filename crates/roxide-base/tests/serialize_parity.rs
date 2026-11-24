#![forbid(unsafe_code)]

//! Check XDR streams and workspace files against GNU R 4.6.2.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, CtxExt, Dispatch, EvalResult, Module, PrimDef, PrimKind,
    Unwind, Visibility,
};
use roxide_gc::{GcPolicy, Trace, Tracer};
use roxide_host::{Host, Limits, NativeHost, PureHost};
use roxide_value::{DeparseOpts, GLOBAL_ENV, SEXP_NIL, ValueApi, dput};

const CASES: &str = include_str!("../../../conformance/layer1/07_rds_rdata_roundtrip.rcase");

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static INDEX: AtomicUsize = AtomicUsize::new(0);
        let path = env::temp_dir().join(format!(
            "roxide-xdr-{}-{}",
            std::process::id(),
            INDEX.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("Create test directory");
        Self(path)
    }

    fn path(&self, name: &str) -> String {
        format!("{:?}", self.0.join(name).to_str().expect("UTF-8 test path"))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Output(String);

impl Trace for Output {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for Output {}

fn capture(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    for line in dput(ctx.session(), value, DeparseOpts::default())? {
        let out = &mut ctx.ext::<Output>().0;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(value)
}

const CAPTURE: Module = Module {
    prims: &[PrimDef {
        name: "dput",
        fun: capture,
        kind: PrimKind::Primitive,
        args: ArgEval::Eval,
        arity: Arity::Exact(1),
        dispatch: Dispatch::None,
        visibility: Visibility::Off,
    }],
    prelude: "",
};

fn context(host: Box<dyn Host>) -> Ctx {
    let mut ctx = Ctx::new(host, GcPolicy::Torture).expect("Create R session");
    roxide_base::register_all(&mut ctx).expect("Register base modules");
    ctx
}

fn run(ctx: &mut Ctx, name: &str, script: &str) {
    ctx.run_script(name, script, GLOBAL_ENV, false)
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
}

fn oracle(executable: &Path, code: &str) -> String {
    let output = Command::new(executable)
        .args(["--vanilla", "-e", code])
        .env("R_ENABLE_JIT", "0")
        .output()
        .expect("Run GNU R 4.6.2");
    assert!(
        output.status.success(),
        "GNU R: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Read GNU R output")
}

#[test]
fn layer_one_serialization_cases_match_gnu_r() {
    let mut total = 0;
    for case in CASES.split("=== case: ").skip(1) {
        let (header, script) = case.split_once('\n').expect("Read case header");
        let name = header.strip_suffix(" ===").expect("Read case name");
        let expected = match name {
            "serialize_null_in_both_versions" => "\"raw\"\nTRUE\n\"raw\"\nTRUE\n",
            "serialize_logical_and_integer_vectors"
            | "serialize_pairlists_and_calls"
            | "save_rds_and_read_rds_with_each_compression" => "TRUE\nTRUE\nTRUE\nTRUE\n",
            "serialize_double_special_values_and_negative_zero" => {
                "TRUE\nc(-Inf, Inf)\nTRUE\nc(-Inf, Inf)\n"
            }
            "serialize_utf8_and_missing_strings" => concat!(
                "TRUE\nc(\"UTF-8\", \"UTF-8\", \"unknown\", \"unknown\")\n",
                "TRUE\nc(\"UTF-8\", \"UTF-8\", \"unknown\", \"unknown\")\n",
            ),
            "serialize_compact_integer_sequence" => {
                "TRUE\nc(100L, 1L, 42L)\nTRUE\nc(100L, 1L, 42L)\n"
            }
            "serialize_closures_with_shared_lexical_environment" => concat!(
                "TRUE\nTRUE\nTRUE\nc(6L, 8L, 8L, 6L)\n",
                "TRUE\nTRUE\nTRUE\nc(6L, 8L, 8L, 6L)\n",
            ),
            "save_and_load_objects_in_fresh_environment" => {
                "c(\"notes\", \"counts\")\nc(\"counts\", \"notes\")\nTRUE\nTRUE\n"
            }
            _ => "TRUE\nTRUE\n",
        };
        let mut ctx = context(Box::new(PureHost::new()));
        ctx.register_module(&CAPTURE)
            .expect("Register output capture");
        // Use a fresh virtual file system for each case.
        let script = script
            .replace("tempfile()", "\"/roundtrip\"")
            .replace("    on.exit(unlink(path))\n", "");
        run(&mut ctx, name, &script);
        assert_eq!(ctx.ext::<Output>().0, expected, "{name}");
        total += 1;
    }
    assert_eq!(total, 15);
}

#[test]
fn cross_engine_rds_and_workspace_roundtrips() {
    let Some(executable) = env::var_os("ROXIDE_ORACLE_RSCRIPT") else {
        return;
    };
    let executable = Path::new(&executable);
    let dir = TempDir::new();
    let mut prepare = String::from(
        r#"
stopifnot(as.character(getRversion()) == "4.6.2")
e <- new.env(parent = baseenv())
e$total <- 6L
e$self <- e
e$step <- 2L
delayedAssign("lazy", total + 10L, eval.env=e, assign.env=e)
delayedAssign("forced", total + 20L, eval.env=e, assign.env=e)
invisible(e$forced)
lockBinding("step", e)
lockEnvironment(e)
fun <- evalq(function() {
    step_value <- function() step
    total <<- total + step_value()
    total
}, e)
get_total <- evalq(function() total, e)
obj <- list(
    values = c(NA_real_, NaN, -0.0, Inf, -Inf),
    strings = c("caf\u00e9", "\u754c", "", NA_character_,
                iconv("caf\u00e9", "UTF-8", "latin1")),
    complex = complex(real = c(1, NA_real_, 0), imaginary = c(-2, 4, Inf)),
    raw = as.raw(c(0L, 1L, 128L, 255L)),
    compact = 1:100,
    reverse = 2147483651:2147483648,
    data = data.frame(x = c(9L, NA_integer_), y = c("ridge", "shore")),
    factor = ordered(c("medium", NA_character_, "low"), levels = c("low", "medium", "high")),
    object = structure(c(first = 12.5, second = 14), class = c("sample_mass", "numeric"), unit = "g"),
    pair = pairlist(site = "shore", action = quote(combine(site, , count = 3L))),
    expr = expression(x + 1L, y),
    methods = list(add = compiler::cmpfun(fun), get_total = compiler::cmpfun(get_total)),
    primitives = list(.Primitive("+"), .Primitive("if")),
    extra = list(.GlobalEnv, emptyenv(), baseenv(), .BaseNamespaceEnv)
)
alpha <- obj
beta <- obj$methods
"#,
    );
    let mut roundtrip = String::from(
        r#"
verify <- function(obj) {
    scope <- environment(obj$methods$add)
    stopifnot(identical(scope, environment(obj$methods$get_total)))
    stopifnot(identical(scope, scope$self))
    stopifnot(identical(obj$methods$get_total(), 6L))
}
"#,
    );
    let mut verify = String::from(
        r#"
verify <- function(before, after) {
    old <- environment(before$methods$add)
    new <- environment(after$methods$add)
    stopifnot(identical(new, environment(after$methods$get_total)), identical(new, new$self))
    stopifnot(identical(parent.env(old), parent.env(new)), environmentIsLocked(new))
    stopifnot(bindingIsLocked("step", new), identical(ls(old), ls(new)))
    stopifnot(identical(old$total, new$total), identical(old$step, new$step))
    environment(before$methods$add) <- new
    environment(before$methods$get_total) <- new
    stopifnot(identical(before, after, num.eq = FALSE, single.NA = FALSE))
    stopifnot(identical(after$methods$get_total(), 6L), identical(after$methods$add(), 8L))
    stopifnot(identical(new$lazy, 18L), identical(new$forced, 26L))
}
"#,
    );
    for version in [2, 3] {
        for codec in ["FALSE", "\"gzip\"", "\"bzip2\"", "\"xz\"", "\"zstd\""] {
            let label = format!("{version}-{}", codec.trim_matches('"'));
            let rds = dir.path(&format!("gnu-{label}.rds"));
            let rdata = dir.path(&format!("gnu-{label}.RData"));
            let out_rds = dir.path(&format!("rox-{label}.rds"));
            let out_rdata = dir.path(&format!("rox-{label}.RData"));
            prepare.push_str(&format!(
                "saveRDS(obj, {rds}, version={version}, compress={codec})\n\
                 save(alpha, beta, file={rdata}, version={version}, compress={codec})\n"
            ));
            roundtrip.push_str(&format!(
                "obj <- readRDS({rds})\nverify(obj)\nsaveRDS(obj, {out_rds}, version={version}, compress={codec})\n\
                 e <- new.env(parent=emptyenv())\nresult <- withVisible(load({rdata}, envir=e))\n\
                 stopifnot(identical(result$value, c(\"alpha\", \"beta\")), identical(result$visible, FALSE))\n\
                 verify(e$alpha)\nstopifnot(identical(environment(e$alpha$methods$add), environment(e$beta$add)))\n\
                 save(alpha, list=\"beta\", file={out_rdata}, envir=e, version={version}, compress={codec})\n"
            ));
            verify.push_str(&format!(
                "verify(readRDS({rds}), readRDS({out_rds}))\n\
                 old <- new.env()\nnew <- new.env()\n\
                 stopifnot(identical(load({rdata}, envir=old), load({out_rdata}, envir=new)))\n\
                 stopifnot(identical(environment(new$alpha$methods$add), environment(new$beta$add)))\n\
                 verify(old$alpha, new$alpha)\n"
            ));
        }
    }
    oracle(executable, &prepare);
    run(
        &mut context(Box::new(NativeHost::new())),
        "cross-engine roundtrip",
        &roundtrip,
    );
    oracle(executable, &verify);
}

#[test]
fn xdr_bytes_match_gnu_r() {
    let Some(executable) = env::var_os("ROXIDE_ORACLE_RSCRIPT") else {
        return;
    };
    let executable = Path::new(&executable);
    let dir = TempDir::new();
    let cases = r#"
scope <- new.env(parent = baseenv(), hash = FALSE)
scope$x <- 4L
scope$y <- scope
objects <- list(
    NULL, c(TRUE, NA, FALSE), c(1L, NA_integer_), c(NA_real_, NaN, -0.0, Inf),
    complex(real = c(1, NA_real_), imaginary = c(-2, 4)),
    c("caf\u00e9", "", NA_character_), as.raw(c(0L, 1L, 255L)), 1:100,
    structure(1.25, class = "test", unit = "m"),
    pairlist(x = quote(f(x,, x = 1L)), y = scope), list(scope, scope),
    evalq(function(a = 1L) x + a, scope)
)
"#;
    let mut gnu = cases.to_owned();
    let mut rox = cases.to_owned();
    let mut files = Vec::new();
    for v in [2, 3] {
        for i in 1..=12 {
            let r = format!("{v}-{i}-gnu.rds");
            let x = format!("{v}-{i}-rox.rds");
            gnu.push_str(&format!(
                "saveRDS(objects[[{i}]], {}, version={v}, compress=FALSE)\n",
                dir.path(&r)
            ));
            rox.push_str(&format!(
                "saveRDS(objects[[{i}]], {}, version={v}, compress=FALSE)\n",
                dir.path(&x)
            ));
            files.push((r, x));
        }
    }
    oracle(executable, &gnu);
    run(&mut context(Box::new(NativeHost::new())), "XDR bytes", &rox);
    for (r, x) in files {
        assert_eq!(
            fs::read(dir.0.join(&r)).unwrap(),
            fs::read(dir.0.join(x)).unwrap(),
            "{r}"
        );
    }
}

#[test]
fn rejects_corrupt_streams_and_resource_limit_violations() {
    let dir = TempDir::new();
    let path = dir.path("bounded.rds");
    let good = format!("saveRDS(rep(1L, 2048L), {path})");
    run(
        &mut context(Box::new(NativeHost::new())),
        "gzip fixture",
        &good,
    );
    let limits = Limits {
        max_decompress_bytes: 256,
        max_vector_bytes: 128,
        ..Limits::default()
    };
    let mut ctx = context(Box::new(NativeHost::with_limits(limits)));
    let result = ctx.run_script("gzip limit", &format!("readRDS({path})"), GLOBAL_ENV, false);
    assert!(matches!(result, Err(Unwind::TopLevel)), "{result:?}");

    let cases = vec![
        vec![],
        b"X\n\0".to_vec(),
        xdr_body(&[0x1ff]),
        xdr_body(&[255, 0]),
        xdr_body(&[13, 33]),
        xdr_body(&[14, u32::MAX, u32::MAX, u32::MAX]),
        xdr_body(&[4, 0, 0x1ff, 254, 254, 254]),
    ];
    for bytes in cases {
        rejects_raw(&mut ctx, &bytes);
    }

    let mut nested = [19, 1].repeat(180);
    nested.push(254);
    rejects_raw(&mut context(Box::new(PureHost::new())), &xdr_body(&nested));
}

fn rejects_raw(ctx: &mut Ctx, bytes: &[u8]) {
    let input = ctx.session_mut().intern("input").unwrap();
    let raw = ctx.session_mut().alloc_raw_from(bytes).unwrap();
    ctx.assign(input, raw, GLOBAL_ENV).unwrap();
    let result = ctx.run_script("invalid XDR", "unserialize(input)", GLOBAL_ENV, false);
    assert!(matches!(result, Err(Unwind::TopLevel)), "{result:?}");
}

fn xdr_body(words: &[u32]) -> Vec<u8> {
    let mut bytes = b"X\n\0\0\0\x02\0\x04\x06\x02\0\x02\x03\0".to_vec();
    bytes.extend(words.iter().flat_map(|n| n.to_be_bytes()));
    bytes
}

#[test]
fn external_pointers_round_trip_with_attributes_and_reference_cycles() {
    let mut ctx = context(Box::new(PureHost::new()));
    let s = ctx.session_mut();
    let tag = s.scalar_int(9).unwrap();
    let ptr = s.alloc_extptr(123, tag, SEXP_NIL).unwrap();
    s.set_extptr_prot(ptr, ptr).unwrap();
    let stamp = s.intern("stamp").unwrap();
    s.set_attrib(ptr, stamp, ptr).unwrap();
    let binding = s.intern("ptr").unwrap();
    ctx.assign(binding, ptr, GLOBAL_ENV).unwrap();

    for version in [2, 3] {
        run(
            &mut ctx,
            "External pointer round trip",
            &format!(
                r#"
                output <- serialize(list(ptr, ptr), NULL, version={version}L)
                restored <- unserialize(output)
                stopifnot(identical(serialize(restored, NULL, version={version}L), output))
                "#,
            ),
        );
        let get = |ctx: &mut Ctx, name| {
            let sym = ctx.session_mut().intern(name).unwrap();
            ctx.get_var(sym, GLOBAL_ENV).unwrap().unwrap()
        };
        let restored = get(&mut ctx, "restored");
        let output = get(&mut ctx, "output");
        let s = ctx.session();
        let ptr = s.list_get(restored, 0).unwrap();
        assert_eq!(s.list_get(restored, 1).unwrap(), ptr);
        assert_eq!(s.extptr_addr(ptr).unwrap(), 0);
        assert_eq!(s.extptr_prot(ptr).unwrap(), ptr);
        assert_eq!(s.attr(ptr, stamp), ptr);
        assert_eq!(s.int(s.extptr_tag(ptr).unwrap()).unwrap().get(0), 9);

        let mut expected = xdr_body(&[]);
        if version == 3 {
            expected[5] = 3;
            expected[10..14].copy_from_slice(&0x00030500_u32.to_be_bytes());
            expected.extend_from_slice(b"\0\0\0\x05UTF-8");
        }
        for word in [19_u32, 2, 0x216, 0x1ff, 13, 1, 9, 0x402, 1, 0x40009, 5] {
            expected.extend(word.to_be_bytes());
        }
        expected.extend_from_slice(b"stamp");
        for word in [0x1ff_u32, 254, 0x1ff] {
            expected.extend(word.to_be_bytes());
        }
        let actual = s.raw(output).unwrap().to_vec();
        assert_eq!(actual, expected, "XDR version {version}");

        if let Some(executable) = env::var_os("ROXIDE_ORACLE_RSCRIPT") {
            let input = actual
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let response = oracle(
                Path::new(&executable),
                &format!(
                    r#"
                    input <- as.raw(c({input}))
                    decoded <- unserialize(input)
                    stopifnot(typeof(decoded[[1L]]) == "externalptr",
                              identical(attr(decoded[[1L]], "stamp"), decoded[[1L]]),
                              identical(serialize(decoded, NULL, version={version}L), input))
                    p <- new("externalptr")
                    attr(p, "stamp") <- 42L
                    cat(as.integer(serialize(list(p, p), NULL, version={version}L)), sep=",")
                    "#,
                ),
            );
            run(
                &mut ctx,
                "GNU R external pointer stream",
                &format!(
                    r#"
                    input <- as.raw(c({response}))
                    decoded <- unserialize(input)
                    stopifnot(identical(attr(decoded[[2L]], "stamp"), 42L),
                              identical(serialize(decoded, NULL, version={version}L), input))
                    "#,
                ),
            );
        }
    }
}

#[test]
fn binary_connections_keep_object_boundaries_and_open_state() {
    let script = r#"
    out <- rawConnection(raw(), "w")
    serialize(11L, out)
    saveRDS("second", out)
    stopifnot(isOpen(out))
    input <- rawConnection(rawConnectionValue(out))
    close(out)
    stopifnot(identical(unserialize(input), 11L),
              identical(readRDS(input), "second"), isOpen(input))
    close(input)

    p <- tempfile()
    for (open_file in list(file, gzfile, bzfile, xzfile, zstdfile)) {
        out <- open_file(p, "wb")
        serialize(11L, out)
        saveRDS("second", out)
        stopifnot(isOpen(out))
        close(out)
        input <- open_file(p, "rb")
        stopifnot(identical(unserialize(input), 11L),
                  identical(readRDS(input), "second"), isOpen(input))
        close(input)

        out <- open_file(p)
        stopifnot(identical(tryCatch(serialize(11L, out), error = conditionMessage),
                            "connection is not open"), !isOpen(out))
        saveRDS(42L, out)
        stopifnot(!isOpen(out))
        stopifnot(identical(tryCatch(unserialize(out), error = conditionMessage),
                            "connection is not open"), !isOpen(out))
        stopifnot(identical(readRDS(out), 42L), !isOpen(out))
        close(out)

        e1 <- new.env()
        e2 <- new.env()
        e1$x <- 42L
        out <- open_file(p)
        save(x, envir = e1, file = out)
        stopifnot(!isOpen(out))
        stopifnot(identical(load(out, envir = e2), "x"), identical(e2$x, 42L), !isOpen(out))
        close(out)
    }
    save(x, envir = e1, file = p)
    input <- file(p, "rb")
    stopifnot(identical(load(input, envir = e2), "x"), isOpen(input))
    close(input)
    unlink(p)
"#;
    if let Some(executable) = env::var_os("ROXIDE_ORACLE_RSCRIPT") {
        oracle(Path::new(&executable), script);
    }
    run(
        &mut context(Box::new(PureHost::new())),
        "serialized connections",
        script,
    );
}

#[test]
fn save_invokes_active_bindings_even_without_forcing_promises() {
    let mut ctx = context(Box::new(PureHost::new()));
    run(
        &mut ctx,
        "active binding setup",
        "reader <- function() 42L\ne <- new.env()",
    );
    let session = ctx.session_mut();
    let reader = session.intern("reader").unwrap();
    let reader = session.env_get(GLOBAL_ENV, reader).unwrap().value;
    let env = session.intern("e").unwrap();
    let env = session.env_get(GLOBAL_ENV, env).unwrap().value;
    let x = session.intern("x").unwrap();
    session
        .env_define_binding(
            env,
            x,
            roxide_value::Binding {
                value: reader,
                active: true,
                locked: false,
                missing: 0,
            },
        )
        .unwrap();
    run(
        &mut ctx,
        "save active binding",
        r#"
p <- tempfile()
save(x, envir = e, file = p, eval.promises = FALSE)
target <- new.env()
stopifnot(identical(load(p, envir = target), "x"), identical(target$x, 42L))
unlink(p)
"#,
    );
}

#[test]
fn decodes_long_parent_chains_and_large_attribute_lists() {
    let mut ctx = context(Box::new(PureHost::new()));
    let session = ctx.session_mut();
    let mut words = vec![19, 500];
    for i in 0..500 {
        let parent = if i == 0 { 242 } else { (i << 8) | 255 };
        words.extend([4, 0, parent, 254, 254, 254]);
    }
    let wire = session.alloc_raw_from(&xdr_body(&words)).unwrap();
    let parents = session.intern("parents").unwrap();
    session.env_define(GLOBAL_ENV, parents, wire).unwrap();

    let mut bytes = xdr_body(&[13 | (1 << 9), 1, 42]);
    for i in 0..1000 {
        let name = format!("attr{i}");
        bytes.extend(
            [2u32 | (1 << 10), 1, 9, name.len() as u32]
                .into_iter()
                .flat_map(u32::to_be_bytes),
        );
        bytes.extend(name.bytes());
        bytes.extend([13u32, 1, i].into_iter().flat_map(u32::to_be_bytes));
    }
    bytes.extend(254u32.to_be_bytes());
    let wire = session.alloc_raw_from(&bytes).unwrap();
    let attrs = session.intern("attrs").unwrap();
    session.env_define(GLOBAL_ENV, attrs, wire).unwrap();
    run(
        &mut ctx,
        "large XDR objects",
        "parents <- unserialize(parents)\nattrs <- unserialize(attrs)",
    );
    let session = ctx.session();
    let root = session.env_get(GLOBAL_ENV, parents).unwrap().value;
    let mut env = session.list_get(root, 499).unwrap();
    for i in (0..500).rev() {
        assert_eq!(env, session.list_get(root, i).unwrap());
        env = session.env_enclos(env).unwrap();
    }
    assert_eq!(env, roxide_value::EMPTY_ENV);
    let value = session.env_get(GLOBAL_ENV, attrs).unwrap().value;
    let attrs = session.attrs(value).unwrap();
    assert_eq!(attrs.len(), 1000);
    for (i, (key, value)) in attrs.into_iter().enumerate() {
        assert_eq!(session.sym_name(key).unwrap(), format!("attr{i}"));
        assert_eq!(session.int(value).unwrap().get(0), i as i32);
    }

    let two_environment_cycle = [4, 0, 4, 0, 0x1ff, 254, 254, 254, 254, 254, 254];
    rejects_raw(&mut ctx, &xdr_body(&two_environment_cycle));
}

#[test]
fn package_namespaces_and_primitives_roundtrip() {
    const MODULE: Module = Module {
        prims: &[PrimDef {
            name: "packagePrimitive",
            ..CAPTURE.prims[0]
        }],
        prelude: "twice <- function(x) x + x",
    };
    let mut ctx = context(Box::new(PureHost::new()));
    ctx.register_package(&roxide_eval::Package {
        name: "stats",
        attach: true,
        exports: None,
        modules: &[&MODULE],
    })
    .expect("Register test package");
    run(
        &mut ctx,
        "Package closure round trip",
        r#"
f <- stats::twice
p <- stats::packagePrimitive
for (version in c(2L, 3L)) {
    result <- unserialize(serialize(list(f, f, p), NULL, version=version))
    stopifnot(identical(result[[1L]](21), 42),
              identical(environment(result[[1L]]), environment(f)),
              identical(environment(result[[2L]]), environment(f)),
              identical(result[[3L]], p))
}
"#,
    );
    if let Some(executable) = env::var_os("ROXIDE_ORACLE_RSCRIPT") {
        let raw = oracle(
            Path::new(&executable),
            r#"cat(as.integer(serialize(asNamespace("stats"), NULL, version=2)), sep=",")"#,
        );
        run(
            &mut ctx,
            "GNU R namespace reference",
            &format!(
                r#"
bytes <- as.raw(c({raw}))
ns <- environment(stats::twice)
stopifnot(identical(serialize(ns, NULL, version=2), bytes),
          identical(unserialize(bytes), ns))
"#
            ),
        );
    }
}

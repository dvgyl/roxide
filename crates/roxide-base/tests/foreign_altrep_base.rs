#![forbid(unsafe_code)]

//! Test foreign ALTREP callbacks through the R evaluator.

use roxide_eval::{AltOp, AltReply, Ctx, CtxExt, EvalResult, NativeHooks};
use roxide_gc::{GcPolicy, SexpId, Trace, Tracer};
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, NA_INTEGER, SEXP_NIL, SexpType, ValueApi};

#[derive(Debug, PartialEq, Eq)]
enum Method {
    Length,
    Materialize,
    Sum(bool),
    Min(bool),
    Max(bool),
    NoNa,
    IsSorted,
    Extract(Vec<i32>),
    Coerce(SexpType),
}

#[derive(Clone, Copy, Default)]
enum Reply {
    #[default]
    Value,
    Null,
    Fallback,
}

struct Methods {
    calls: Vec<Method>,
    reply: Reply,
    sorted: i32,
    no_na: bool,
}

impl Default for Methods {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            reply: Reply::Value,
            sorted: 1,
            no_na: true,
        }
    }
}

impl Trace for Methods {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for Methods {}

struct Hooks;

impl NativeHooks for Hooks {
    fn altrep(&self, ctx: &mut Ctx, op: AltOp) -> EvalResult<AltReply> {
        use AltOp::*;
        let (x, method) = match op {
            Length(x) => (x, Method::Length),
            Materialize(x) => (x, Method::Materialize),
            Sum { x, na_rm } => (x, Method::Sum(na_rm)),
            Min { x, na_rm } => (x, Method::Min(na_rm)),
            Max { x, na_rm } => (x, Method::Max(na_rm)),
            NoNa(x) => (x, Method::NoNa),
            IsSorted(x) => (x, Method::IsSorted),
            ExtractSubset { x, indx, call } => {
                assert_eq!(ctx.session().type_of(call)?, SexpType::Lang);
                (x, Method::Extract(ctx.session().int(indx)?.to_vec()))
            }
            Coerce { x, to } => (x, Method::Coerce(to)),
            _ => panic!("unexpected method {op:?}"),
        };
        ctx.collect_gc();
        let methods = ctx.ext::<Methods>();
        methods.calls.push(method);
        let (reply, sorted, no_na) = (methods.reply, methods.sorted, methods.no_na);
        let s = ctx.session_mut();
        let data = s.foreign_alt(x)?.unwrap().data1;
        Ok(match op {
            Length(_) => AltReply::Len(s.xlength(data)?),
            Materialize(_) => {
                assert!(s.adopt_foreign_alt_payload(x, data)?);
                AltReply::Unit
            }
            NoNa(_) => AltReply::Flag(i32::from(no_na)),
            IsSorted(_) => AltReply::Flag(sorted),
            Sum { .. } | Min { .. } | Max { .. } => AltReply::Value(match reply {
                Reply::Value => Some(s.scalar_int(123)?),
                Reply::Null => Some(SEXP_NIL),
                Reply::Fallback => None,
            }),
            ExtractSubset { indx, .. } => AltReply::Value(match reply {
                Reply::Fallback => None,
                _ => {
                    let values: Vec<_> = s
                        .int(indx)?
                        .iter()
                        .map(|i| if i == NA_INTEGER { i } else { 100 + i })
                        .collect();
                    Some(s.alloc_int_from(&values)?)
                }
            }),
            Coerce { to, .. } => AltReply::Value(match reply {
                Reply::Fallback => None,
                _ if to == SexpType::List => Some(s.cons(data, SEXP_NIL, SEXP_NIL)?),
                _ if to == SexpType::Sym => Some(s.intern("converted")?.as_sexp()),
                _ => Some(s.coerce(data, to)?.0),
            }),
            _ => unreachable!(),
        })
    }
}

fn context(values: &[i32]) -> Ctx {
    let policy = if std::env::var_os("ROXIDE_GC_TORTURE").is_some() {
        GcPolicy::Torture
    } else {
        GcPolicy::default()
    };
    let mut ctx = Ctx::new(Box::new(PureHost::new()), policy).unwrap();
    roxide_base::register_all(&mut ctx).unwrap();
    ctx.set_native_hooks(&Hooks);
    let s = ctx.session_mut();
    let class = s.intern("foreign_integer").unwrap();
    let package = s.intern("base_test").unwrap();
    let descriptor = s.alloc_alt_class(class, package, 0).unwrap();
    let data = s.alloc_int_from(values).unwrap();
    let x = s
        .alloc_foreign_alt(descriptor, SexpType::Int, data, SEXP_NIL)
        .unwrap();
    let symbol = s.intern("x").unwrap();
    s.env_define(GLOBAL_ENV, symbol, x).unwrap();
    let nested = s.alloc_list(1).unwrap();
    s.list_set(nested, 0, x).unwrap();
    let symbol = s.intern("nested").unwrap();
    s.env_define(GLOBAL_ENV, symbol, nested).unwrap();
    ctx
}

fn evaluate(ctx: &mut Ctx, code: &str) -> SexpId {
    ctx.run_global("<foreign-altrep-base>", &format!("result <- {{ {code} }}"))
        .unwrap_or_else(|error| panic!("{code}: {error:?}: {}", ctx.last_error_message()));
    let result = ctx.session_mut().intern("result").unwrap();
    ctx.get_var(result, GLOBAL_ENV).unwrap().unwrap()
}

fn assert_calls(ctx: &mut Ctx, expected: &[Method]) {
    assert_eq!(ctx.ext::<Methods>().calls, expected);
}

#[test]
fn length_calls_only_length_and_seals_it() {
    let mut ctx = context(&[7, 8, 9]);
    let result = evaluate(&mut ctx, "c(length(x), length(x))");
    assert_eq!(ctx.session().int(result).unwrap().to_vec(), [3, 3]);
    assert_calls(&mut ctx, &[Method::Length]);
}

#[test]
fn summaries_use_callback_values() {
    let mut ctx = context(&[7, 8, 9]);
    let result = evaluate(&mut ctx, "c(sum(x), min(x, na.rm=TRUE), max(x))");
    assert_eq!(ctx.session().int(result).unwrap().to_vec(), [123; 3]);
    assert_calls(
        &mut ctx,
        &[Method::Sum(false), Method::Min(true), Method::Max(false)],
    );
}

#[test]
fn summary_distinguishes_r_null_from_a_missing_reply() {
    let mut ctx = context(&[7, NA_INTEGER, 9]);
    ctx.ext::<Methods>().reply = Reply::Null;
    assert_eq!(evaluate(&mut ctx, "sum(x)"), SEXP_NIL);
    assert_calls(&mut ctx, &[Method::Sum(false)]);

    ctx.ext::<Methods>().reply = Reply::Fallback;
    let result = evaluate(&mut ctx, "sum(x, na.rm=TRUE)");
    assert_eq!(ctx.session().int(result).unwrap().get(0), 16);
    assert_calls(
        &mut ctx,
        &[Method::Sum(false), Method::Sum(true), Method::Materialize],
    );
}

#[test]
fn summary_uses_dense_reduction_for_multiple_inputs() {
    let mut ctx = context(&[7, 8, 9]);
    let result = evaluate(&mut ctx, "c(sum(x, 1L), min(0L, x), max(x, 10L))");
    assert_eq!(ctx.session().int(result).unwrap().to_vec(), [25, 0, 10]);
    assert_calls(&mut ctx, &[Method::Materialize]);
}

#[test]
fn any_na_uses_the_guarantee_or_scans() {
    let mut ctx = context(&[7, 8, 9]);
    let result = evaluate(&mut ctx, "anyNA(x)");
    assert_eq!(ctx.session().lgl(result).unwrap().get(0).get(), Some(false));
    assert_calls(&mut ctx, &[Method::NoNa]);

    let mut ctx = context(&[7, NA_INTEGER, 9]);
    ctx.ext::<Methods>().no_na = false;
    let result = evaluate(&mut ctx, "anyNA(nested, recursive=TRUE)");
    assert_eq!(ctx.session().lgl(result).unwrap().get(0).get(), Some(true));
    assert_calls(&mut ctx, &[Method::NoNa, Method::Materialize]);
}

#[test]
fn sortedness_keeps_the_source_lazy_when_metadata_is_sufficient() {
    for (values, flag, code, expected, calls) in [
        (
            [7, 8, 9],
            1,
            "is.unsorted(x)",
            false,
            vec![Method::Length, Method::IsSorted, Method::NoNa],
        ),
        (
            [NA_INTEGER, 7, 8],
            2,
            "is.unsorted(x, na.rm=TRUE)",
            false,
            vec![Method::Length, Method::IsSorted],
        ),
        (
            [9, 8, 7],
            -1,
            "is.unsorted(x, strictly=TRUE)",
            true,
            vec![Method::Length, Method::IsSorted, Method::NoNa],
        ),
        (
            [8, 7, 9],
            0,
            "is.unsorted(x)",
            true,
            vec![Method::Length, Method::IsSorted, Method::NoNa],
        ),
    ] {
        let mut ctx = context(&values);
        ctx.ext::<Methods>().no_na = !values.contains(&NA_INTEGER);
        ctx.ext::<Methods>().sorted = flag;
        let result = evaluate(&mut ctx, code);
        assert_eq!(
            ctx.session().lgl(result).unwrap().get(0).get(),
            Some(expected)
        );
        assert_calls(&mut ctx, &calls);
    }
}

#[test]
fn sortedness_checks_ties_and_missing_values() {
    for (values, flag, code, expected) in [
        ([7, 7, 8], 1, "is.unsorted(x, strictly=TRUE)", Some(true)),
        ([7, 7, 7], -1, "is.unsorted(x)", Some(false)),
        ([NA_INTEGER, 8, 9], 2, "is.unsorted(x)", None),
        (
            [NA_INTEGER, 9, 8],
            -2,
            "is.unsorted(x, na.rm=TRUE)",
            Some(true),
        ),
        ([9, NA_INTEGER, 7], i32::MIN, "is.unsorted(x)", None),
    ] {
        let mut ctx = context(&values);
        ctx.ext::<Methods>().sorted = flag;
        ctx.ext::<Methods>().no_na = false;
        let result = evaluate(&mut ctx, code);
        assert_eq!(ctx.session().lgl(result).unwrap().get(0).get(), expected);
        assert_eq!(
            ctx.ext::<Methods>().calls.last(),
            Some(&Method::Materialize)
        );
    }
}

#[test]
fn subset_converts_a_logical_index_and_attaches_names() {
    let mut ctx = context(&[7, 8, 9]);
    let x = evaluate(&mut ctx, "x");
    let names = evaluate(&mut ctx, "c('one', 'two', 'three')");
    assert_eq!(ctx.altrep_len(x).unwrap(), 3);
    let s = ctx.session_mut();
    let sym = s.intern("names").unwrap();
    s.set_attrib(x, sym, names).unwrap();
    let result = evaluate(&mut ctx, "x[c(TRUE, FALSE, TRUE)]");
    let s = ctx.session();
    assert_eq!(s.int(result).unwrap().to_vec(), [101, 103]);
    let names = s.str_slice(s.attr(result, sym)).unwrap();
    assert_eq!(s.char_bytes(names[0]).unwrap(), b"one");
    assert_eq!(s.char_bytes(names[1]).unwrap(), b"three");

    let out_of_range = evaluate(&mut ctx, "x[c(FALSE, TRUE, FALSE, TRUE)]");
    let s = ctx.session();
    assert_eq!(s.int(out_of_range).unwrap().to_vec(), [102, 104]);
    assert_eq!(
        s.str_slice(s.attr(out_of_range, sym)).unwrap()[1],
        roxide_value::CharId::NA
    );
    assert_calls(
        &mut ctx,
        &[
            Method::Length,
            Method::Extract(vec![1, 3]),
            Method::Extract(vec![2, 4]),
        ],
    );
}

#[test]
fn subset_falls_back_for_a_missing_reply_or_multiple_indices() {
    let mut ctx = context(&[7, 8, 9]);
    ctx.ext::<Methods>().reply = Reply::Fallback;
    let result = evaluate(&mut ctx, "x[-2L]");
    assert_eq!(ctx.session().int(result).unwrap().to_vec(), [7, 9]);
    assert_calls(
        &mut ctx,
        &[
            Method::Length,
            Method::Extract(vec![1, 3]),
            Method::Materialize,
        ],
    );

    let mut ctx = context(&[7, 8, 9]);
    let x = evaluate(&mut ctx, "x");
    let dim = evaluate(&mut ctx, "c(1L, 3L)");
    let s = ctx.session_mut();
    let sym = s.intern("dim").unwrap();
    s.seal_foreign_alt_len(x, 3).unwrap();
    s.set_attrib(x, sym, dim).unwrap();
    let result = evaluate(&mut ctx, "x[1L, 2L]");
    assert_eq!(ctx.session().int(result).unwrap().to_vec(), [8]);
    assert_calls(&mut ctx, &[Method::Materialize]);
}

#[test]
fn coercion_uses_the_requested_type_or_materializes() {
    let mut ctx = context(&[7, 8, 9]);
    let result = evaluate(&mut ctx, "as.double(x)");
    assert_eq!(
        ctx.session().real(result).unwrap().to_vec(),
        [7.0, 8.0, 9.0]
    );
    assert_calls(&mut ctx, &[Method::Coerce(SexpType::Real)]);

    ctx.ext::<Methods>().reply = Reply::Fallback;
    let result = evaluate(&mut ctx, "as.vector(x, 'double')");
    assert_eq!(
        ctx.session().real(result).unwrap().to_vec(),
        [7.0, 8.0, 9.0]
    );
    assert_calls(
        &mut ctx,
        &[
            Method::Coerce(SexpType::Real),
            Method::Coerce(SexpType::Real),
            Method::Materialize,
        ],
    );
}

#[test]
fn classed_vectors_and_special_modes_use_the_coercion_method() {
    let mut ctx = context(&[7, 8, 9]);
    let x = evaluate(&mut ctx, "x");
    let s = ctx.session_mut();
    let class = s.intern("class").unwrap();
    let label = s.scalar_str("example").unwrap();
    s.set_attrib(x, class, label).unwrap();
    let result = evaluate(&mut ctx, "as.double(x)");
    assert_eq!(
        ctx.session().real(result).unwrap().to_vec(),
        [7.0, 8.0, 9.0]
    );
    let result = evaluate(&mut ctx, "as.vector(x, 'pairlist')");
    assert_eq!(ctx.session().type_of(result).unwrap(), SexpType::List);
    let result = evaluate(&mut ctx, "as.vector(x, 'symbol')");
    assert_eq!(ctx.session().sym_name_of(result).unwrap(), "converted");
    assert_calls(
        &mut ctx,
        &[
            Method::Coerce(SexpType::Real),
            Method::Coerce(SexpType::List),
            Method::Coerce(SexpType::Sym),
        ],
    );
}

#[test]
fn container_walks_read_nested_foreign_elements() {
    for code in [
        "identical(unlist(nested), c(7L, 8L, 9L))",
        "identical(c(nested, recursive=TRUE), c(7L, 8L, 9L))",
        "identical(nested, list(c(7L, 8L, 9L)))",
        "identical(print(nested), list(c(7L, 8L, 9L)))",
        "isTRUE(all.equal(nested, list(c(7L, 8L, 9L))))",
        "identical(deparse(nested), deparse(list(c(7L, 8L, 9L))))",
        "identical(deparse1(nested), deparse1(list(c(7L, 8L, 9L))))",
        "identical(dput(nested), list(c(7L, 8L, 9L)))",
    ] {
        let mut ctx = context(&[7, 8, 9]);
        let result = evaluate(&mut ctx, code);
        assert_eq!(
            ctx.session().lgl(result).unwrap().get(0).get(),
            Some(true),
            "{code}"
        );
        assert_calls(&mut ctx, &[Method::Materialize]);
    }
}

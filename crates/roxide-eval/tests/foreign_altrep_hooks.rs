//! Foreign ALTREP hook and evaluator tests.

use roxide_eval::vm::{compile_expr, run_compiled};
use roxide_eval::{
    AltOp, AltReply, ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, InternalError, Module,
    NativeHooks, PrimDef, PrimKind, Unwind, Visibility,
};
use roxide_gc::{GcPolicy, SexpId};
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, SEXP_NIL, SexpType, Sortedness, ValueApi};

struct Hook;

impl NativeHooks for Hook {
    fn altrep(&self, ctx: &mut Ctx, op: AltOp) -> EvalResult<AltReply> {
        ctx.collect_gc();
        let session = ctx.session_mut();
        Ok(match op {
            AltOp::Length(x) => {
                let info = session.foreign_alt(x)?.unwrap();
                assert_eq!(info.len, None);
                AltReply::Len(session.xlength(info.data1)?)
            }
            AltOp::Materialize(x) => {
                let dense = session.foreign_alt(x)?.unwrap().data1;
                assert!(session.adopt_foreign_alt_payload(x, dense)?);
                AltReply::Unit
            }
            AltOp::IsSorted(x) | AltOp::NoNa(x) => {
                let flag = session.foreign_alt(x)?.unwrap().data2;
                AltReply::Flag(session.int(flag)?.get(0))
            }
            AltOp::Sum { x, na_rm: true }
            | AltOp::Min { x, na_rm: true }
            | AltOp::Max { x, na_rm: true }
            | AltOp::SerializedState(x) => AltReply::Value(Some(x)),
            AltOp::ExtractSubset { indx, .. } => AltReply::Value(Some(indx)),
            AltOp::Coerce { x, to } if session.type_of(x)? == to => AltReply::Value(Some(x)),
            AltOp::Unserialize { state, .. } => AltReply::Value(Some(state)),
            _ => AltReply::Value(None),
        })
    }
}

fn context() -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).unwrap();
    ctx.set_native_hooks(&Hook);
    ctx
}

fn foreign(ctx: &mut Ctx, dense: SexpId, flag: i32) -> SexpId {
    let s = ctx.session_mut();
    let class = s.intern("foreign_vector").unwrap();
    let pkg = s.intern("test_package").unwrap();
    let class = s.alloc_alt_class(class, pkg, 0).unwrap();
    let flag = s.scalar_int(flag).unwrap();
    let ty = s.type_of(dense).unwrap();
    s.alloc_foreign_alt(class, ty, dense, flag).unwrap()
}

fn ints(ctx: &mut Ctx, values: &[i32], flag: i32) -> SexpId {
    let dense = ctx.session_mut().alloc_int_from(values).unwrap();
    foreign(ctx, dense, flag)
}

fn define(ctx: &mut Ctx, name: &str, value: SexpId) {
    let sym = ctx.session_mut().intern(name).unwrap();
    ctx.session_mut()
        .env_define(GLOBAL_ENV, sym, value)
        .unwrap();
}

fn get(ctx: &mut Ctx, name: &str) -> SexpId {
    let sym = ctx.session_mut().intern(name).unwrap();
    ctx.get_var(sym, GLOBAL_ENV).unwrap().unwrap()
}

fn int(ctx: &Ctx, x: SexpId) -> i32 {
    ctx.session().int(x).unwrap().get(0)
}

fn owned(ctx: &Ctx, x: SexpId) -> bool {
    ctx.session().foreign_alt(x).unwrap().unwrap().owned
}

fn value_text(ctx: &mut Ctx, name: &str) -> String {
    let value = get(ctx, name);
    ctx.deparse_lines(value, Default::default())
        .unwrap()
        .join("\n")
}

fn ordinary(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let n = ctx.session().int(args.get(0))?.get(0);
    Ok(ctx.session_mut().scalar_int(n)?)
}

fn aware(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let len = ctx.altrep_len(args.get(0))?;
    Ok(ctx.session_mut().scalar_int(i32::try_from(len).unwrap())?)
}

const fn builtin(name: &'static str, fun: roxide_eval::PrimFn) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Primitive,
        args: ArgEval::Eval,
        arity: Arity::Exact(1),
        visibility: Visibility::On,
        dispatch: Dispatch::None,
    }
}

#[test]
fn primitives_resolve_arguments_unless_marked_aware() {
    let mut ctx = context();
    const PRIMS: &[PrimDef] = &[
        builtin("ordinary_alt", ordinary),
        builtin("aware_alt", aware),
        builtin("read_lazy_alt", ordinary),
    ];
    ctx.register_module(&Module {
        prims: PRIMS,
        prelude: "",
    })
    .unwrap();
    ctx.mark_altrep_aware(&["aware_alt", "read_lazy_alt"]);
    let eager = ints(&mut ctx, &[25, 30], 0);
    define(&mut ctx, "eager", eager);
    let lazy = ints(&mut ctx, &[8, 9, 10], 0);
    define(&mut ctx, "lazy", lazy);

    ctx.run_global(
        "<primitives>",
        r#"
        a <- ordinary_alt(eager)
        b <- aware_alt(lazy)
        escaped <- tryCatch(read_lazy_alt(lazy), error = function(e) e)
        message <- conditionMessage(escaped)
        origin <- conditionCall(escaped)
    "#,
    )
    .unwrap();

    assert_eq!(value_text(&mut ctx, "a"), "25L");
    assert_eq!(value_text(&mut ctx, "b"), "3L");
    assert!(owned(&ctx, eager));
    assert!(!owned(&ctx, lazy));
    let message = get(&mut ctx, "message");
    let chars = ctx.session().str_slice(message).unwrap();
    assert_eq!(
        ctx.session().char_bytes(chars[0]).unwrap(),
        b"foreign ALTREP payload is unavailable"
    );
    assert_eq!(value_text(&mut ctx, "origin"), "read_lazy_alt(lazy)");
}

#[test]
fn metadata_queries_cache_length_and_keep_the_vector_lazy() {
    let mut ctx = context();
    for (code, sorted) in [
        (1, Sortedness::Increasing),
        (2, Sortedness::IncreasingNaFirst),
        (-1, Sortedness::Decreasing),
        (-2, Sortedness::DecreasingNaFirst),
        (0, Sortedness::Unknown),
        (i32::MIN, Sortedness::Unknown),
    ] {
        let value = ints(&mut ctx, &[11, 12, 13], code);
        ctx.protect(value);
        assert_eq!(ctx.altrep_len(value).unwrap(), 3);
        assert_eq!(ctx.altrep_len(value).unwrap(), 3);
        assert_eq!(ctx.altrep_sorted(value).unwrap(), sorted);
        assert_eq!(ctx.altrep_no_na(value).unwrap(), code != 0);
        assert!(!owned(&ctx, value));
    }
    let builtin = ctx.session_mut().alloc_compact_intseq(1, 4, 1).unwrap();
    assert_eq!(ctx.altrep_len(builtin).unwrap(), 4);
    assert_eq!(ctx.altrep_sorted(builtin).unwrap(), Sortedness::Increasing);
    assert!(ctx.altrep_no_na(builtin).unwrap());
}

#[test]
fn methods_materialize_vector_results_and_preserve_serialized_state() {
    let mut ctx = context();
    let x = ints(&mut ctx, &[7], 0);
    let class = ctx.session_mut().intern("foreign_vector").unwrap();
    let package = ctx.session_mut().intern("test_package").unwrap();
    for op in [
        AltOp::SerializedState(x),
        AltOp::Unserialize {
            class,
            package,
            ty: SexpType::Int,
            state: x,
            attrs: SEXP_NIL,
            objf: 0,
            levels: 0,
        },
    ] {
        assert_eq!(ctx.altrep_method(op).unwrap(), Some(x));
        assert!(!owned(&ctx, x));
    }
    assert_eq!(
        ctx.altrep_method(AltOp::Coerce {
            x,
            to: SexpType::Real
        })
        .unwrap(),
        None
    );

    let x = ints(&mut ctx, &[7], 0);
    ctx.protect(x);
    let indx = ints(&mut ctx, &[8], 0);
    assert_eq!(
        ctx.altrep_method(AltOp::ExtractSubset {
            x,
            indx,
            call: SEXP_NIL
        })
        .unwrap(),
        Some(indx)
    );
    assert_eq!(int(&ctx, indx), 8);
    assert!(!owned(&ctx, x));

    for make_op in [
        (|x| AltOp::Coerce {
            x,
            to: SexpType::Int,
        }) as fn(SexpId) -> AltOp,
        |x| AltOp::Sum { x, na_rm: true },
        |x| AltOp::Min { x, na_rm: true },
        |x| AltOp::Max { x, na_rm: true },
    ] {
        let x = ints(&mut ctx, &[7], 0);
        assert_eq!(ctx.altrep_method(make_op(x)).unwrap(), Some(x));
        assert_eq!(int(&ctx, x), 7);
    }
}

#[test]
fn resolution_handles_attributes_lists_and_pairlist_cycles() {
    let mut ctx = context();
    let attr = ints(&mut ctx, &[4], 0);
    let item = ints(&mut ctx, &[5], 0);
    let behind_promise = ints(&mut ctx, &[6], 0);
    let foreign_list = {
        let s = ctx.session_mut();
        let promise = s.alloc_promise(behind_promise, GLOBAL_ENV).unwrap();
        let pair = s.cons(item, SEXP_NIL, SEXP_NIL).unwrap();
        s.set_cdr(pair, pair).unwrap();
        let expr = s.alloc_expr_from(&[pair, promise]).unwrap();
        let list = s.alloc_list(1).unwrap();
        s.list_set(list, 0, expr).unwrap();
        foreign(&mut ctx, list, 0)
    };
    let label = ctx.session_mut().intern("label").unwrap();
    ctx.session_mut()
        .set_attrib(foreign_list, label, attr)
        .unwrap();

    ctx.resolve_altrep(foreign_list).unwrap();
    assert!(owned(&ctx, foreign_list));
    assert_eq!(int(&ctx, attr), 4);
    assert!(!owned(&ctx, item));
    ctx.resolve_altrep_deep(foreign_list).unwrap();
    assert_eq!(int(&ctx, item), 5);
    assert!(!owned(&ctx, behind_promise));
}

#[test]
fn missing_hooks_and_invalid_replies_return_internal_errors() {
    struct DefaultHook;
    impl NativeHooks for DefaultHook {}
    struct WrongReply;
    impl NativeHooks for WrongReply {
        fn altrep(&self, _: &mut Ctx, _: AltOp) -> EvalResult<AltReply> {
            Ok(AltReply::Unit)
        }
    }

    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).unwrap();
    let x = ints(&mut ctx, &[1], 0);
    let missing = Err(Unwind::Internal(InternalError {
        message: "foreign ALTREP hook not installed".into(),
    }));
    assert_eq!(ctx.altrep_len(x), missing);
    ctx.set_native_hooks(&DefaultHook);
    assert_eq!(ctx.altrep_len(x), missing);
    ctx.set_native_hooks(&WrongReply);
    assert!(matches!(ctx.altrep_len(x), Err(Unwind::Internal(_))));
    assert!(!owned(&ctx, x));
}

#[test]
fn ast_and_vm_handle_foreign_conditions_sequences_and_operands() {
    for compiled in [false, true] {
        let mut ctx = context();
        ctx.set_jit_level(0);
        let cond = ints(&mut ctx, &[1], 0);
        define(&mut ctx, "cond", cond);
        let seq = ints(&mut ctx, &[1, 2, 3], 0);
        define(&mut ctx, "seq", seq);
        let dense = ctx.session_mut().scalar_str("pick").unwrap();
        let key = foreign(&mut ctx, dense, 0);
        define(&mut ctx, "key", key);
        let operand = ints(&mut ctx, &[5], 0);
        define(&mut ctx, "operand", operand);
        let subset = ints(&mut ctx, &[7, 8], 0);
        define(&mut ctx, "subset_value", subset);
        assert_eq!(ctx.altrep_len(subset).unwrap(), 2);

        ctx.run_global(
            "<control>",
            r#"
            n <- 0L
            seq_value <- function() { n <<- n + 1L; seq }
            expr <- quote({
                total <- 0L
                if (cond) for (i in seq_value()) {
                    if (i == 2L) next
                    total <- total + i
                }
                sum <- operand + 2L
                element <- subset_value[2L]
                list(total, n, switch(key, pick = 6L), sum, element)
            })
        "#,
        )
        .unwrap();
        let expr = get(&mut ctx, "expr");
        let result = if compiled {
            let code = compile_expr(ctx.session(), expr).unwrap();
            run_compiled(&mut ctx, &code, GLOBAL_ENV).unwrap()
        } else {
            ctx.eval(expr, GLOBAL_ENV).unwrap()
        };
        define(&mut ctx, "result", result);
        assert_eq!(value_text(&mut ctx, "result"), "list(4L, 1L, 6L, 7L, 8L)");
    }
}

#[test]
fn literal_assignment_errors_match_the_gnu_r_diagnostics() {
    let mut ctx = context();
    ctx.run_global(
        "<assign>",
        r#"
        f <- function(x) x
        `f<-` <- function(x, value) value
        nested <- tryCatch(f(1) <- 2, error = conditionMessage)
        nested_string <- tryCatch(f("x") <- 2, error = conditionMessage)
        direct <- tryCatch(1 <- 2, error = conditionMessage)
    "#,
    )
    .unwrap();
    assert_eq!(
        value_text(&mut ctx, "nested"),
        "\"target of assignment expands to non-language object\""
    );
    assert_eq!(
        value_text(&mut ctx, "nested_string"),
        value_text(&mut ctx, "nested")
    );
    assert_eq!(
        value_text(&mut ctx, "direct"),
        "\"invalid (do_set) left-hand side to assignment\""
    );
}

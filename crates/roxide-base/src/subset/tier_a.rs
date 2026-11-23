#[path = "sets.rs"]
mod sets;
#[path = "unlist.rs"]
mod unlist;

use roxide_eval::{ArgEval, Arity, Dispatch, Module, PrimDef, PrimFn, PrimKind, Visibility};

pub(crate) const MODULE: Module = Module {
    prims: &[
        internal("rox_match", sets::match_values, 4),
        internal("rox_duplicated", sets::duplicated, 4),
        internal("rox_unlist", unlist::unlist, 3),
        internal("rox_expression_vector", expression_vector, 1),
    ],
    prelude: include_str!("tier_a.R"),
};

const fn internal(name: &'static str, fun: PrimFn, n: usize) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(n),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
    }
}

fn expression_vector(
    ctx: &mut roxide_eval::Ctx,
    call: &roxide_eval::Call,
    args: &mut roxide_eval::Args,
) -> roxide_eval::EvalResult {
    use roxide_value::{SEXP_NIL, ValueApi};

    let n = crate::ops::common::number(ctx.session_mut(), args.get(0))?;
    let n = crate::ops::common::count(ctx, call, n)?;
    crate::ops::common::vector_limit(ctx, call, n, size_of::<roxide_gc::SexpId>())?;
    Ok(ctx.session_mut().alloc_expr_from(&vec![SEXP_NIL; n])?)
}

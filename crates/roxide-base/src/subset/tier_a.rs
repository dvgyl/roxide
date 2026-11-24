#[path = "sets.rs"]
mod sets;
#[path = "unlist.rs"]
mod unlist;

use roxide_eval::{ArgEval, Arity, Dispatch, Module, PrimDef, PrimFn, PrimKind, Visibility};
use roxide_value::{NA_INTEGER, ValueApi};

use crate::ops::{attributes::row_count, common};

pub(crate) const MODULE: Module = Module {
    prims: &[
        internal("rox_match", sets::match_values, 4),
        internal("rox_duplicated", sets::duplicated, 4),
        internal("rox_unlist", unlist::unlist, 3),
        internal("rox_expression_vector", expression_vector, 1),
        internal("rox_transform_row_names", transform_row_names, 2),
    ],
    prelude: concat!(include_str!("tier_a.R"), include_str!("data_frame.R")),
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

fn transform_row_names(
    ctx: &mut roxide_eval::Ctx,
    call: &roxide_eval::Call,
    args: &mut roxide_eval::Args,
) -> roxide_eval::EvalResult {
    let x = args.get(0);
    let size = common::number(ctx.session_mut(), args.get(1))?;
    let size = common::count(ctx, call, size)?;
    let rows = row_count(ctx.session_mut(), x)?;
    let s = ctx.session();
    let labels = common::attr(s, x, "row.names");
    if size == rows {
        return Ok(labels);
    }
    let automatic = s.int(labels).is_ok_and(|v| {
        v.len() == 2 && v.get(0) == NA_INTEGER && v.get(1) != NA_INTEGER && v.get(1) < 0
    });
    let size =
        i32::try_from(size).map_err(|_| common::error(ctx, call, "too many rows in data frame"))?;
    if rows != 0 && !automatic {
        ctx.warn_now(
            call.err_call(),
            "row names were found from a short variable and have been discarded",
        )?;
    }
    Ok(ctx.session_mut().alloc_int_from(&[NA_INTEGER, -size])?)
}

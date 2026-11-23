//! Grid units, graphical parameters, and viewport geometry.

mod gpar;
mod layout;
mod unit;
mod viewport;

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind,
    Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{SexpType, ValueApi};

/// Evaluator bindings for the grid package.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_grid_s3", 5, register_s3),
        internal("C_grid_device", 0, viewport::device_info),
        internal("C_grid_state", 0, viewport::state),
        internal("C_grid_set_state", 1, viewport::set_state),
        internal("C_grid_page", 0, viewport::page),
        internal("C_grid_clip", 1, viewport::clip),
        internal("C_grid_colors", 2, gpar::validate_colors),
        internal("C_grid_match", 2, unit::match_indices),
        internal("C_grid_metrics", 4, unit::metrics),
        internal("C_grid_layout_tracks", 7, layout::tracks),
    ],
    prelude: include_str!("prelude.R"),
};

const fn internal(name: &'static str, arity: usize, fun: PrimFn) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(arity),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
        fun,
    }
}

fn numbers(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<Vec<f64>> {
    if ctx.session().xlength(value)? > ctx.host().limits().max_vector_bytes / size_of::<f64>() {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Real)?;
    Ok(ctx.session().real(value)?.iter().collect())
}

fn register_s3(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let generics = ["[<-", "as.double", "as.integer", "length", "rep"];
    for (i, generic) in generics.into_iter().enumerate() {
        ctx.ns_register_s3("base", generic, "unit", args.get(i))?;
    }
    Ok(roxide_value::SEXP_NIL)
}

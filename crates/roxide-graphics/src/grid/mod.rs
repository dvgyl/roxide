//! Grid units, graphical parameters, and viewport geometry.

mod draw;
mod fields;
mod gpar;
mod grob;
mod layout;
mod measure;
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
        internal("C_grid_unit", 3, measure::create),
        internal("C_grid_items", 1, measure::items),
        internal("C_grid_add", 2, measure::add),
        internal("C_grid_unit_text", 1, measure::as_char),
        internal("C_grid_unit_types", 2, measure::types),
        internal("C_grid_unit_extrema", 2, measure::extrema),
        internal("C_grid_convert", 8, measure::convert),
        internal("C_grid_state", 0, viewport::state),
        internal("C_grid_select", 2, viewport::select),
        internal("C_grid_page", 0, viewport::page),
        internal("C_grid_clip", 1, viewport::clip),
        internal("C_grid_match", 2, unit::match_indices),
        internal("C_grid_grob_name", 3, grob::next_name),
        internal("C_grid_grob_validate", 2, grob::validate),
        internal("C_grid_glist", 1, grob::create_list),
        internal("C_grid_children", 2, grob::children),
        internal("C_grid_draw", 3, draw::draw),
        internal("C_grid_bounds", 5, draw::bounds),
        internal("C_grid_viewport", 19, viewport::create),
        internal("C_grid_push", 4, viewport::push),
        internal("C_grid_up", 3, viewport::up),
        internal("C_grid_record", 4, viewport::record),
        internal("C_grid_gp", 2, gpar::merge),
    ],
    prelude: concat!(
        include_str!("prelude.R"),
        include_str!("grob.R"),
        include_str!("display.R"),
    ),
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

//! S4 generic registration and method dispatch.

mod groups;
mod invoke;
pub(crate) mod metadata;
mod method_table;
mod rank;
mod resolver;

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, FrameId, Module, PrimDef, PrimKind,
    Visibility,
};
use roxide_gc::SexpId;
use roxide_value::Sym;

/// S4 generic and method operations.
pub const MODULE: Module = Module {
    prims: &[
        prim(
            ".rx_setGeneric",
            metadata::set_generic,
            ArgEval::Eval,
            Visibility::On,
        ),
        prim(
            ".rx_generic",
            metadata::get_generic,
            ArgEval::Eval,
            Visibility::On,
        ),
        prim(
            ".rx_isGeneric",
            metadata::is_generic,
            ArgEval::Eval,
            Visibility::On,
        ),
        prim(
            ".rx_removeGeneric",
            metadata::remove_generic,
            ArgEval::Eval,
            Visibility::On,
        ),
        prim(
            ".rx_setMethod",
            method_table::set_method,
            ArgEval::Eval,
            Visibility::Off,
        ),
        prim(
            ".rx_removeMethod",
            method_table::remove_method,
            ArgEval::Eval,
            Visibility::On,
        ),
        prim(
            ".rx_selectMethod",
            metadata::select_method,
            ArgEval::Eval,
            Visibility::On,
        ),
        prim(
            ".rx_findMethods",
            metadata::find_methods,
            ArgEval::Eval,
            Visibility::On,
        ),
        prim(
            ".rx_showMethods",
            metadata::show_methods,
            ArgEval::Eval,
            Visibility::Off,
        ),
        prim(
            ".rx_getGroup",
            groups::get_group,
            ArgEval::Eval,
            Visibility::On,
        ),
        prim(
            ".rx_getGroupMembers",
            groups::get_members,
            ArgEval::Eval,
            Visibility::On,
        ),
        prim(
            ".rx_isGroup",
            groups::is_group,
            ArgEval::Eval,
            Visibility::On,
        ),
        prim(
            "callGeneric",
            invoke::call_generic,
            ArgEval::Unevaluated,
            Visibility::SetByFn,
        ),
        prim(
            "callNextMethod",
            invoke::call_next_method,
            ArgEval::Unevaluated,
            Visibility::SetByFn,
        ),
    ],
    prelude: include_str!("prelude.R"),
};

const fn prim(
    name: &'static str,
    fun: roxide_eval::PrimFn,
    args: ArgEval,
    visibility: Visibility,
) -> PrimDef {
    PrimDef {
        name,
        fun,
        args,
        visibility,
        arity: Arity::Any,
        dispatch: Dispatch::None,
        kind: PrimKind::Primitive,
    }
}

/// Dispatch an S4 primitive or group generic when it has an applicable method.
pub(crate) fn dispatch_primitive(
    ctx: &mut Ctx,
    generic: Sym,
    call: &Call,
    args: &Args,
) -> EvalResult<Option<SexpId>> {
    invoke::dispatch_primitive(ctx, generic, call, args)
}

/// Dispatch from the active frame of a standard generic.
pub(crate) fn standard_generic(ctx: &mut Ctx, name: Sym, frame: FrameId) -> EvalResult<SexpId> {
    invoke::standard_generic(ctx, name, frame)
}

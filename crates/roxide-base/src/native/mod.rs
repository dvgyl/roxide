//! Native calls and shared-library management.

mod call;
mod dll;

use roxide_eval::{
    ArgEval, Arity, Call, Ctx, Dispatch, EvalResult, Module, NativeHooks, PrimDef, PrimFn,
    PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{CharId, RLogical, SexpType, ValueApi};

use crate::ops::common::{chars, text};

/// Native call and DLL management functions in the base package.
pub const MODULE: Module = Module {
    prims: &[
        primitive(".Call", call::dot_call, Visibility::On),
        primitive(".External", call::external, Visibility::On),
        primitive(".External2", call::external2, Visibility::SetByFn),
        primitive(".C", call::dot_c, Visibility::On),
        primitive(".Fortran", call::fortran, Visibility::On),
        internal("rox_dyn_load", dll::load, 3, Visibility::Off),
        internal("rox_dyn_unload", dll::unload, 1, Visibility::Off),
        internal("rox_is_loaded", dll::is_loaded, 3, Visibility::On),
        internal(
            "rox_native_symbol_info",
            dll::symbol_info,
            4,
            Visibility::On,
        ),
        internal("rox_loaded_dlls", dll::loaded, 0, Visibility::On),
        internal(
            "rox_registered_routines",
            dll::registered,
            2,
            Visibility::On,
        ),
    ],
    prelude: include_str!("prelude.R"),
};

const fn primitive(name: &'static str, fun: PrimFn, visibility: Visibility) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Primitive,
        args: ArgEval::EvalKeepMissing,
        arity: Arity::Any,
        dispatch: Dispatch::None,
        visibility,
    }
}

const fn internal(name: &'static str, fun: PrimFn, n: usize, visibility: Visibility) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(n),
        dispatch: Dispatch::None,
        visibility,
    }
}

struct Unstaged;
impl NativeHooks for Unstaged {}

fn hooks(ctx: &Ctx) -> &'static dyn NativeHooks {
    ctx.native_hooks().unwrap_or(&Unstaged)
}

fn character_scalar(ctx: &mut Ctx, call: &Call, value: SexpId, name: &str) -> EvalResult<String> {
    match chars(ctx.session(), value) {
        &[id] if id != CharId::NA => Ok(text(ctx.session(), id)),
        _ => Err(ctx.stop(
            call.err_call(),
            format!("'{name}' must be a single non-missing character string"),
        )),
    }
}

fn logical(ctx: &mut Ctx, call: &Call, value: SexpId, name: &str) -> EvalResult<bool> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
    let values = ctx.session().lgl(value)?;
    if values.len() == 1
        && let Some(flag) = values.iter().next().and_then(RLogical::get)
    {
        return Ok(flag);
    }
    Err(ctx.stop(
        call.err_call(),
        format!("'{name}' must be a single non-missing logical value"),
    ))
}

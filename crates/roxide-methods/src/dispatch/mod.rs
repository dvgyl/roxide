//! S4 generic functions, multiple dispatch resolver, `callNextMethod()`, primitive & group generics.

use roxide_eval::{Args, Call, Ctx, EvalResult, FrameId, Module};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, Sym};

/// S4 dispatch submodule (owned by M3 Builder B).
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

/// Attempt S4 primitive/group generic dispatch; returns `Ok(None)` to fall through.
pub(crate) fn dispatch_primitive(
    _ctx: &mut Ctx,
    _generic: Sym,
    _call: &Call,
    _args: &Args,
) -> EvalResult<Option<SexpId>> {
    Ok(None)
}

/// Execute `standardGeneric(name)` in active call `frame`.
pub(crate) fn standard_generic(ctx: &mut Ctx, _name: Sym, _frame: FrameId) -> EvalResult<SexpId> {
    Err(ctx.stop(SEXP_NIL, "S4 dispatch not yet implemented"))
}

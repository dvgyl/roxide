//! S4 class definitions, slots, inheritance, `new()`, `validObject()`, `is()`/`as()`.

use roxide_eval::{Ctx, EvalResult, Module};
use roxide_gc::SexpId;
use roxide_value::Sym;

/// S4 class submodule (owned by M3 Builder A).
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

/// Return the S4 superclass chain (`STRSXP`) for `value`, if registered.
pub(crate) fn class_chain(_ctx: &mut Ctx, _value: SexpId) -> EvalResult<Option<SexpId>> {
    Ok(None)
}

/// Validate and coerce `value` prior to storing into slot `name` of S4 object `obj`.
pub(crate) fn check_slot(
    _ctx: &mut Ctx,
    _obj: SexpId,
    _name: Sym,
    value: SexpId,
) -> EvalResult<SexpId> {
    Ok(value)
}

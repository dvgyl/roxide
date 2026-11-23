//! S4 `show()` printing support.

use roxide_eval::{Ctx, EvalResult, Module};
use roxide_gc::SexpId;

/// S4 `show()` submodule (owned by M3 Builder B).
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

/// Print an S4 value via `show()`.
pub(crate) fn show_object(ctx: &mut Ctx, value: SexpId, _env: SexpId) -> EvalResult<()> {
    ctx.print_value_default(value, ctx.opts().digits)
}

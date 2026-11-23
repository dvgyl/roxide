//! `loadNamespace`, `attachNamespace`, `unloadNamespace`, `requireNamespace`, and `NamespaceLoader`.

use roxide_eval::{Ctx, EvalResult, Module, NamespaceLoader};
use roxide_gc::SexpId;

/// Package loader base primitives (owned by M3 Builder C).
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

/// Stateless [`NamespaceLoader`] installed into [`Ctx`] by `roxide-pkg::register`.
pub struct PkgNamespaceLoader;

/// Global singleton loader hook.
pub static LOADER: PkgNamespaceLoader = PkgNamespaceLoader;

impl NamespaceLoader for PkgNamespaceLoader {
    fn load(&self, ctx: &mut Ctx, name: &str, call: SexpId) -> EvalResult<SexpId> {
        Err(ctx.stop(call, format!("there is no package called '{name}'")))
    }
}

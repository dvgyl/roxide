//! Register the built-in statistical datasets.

use roxide_eval::{Ctx, InternalError, Module, Package};

const MODULE: Module = Module {
    prims: &[],
    prelude: include_str!("datasets.R"),
};

/// Package descriptor for the default `datasets` package.
pub const PACKAGE: Package = Package {
    name: "datasets",
    attach: true,
    exports: None,
    modules: &[&MODULE],
};

/// Register and attach the `datasets` package.
pub fn register(ctx: &mut Ctx) -> Result<(), InternalError> {
    ctx.register_package(&PACKAGE)
}

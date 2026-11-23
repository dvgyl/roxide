#![forbid(unsafe_code)]

//! Clean-room GNU R 4.6.2 `methods` package (S4 classes, multiple dispatch, Reference Classes; ADR-0012).

pub mod class;
pub mod dispatch;
pub mod meta;
pub mod refclass;
pub mod registry;
pub mod show;

mod hooks;

use roxide_eval::{Ctx, InternalError, Module, Package};

/// Submodules registered into `namespace:methods`.
pub const MODULES: &[&Module] = &[
    &class::MODULE,
    &dispatch::MODULE,
    &show::MODULE,
    &refclass::MODULE,
    &meta::MODULE,
];

/// Package descriptor for `methods` (attached on `search()` as `package:methods`).
pub const PACKAGE: Package = Package {
    name: "methods",
    attach: true,
    exports: None,
    modules: MODULES,
};

/// Register and attach `methods` and install the stateless [`roxide_eval::S4Hooks`].
pub fn register(ctx: &mut Ctx) -> Result<(), InternalError> {
    ctx.register_package(&PACKAGE)?;
    ctx.set_s4_hooks(&hooks::HOOKS);
    Ok(())
}

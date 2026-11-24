#![forbid(unsafe_code)]

//! Clean-room GNU R 4.6.2 `utils` and `tools` packages split into disjoint submodules.

mod archive;
pub mod datasets;
pub mod inspect;
pub mod pkgutils;
pub mod str;
pub mod table_io;
pub mod tools_license;
pub mod tools_rd;

use roxide_eval::{Ctx, InternalError, Module, Package};

/// Submodules registered into `namespace:utils`.
pub const UTILS_MODULES: &[&Module] = &[
    &table_io::MODULE,
    &str::MODULE,
    &inspect::MODULE,
    &pkgutils::MODULE,
    &archive::MODULE,
    &pkgutils::PACKAGE_TOOLS,
    &pkgutils::NAMESPACE_TOOLS,
];

/// Submodules registered into `namespace:tools`.
pub const TOOLS_MODULES: &[&Module] = &[&tools_rd::MODULE, &tools_license::MODULE];

/// Package descriptor for `utils` (attached on `search()` as `package:utils`).
pub const UTILS_PACKAGE: Package = Package {
    name: "utils",
    attach: true,
    exports: None,
    modules: UTILS_MODULES,
};

/// Package descriptor for `tools` (loaded namespace `namespace:tools`, not attached by default).
pub const TOOLS_PACKAGE: Package = Package {
    name: "tools",
    attach: false,
    exports: None,
    modules: TOOLS_MODULES,
};

/// Register `utils` (attached) and `tools` (namespace-only) on `ctx`.
pub fn register(ctx: &mut Ctx) -> Result<(), InternalError> {
    ctx.register_package(&UTILS_PACKAGE)?;
    ctx.register_package(&TOOLS_PACKAGE)?;
    Ok(())
}

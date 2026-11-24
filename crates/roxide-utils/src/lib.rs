#![forbid(unsafe_code)]

//! Clean-room GNU R 4.6.2 `utils` and `tools` packages split into disjoint submodules.

mod adist;
mod archive;
pub mod datasets;
pub mod inspect;
pub mod pkgutils;
pub mod str;
pub mod table_io;
mod tools_build;
pub mod tools_license;
mod tools_md5;
pub mod tools_rd;

use roxide_eval::{Ctx, InternalError, Module, Package};
use roxide_value::ValueApi;

/// Submodules registered into `namespace:utils`.
pub const UTILS_MODULES: &[&Module] = &[
    &table_io::MODULE,
    &str::MODULE,
    &inspect::MODULE,
    &pkgutils::MODULE,
    &archive::MODULE,
    &pkgutils::PACKAGE_TOOLS,
    &pkgutils::NAMESPACE_TOOLS,
    &adist::MODULE,
];

/// Submodules registered into `namespace:tools`.
pub const TOOLS_MODULES: &[&Module] = &[
    &tools_rd::MODULE,
    &tools_license::MODULE,
    &tools_build::MODULE,
    &tools_md5::MODULE,
];

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
    // `.DollarNames` begins with `.` so `exports: None` skips it; export it explicitly and
    // register its built-in S3 methods in `namespace:utils` to match GNU R 4.6.2.
    if let Some(rec) = ctx.ns_record("utils").cloned() {
        let sym = ctx.session_mut().intern(".DollarNames")?;
        if let Some(binding) = ctx.session().env_get(rec.ns, sym) {
            if let Ok(Some(exports_env)) = ctx.ns_exports_env(rec.ns) {
                let label = ctx.session_mut().scalar_str(".DollarNames")?;
                ctx.session_mut().env_define(exports_env, sym, label)?;
            }
            if let Some(attached) = rec.attached {
                ctx.session_mut()
                    .env_define_binding(attached, sym, binding)?;
            }
        }
        for class in ["default", "list", "environment"] {
            let mname = format!(".DollarNames.{class}");
            let msym = ctx.session_mut().intern(&mname)?;
            if let Some(mbinding) = ctx.session().env_get(rec.ns, msym) {
                ctx.ns_register_s3("utils", ".DollarNames", class, mbinding.value)
                    .map_err(|err| InternalError {
                        message: format!("failed to register {mname}: {err:?}"),
                    })?;
            }
        }
    }
    ctx.register_package(&TOOLS_PACKAGE)?;
    Ok(())
}

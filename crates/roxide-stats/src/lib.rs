#![forbid(unsafe_code)]

//! Clean-room GNU R 4.6.2 `stats` package primitives and R-level wrappers.
//!
//! Split into disjoint worker-owned submodules wired together solely through [`PACKAGE`].

pub mod descriptive;
pub mod distn;
pub mod glm;
pub mod htest;
pub mod linmod;
pub mod model;
pub mod multivar;
pub mod optimize;
pub mod smooth;
pub mod ts;

use roxide_eval::{Ctx, InternalError, Module, Package};

/// Ordered list of `roxide-stats` submodules registered into `namespace:stats`.
pub const MODULES: &[&Module] = &[
    &distn::MODULE,
    &descriptive::MODULE,
    &model::MODULE,
    &linmod::MODULE,
    &glm::MODULE,
    &htest::MODULE,
    &optimize::MODULE,
    &ts::MODULE,
    &multivar::MODULE,
    &smooth::MODULE,
];

/// Package descriptor for `stats` (attached on `search()` as `package:stats`).
pub const PACKAGE: Package = Package {
    name: "stats",
    attach: true,
    exports: None,
    modules: MODULES,
};

/// Register and attach the `stats` package on `ctx`.
pub fn register(ctx: &mut Ctx) -> Result<(), InternalError> {
    ctx.register_package(&PACKAGE)
}

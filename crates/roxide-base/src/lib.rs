#![forbid(unsafe_code)]

//! Clean-room GNU R 4.6.2 `base` package primitives and R-level wrappers.
//!
//! Split into disjoint worker-owned submodules (`ops`, `subset`, `strings`,
//! `io_os`, `datetime`, `serialize`) wired together solely through [`MODULES`].

pub mod datetime;
pub mod io_os;
pub mod ops;
pub mod serialize;
pub mod strings;
pub mod subset;

use roxide_eval::{Ctx, InternalError, Module};

/// Ordered list of `roxide-base` submodules registered into `BASE_ENV`.
pub const MODULES: &[&Module] = &[
    &ops::MODULE,
    &subset::MODULE,
    &strings::MODULE,
    &io_os::MODULE,
    &datetime::MODULE,
    &serialize::MODULE,
];

/// Register all `roxide-base` primitives and clean-room R preludes on `ctx`.
pub fn register_all(ctx: &mut Ctx) -> Result<(), InternalError> {
    for module in MODULES {
        ctx.register_module(module)?;
    }
    Ok(())
}

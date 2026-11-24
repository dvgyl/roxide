#![forbid(unsafe_code)]

//! Clean-room GNU R 4.6.2 `base` package primitives and R-level wrappers.
//!
//! Split into disjoint worker-owned submodules (`ops`, `subset`, `strings`,
//! `io_os`, `datetime`, `serialize`, `random`, `linalg`, `special`, `scan`,
//! `dcf`) wired together solely through [`MODULES`].

mod compiler;
pub mod datetime;
pub mod dcf;
pub mod io_os;
pub mod linalg;
pub mod native;
pub mod ops;
pub mod random;
pub mod scan;
pub mod serialize;
pub mod special;
pub mod strings;
pub mod subset;

#[path = "ops/constants.rs"]
mod constants;
#[path = "strings/default_format.rs"]
mod default_format;
#[path = "strings/numbers.rs"]
mod numbers;
#[path = "io_os/tier_a.rs"]
mod tier_a_io;
#[path = "strings/tier_a.rs"]
mod tier_a_strings;
#[path = "subset/tier_a.rs"]
mod tier_a_subset;

use roxide_eval::{Ctx, InternalError, Module};

/// Ordered list of `roxide-base` submodules registered into `BASE_ENV`.
pub const MODULES: &[&Module] = &[
    &ops::MODULE,
    &subset::MODULE,
    &strings::MODULE,
    &io_os::MODULE,
    &constants::MODULE,
    &tier_a_subset::MODULE,
    &tier_a_strings::MODULE,
    &tier_a_io::MODULE,
    &datetime::MODULE,
    &serialize::MODULE,
    &random::MODULE,
    &linalg::MODULE,
    &special::MODULE,
    &scan::MODULE,
    &dcf::MODULE,
    &native::MODULE,
    &compiler::BASE,
];

/// Register all `roxide-base` primitives and clean-room R preludes on `ctx`.
pub fn register_all(ctx: &mut Ctx) -> Result<(), InternalError> {
    for module in MODULES {
        ctx.register_module(module)?;
    }
    ctx.register_package(&compiler::PACKAGE)
}

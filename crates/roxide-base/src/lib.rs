#![forbid(unsafe_code)]

//! Clean-room GNU R 4.6.2 `base` package primitives and R-level wrappers.
//!
//! Split into disjoint worker-owned submodules (`ops`, `subset`, `strings`,
//! `io_os`, `datetime`, `serialize`, `random`, `linalg`, `special`, `scan`,
//! `dcf`) wired together solely through [`MODULES`].

pub mod datetime;
pub mod dcf;
pub mod io_os;
pub mod linalg;
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
use roxide_value::{BASE_ENV, ValueApi};

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
];

/// Register all `roxide-base` primitives and clean-room R preludes on `ctx`.
pub fn register_all(ctx: &mut Ctx) -> Result<(), InternalError> {
    for module in MODULES {
        ctx.register_module(module)?;
    }
    // Stats also registers a summary generic. Give it the base methods.
    for class in ["default", "matrix", "factor"] {
        let name = ctx.session_mut().intern(&format!("summary.{class}"))?;
        if let Some(method) = ctx.session().env_get(BASE_ENV, name) {
            ctx.ns_register_s3("stats", "summary", class, method.value)
                .map_err(|err| InternalError {
                    message: format!("could not register summary.{class}: {err:?}"),
                })?;
        }
    }
    Ok(())
}

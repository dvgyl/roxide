//! In-process staged `R CMD INSTALL` for pure-R packages (M3 W2-C; ADR-0014).

use roxide_eval::{Ctx, InternalError};
use roxide_host::Host;

/// Execute `R CMD INSTALL <args...>`, constructing fresh full-session [`Ctx`] instances via `boot`.
pub fn run(_args: &[String], _boot: &dyn Fn(Box<dyn Host>) -> Result<Ctx, InternalError>) -> i32 {
    1
}

//! `R CMD BATCH` execution driver (M3 W3-C; ADR-0014).

use roxide_eval::{Ctx, InternalError};
use roxide_host::Host;

/// Execute `R CMD BATCH [opts] infile [outfile]`, constructing a full-session [`Ctx`] via `boot`.
pub fn run(_args: &[String], _boot: &dyn Fn(Box<dyn Host>) -> Result<Ctx, InternalError>) -> i32 {
    1
}

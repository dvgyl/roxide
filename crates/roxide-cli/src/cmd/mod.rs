//! `R CMD` subcommand dispatcher (`INSTALL`, `BATCH`, `SHLIB`; owned by M3 Builder C).

use std::io::{self, Write};
use std::process::ExitCode;

use roxide_eval::{Ctx, InternalError};
use roxide_host::Host;

/// Dispatch `R CMD <subcommand> [args...]` using the full-session `boot` factory.
pub fn dispatch(
    args: &[String],
    boot: &dyn Fn(Box<dyn Host>) -> Result<Ctx, InternalError>,
) -> ExitCode {
    let Some((subcmd, rest)) = args.split_first() else {
        let _ = writeln!(io::stderr().lock(), "Usage: R CMD command args");
        return ExitCode::FAILURE;
    };
    let status = match subcmd.as_str() {
        "INSTALL" => roxide_pkg::install::run(rest, boot),
        "BATCH" => roxide_pkg::batch::run(rest, boot),
        "SHLIB" => {
            let _ = writeln!(
                io::stderr().lock(),
                "Error: R CMD SHLIB (compiled code) requires Milestone 4 (roxide-capi)"
            );
            1
        }
        other => {
            let _ = writeln!(io::stderr().lock(), "ERROR: unknown command '{other}'");
            1
        }
    };
    ExitCode::from(status.clamp(0, 255) as u8)
}

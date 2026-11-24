#![forbid(unsafe_code)]

//! Multi-call command-line binary for Roxide (`roxide`, `R`, `Rscript`, `R CMD`).

use std::process::ExitCode;

fn main() -> ExitCode {
    roxide_cli::run_main()
}

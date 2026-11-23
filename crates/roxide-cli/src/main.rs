#![forbid(unsafe_code)]

//! Multi-call command-line interface for Roxide (`roxide`, `R`, `Rscript`).

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use roxide_eval::{Ctx, Unwind};
use roxide_gc::GcPolicy;
use roxide_host::NativeHost;

const VERSION_LINE: &str = "Roxide 0.1.0 (targeting GNU R 4.6.2)";

#[derive(Default)]
struct CliOpts {
    show_version: bool,
    exprs: Vec<String>,
    file: Option<String>,
}

fn parse_cli() -> Result<CliOpts, String> {
    let mut opts = CliOpts::default();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--version" | "-V" => {
                opts.show_version = true;
                return Ok(opts);
            }
            "--vanilla"
            | "--slave"
            | "--no-echo"
            | "--no-restore"
            | "--no-save"
            | "--no-environ"
            | "--no-site-file"
            | "--no-init-file"
            | "--no-restore-data"
            | "--no-restore-history"
            | "--quiet"
            | "-q" => {}
            "-e" => {
                let code = args
                    .next()
                    .ok_or_else(|| String::from("option '-e' requires an argument"))?;
                opts.exprs.push(code);
            }
            "-f" => {
                let path = args
                    .next()
                    .ok_or_else(|| String::from("option '-f' requires an argument"))?;
                opts.file = Some(path);
            }
            "--args" | "--" => break,
            s if s.starts_with("--file=") => {
                opts.file = Some(s["--file=".len()..].to_owned());
            }
            s if s.starts_with("-e") && s.len() > 2 => {
                opts.exprs.push(s[2..].to_owned());
            }
            s if s.starts_with("--default-packages=") => {}
            s if s.starts_with('-') => {
                return Err(format!("unknown option '{s}'"));
            }
            path => {
                if opts.exprs.is_empty() && opts.file.is_none() {
                    opts.file = Some(path.to_owned());
                }
                break;
            }
        }
    }
    Ok(opts)
}

fn main() -> ExitCode {
    let opts = match parse_cli() {
        Ok(o) => o,
        Err(err) => {
            let _ = writeln!(io::stderr().lock(), "roxide: {err}");
            return ExitCode::FAILURE;
        }
    };

    if opts.show_version {
        let _ = writeln!(io::stdout().lock(), "{VERSION_LINE}");
        return ExitCode::SUCCESS;
    }

    let policy = if env::var_os("ROXIDE_GC_TORTURE").is_some() {
        GcPolicy::Torture
    } else {
        GcPolicy::default()
    };
    let host = Box::new(NativeHost::new());
    let mut ctx = match Ctx::new(host, policy) {
        Ok(c) => c,
        Err(err) => {
            let _ = writeln!(io::stderr().lock(), "Error: {err}");
            return ExitCode::FAILURE;
        }
    };

    if !opts.exprs.is_empty() {
        for expr in &opts.exprs {
            if let Err(unwind) = ctx.run_global("<expr>", expr) {
                return handle_unwind(unwind);
            }
        }
        return ExitCode::SUCCESS;
    }

    if let Some(path_str) = opts.file {
        let path = Path::new(&path_str);
        let src = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(err) => {
                let _ = writeln!(
                    io::stderr().lock(),
                    "Error: cannot open file '{path_str}': {err}"
                );
                return ExitCode::FAILURE;
            }
        };
        return match ctx.run_global(&path_str, &src) {
            Ok(()) => ExitCode::SUCCESS,
            Err(unwind) => handle_unwind(unwind),
        };
    }

    // Read stdin as a script if no `-e` or file was provided.
    let mut src = String::new();
    if let Err(err) = io::Read::read_to_string(&mut io::stdin().lock(), &mut src) {
        let _ = writeln!(io::stderr().lock(), "Error: {err}");
        return ExitCode::FAILURE;
    }
    match ctx.run_global("<stdin>", &src) {
        Ok(()) => ExitCode::SUCCESS,
        Err(unwind) => handle_unwind(unwind),
    }
}

fn handle_unwind(unwind: Unwind) -> ExitCode {
    match unwind {
        Unwind::Quit { status, .. } => ExitCode::from(status.clamp(0, 255) as u8),
        Unwind::TopLevel => {
            let _ = writeln!(io::stderr().lock(), "Execution halted");
            ExitCode::FAILURE
        }
        Unwind::Internal(err) => {
            let _ = writeln!(io::stderr().lock(), "Error: {err}\nExecution halted");
            ExitCode::FAILURE
        }
        Unwind::Transfer { .. } => {
            let _ = writeln!(io::stderr().lock(), "Execution halted");
            ExitCode::FAILURE
        }
    }
}

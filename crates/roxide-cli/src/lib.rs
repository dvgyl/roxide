#![forbid(unsafe_code)]

//! Multi-call command-line interface and session bootstrapper for Roxide
//! (`roxide`, `R`, `Rscript`, `R CMD`).
//!
//! Exposed as a library crate so `roxide-libr` (ADR-0020) can drive the exact same
//! bootstrap and CLI lifecycle from the staged `libR` launcher.

mod cmd;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use roxide_eval::{Ctx, InternalError, Unwind};
use roxide_gc::GcPolicy;
use roxide_host::{Host, NativeHost};

const VERSION_LINE: &str = "Roxide 0.1.0 (targeting GNU R 4.6.2)";

#[derive(Default)]
struct CliOpts {
    show_version: bool,
    exprs: Vec<String>,
    file: Option<String>,
    full_args: Vec<String>,
    trailing_args: Vec<String>,
}

fn parse_cli(raw: Vec<String>) -> Result<CliOpts, String> {
    let mut opts = CliOpts {
        full_args: raw.clone(),
        ..CliOpts::default()
    };
    let mut args = raw.into_iter().skip(1);
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
            "--args" | "--" => {
                opts.trailing_args.extend(args);
                break;
            }
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
                    opts.trailing_args.extend(args);
                } else {
                    opts.trailing_args.push(path.to_owned());
                    opts.trailing_args.extend(args);
                }
                break;
            }
        }
    }
    Ok(opts)
}

fn configured_gc_policy() -> (GcPolicy, GcPolicy) {
    let torture = env::var_os("ROXIDE_GC_TORTURE").is_some();
    let torture_boot = env::var_os("ROXIDE_GC_TORTURE_BOOT").is_some();
    let boot = if torture && torture_boot {
        GcPolicy::Torture
    } else {
        GcPolicy::default()
    };
    let run = if torture {
        GcPolicy::Torture
    } else {
        GcPolicy::default()
    };
    (boot, run)
}

/// Entry point shared by the standalone `roxide` binary and `roxide-libr`'s launcher.
#[must_use]
pub fn run_main() -> ExitCode {
    let code = run_args(env::args().collect());
    ExitCode::from(code.clamp(0, 255) as u8)
}

/// Execute Roxide CLI logic for an explicit argument vector (`argv[0]` included),
/// returning the exact process exit status code (preserving `quit(status = N)`).
#[must_use]
pub fn run_args(full_args: Vec<String>) -> i32 {
    run_args_with(full_args, |_| {})
}

/// Execute Roxide CLI logic with a context setup callback invoked after bootstrap
/// (used by `roxide-libr` to install `roxide-capi` native hooks).
#[must_use]
pub fn run_args_with(full_args: Vec<String>, setup: fn(&mut Ctx)) -> i32 {
    let tail_args: Vec<String> = full_args.iter().skip(1).cloned().collect();
    if tail_args.first().map(String::as_str) == Some("CMD") {
        let (boot_policy, run_policy) = configured_gc_policy();
        let code = cmd::dispatch(&tail_args[1..], &|host| {
            let mut ctx = bootstrap_with(host, boot_policy, setup)?;
            ctx.session_mut().heap.set_policy(run_policy);
            Ok(ctx)
        });
        return if code == ExitCode::SUCCESS { 0 } else { 1 };
    }
    let opts = match parse_cli(full_args) {
        Ok(o) => o,
        Err(err) => {
            let _ = writeln!(io::stderr().lock(), "roxide: {err}");
            return 1;
        }
    };

    if opts.show_version {
        let _ = writeln!(io::stdout().lock(), "{VERSION_LINE}");
        return 0;
    }

    const STACK_BYTES: usize = 32 * 1024 * 1024;
    let run = std::thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(move || interpret(opts, STACK_BYTES, setup));
    match run {
        Ok(thread) => thread.join().unwrap_or(1),
        Err(err) => {
            let _ = writeln!(io::stderr().lock(), "Error: {err}");
            1
        }
    }
}

/// Construct a fully bootstrapped Roxide session with all built-in packages in GNU R 4.6.2 order.
pub fn bootstrap(host: Box<dyn Host>, policy: GcPolicy) -> Result<Ctx, InternalError> {
    bootstrap_with(host, policy, |_| {})
}

/// Construct a fully bootstrapped Roxide session and apply `setup` before running user code.
pub fn bootstrap_with(
    host: Box<dyn Host>,
    policy: GcPolicy,
    setup: fn(&mut Ctx),
) -> Result<Ctx, InternalError> {
    let mut ctx = Ctx::new(host, policy)?;
    roxide_base::register_all(&mut ctx)?;
    roxide_pkg::register(&mut ctx)?;
    roxide_methods::register(&mut ctx)?;
    roxide_utils::datasets::register(&mut ctx)?;
    roxide_utils::register(&mut ctx)?;
    roxide_graphics::register(&mut ctx)?;
    roxide_stats::register(&mut ctx)?;
    setup(&mut ctx);
    Ok(ctx)
}

fn interpret(opts: CliOpts, stack_bytes: usize, setup: fn(&mut Ctx)) -> i32 {
    let (boot_policy, run_policy) = configured_gc_policy();
    let host = Box::new(NativeHost::new());
    let mut ctx = match bootstrap_with(host, boot_policy, setup) {
        Ok(c) => c,
        Err(err) => {
            let _ = writeln!(io::stderr().lock(), "Error: {err}");
            return 1;
        }
    };
    ctx.session_mut().heap.set_policy(run_policy);
    ctx.set_stack_budget(stack_bytes - 2 * 1024 * 1024);
    roxide_pkg::batch::set_command_args(&mut ctx, opts.full_args, opts.trailing_args);

    if !opts.exprs.is_empty() {
        for expr in &opts.exprs {
            if let Err(unwind) = ctx.run_global("<expr>", expr) {
                return handle_unwind(&mut ctx, unwind);
            }
        }
        ctx.run_exit_finalizers();
        return 0;
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
                return 1;
            }
        };
        return match ctx.run_global(&path_str, &src) {
            Ok(()) => {
                ctx.run_exit_finalizers();
                0
            }
            Err(unwind) => handle_unwind(&mut ctx, unwind),
        };
    }

    // Read stdin as a script if no `-e` or file was provided.
    let mut src = String::new();
    if let Err(err) = io::Read::read_to_string(&mut io::stdin().lock(), &mut src) {
        let _ = writeln!(io::stderr().lock(), "Error: {err}");
        return 1;
    }
    match ctx.run_global("<stdin>", &src) {
        Ok(()) => {
            ctx.run_exit_finalizers();
            0
        }
        Err(unwind) => handle_unwind(&mut ctx, unwind),
    }
}

fn handle_unwind(ctx: &mut Ctx, unwind: Unwind) -> i32 {
    match unwind {
        Unwind::Quit {
            status, run_last, ..
        } => {
            if run_last {
                ctx.run_exit_finalizers();
            }
            status.clamp(0, 255)
        }
        Unwind::TopLevel => {
            let _ = writeln!(io::stderr().lock(), "Execution halted");
            1
        }
        Unwind::Value(err) => {
            let _ = writeln!(io::stderr().lock(), "Error: {err}\nExecution halted");
            1
        }
        Unwind::Internal(err) => {
            let _ = writeln!(io::stderr().lock(), "Error: {err}\nExecution halted");
            1
        }
        Unwind::Transfer { .. } => {
            let _ = writeln!(io::stderr().lock(), "Execution halted");
            1
        }
    }
}

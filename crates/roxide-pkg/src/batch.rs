//! Run an R script in batch mode.

use std::io::Write;
use std::path::{Path, PathBuf};

use roxide_eval::{
    Args, Call, Ctx, CtxExt, EvalResult, InternalError, Module, ScriptOpts, Unwind, Visibility,
};
use roxide_gc::{Trace, Tracer};
use roxide_host::{Host, HostErrorKind, NativeHost, OpenMode};
use roxide_value::{BASE_ENV, GLOBAL_ENV, RLogical, SEXP_NIL, ValueApi};

use crate::loader::values::{internal, strings};

type Boot<'a> = &'a dyn Fn(Box<dyn Host>) -> Result<Ctx, InternalError>;

/// Execute `R CMD BATCH [options] infile [outfile]` in a new session.
pub fn run(args: &[String], boot: Boot<'_>) -> i32 {
    run_with_host(args, Box::new(NativeHost::new()), boot)
}

/// Execute BATCH with the supplied host.
pub fn run_with_host(args: &[String], mut host: Box<dyn Host>, boot: Boot<'_>) -> i32 {
    let options = match Options::parse(args) {
        Ok(options) => options,
        Err(error) => {
            let _ = host.write_err(format!("Error: {error}\n").as_bytes());
            return 1;
        }
    };
    let mut output = match host.open(&options.output, OpenMode::write_truncate()) {
        Ok(output) => output,
        Err(error) => {
            let _ = host
                .write_err(format!("Error: {}: {error}\n", options.output.display()).as_bytes());
            return 1;
        }
    };
    let result = boot(host).and_then(|mut ctx| {
        ctx.register_module(&ARGUMENTS)?;
        ctx.ext::<Arguments>().full = options.full.clone();
        ctx.ext::<Arguments>().trailing = options.trailing.clone();
        ctx.capture_streams(true, true, false, |ctx| Ok(execute(ctx, &options)))
            .map_err(|error| InternalError {
                message: format!("{error:?}"),
            })
    });
    let (status, bytes) = match result {
        Ok((status, bytes)) => (status, bytes),
        Err(error) => (
            1,
            format!("Error: {error}\nExecution halted\n").into_bytes(),
        ),
    };
    if output
        .write_all(&bytes)
        .and_then(|()| output.flush())
        .is_err()
    {
        return 1;
    }
    status
}

struct Options {
    input: PathBuf,
    output: PathBuf,
    timing: bool,
    restore: bool,
    save: bool,
    quiet: bool,
    echo: bool,
    trailing: Vec<String>,
    full: Vec<String>,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut timing = true;
        let mut restore = true;
        let mut save = true;
        let mut quiet = false;
        let mut echo = true;
        let mut trailing = Vec::new();
        let mut forwarded = Vec::new();
        let mut has_args = false;
        let mut args = args.iter().peekable();
        while let Some(arg) = args.peek() {
            match arg.as_str() {
                "--" => {
                    args.next();
                    break;
                }
                "--no-timing" => timing = false,
                "--no-save" => save = false,
                "--save" => save = true,
                "--no-restore" | "--no-restore-data" => restore = false,
                "--restore" => restore = true,
                "--vanilla" => {
                    save = false;
                    restore = false;
                }
                "--slave" | "--no-echo" | "-s" => {
                    quiet = true;
                    echo = false;
                }
                "--quiet" | "--silent" | "-q" => quiet = true,
                "--no-readline"
                | "--no-site-file"
                | "--no-init-file"
                | "--no-environ"
                | "--no-restore-history" => {}
                "--args" => {
                    has_args = true;
                    args.next();
                    while args
                        .peek()
                        .is_some_and(|arg| arg.starts_with('-') && *arg != "--")
                    {
                        trailing.extend(args.next().cloned());
                    }
                    continue;
                }
                other if other.starts_with('-') => {
                    return Err(format!("unknown BATCH option '{other}'"));
                }
                _ => break,
            }
            if arg.as_str() != "--no-timing" {
                forwarded.push((*arg).clone());
            }
            args.next();
        }
        let input = args.next().map(PathBuf::from).ok_or("no input file")?;
        let output = match args.peek() {
            Some(flag) if flag.as_str() == "--args" => None,
            _ => args.next().map(PathBuf::from),
        };
        let output = match output {
            Some(output) => output,
            None => {
                let name = input
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("invalid input file name")?;
                PathBuf::from(format!(
                    "{}.Rout",
                    name.strip_suffix(".R")
                        .filter(|stem| !stem.is_empty())
                        .unwrap_or(name)
                ))
            }
        };
        if let Some(arg) = args.next() {
            if arg != "--args" {
                return Err(format!("unexpected BATCH argument '{arg}'"));
            }
            has_args = true;
            trailing.extend(args.cloned());
        }
        let mut full = vec![
            "R".to_owned(),
            "-f".to_owned(),
            input.to_string_lossy().into_owned(),
            "--restore".to_owned(),
            "--save".to_owned(),
            "--no-readline".to_owned(),
        ];
        full.extend(forwarded);
        if has_args {
            full.push("--args".to_owned());
            full.extend(trailing.iter().cloned());
        }
        Ok(Self {
            input,
            output,
            timing,
            restore,
            save,
            quiet,
            echo,
            trailing,
            full,
        })
    }
}

fn execute(ctx: &mut Ctx, options: &Options) -> i32 {
    let result = script(ctx, options);
    match result {
        Ok(()) => 0,
        Err(Unwind::Quit { status, .. }) => status,
        Err(error) => {
            let detail = match error {
                Unwind::Internal(error) => Some(error.to_string()),
                Unwind::Value(error) => Some(error.to_string()),
                _ => None,
            };
            if let Some(detail) = detail {
                let _ = ctx
                    .host()
                    .write_err(format!("Error: {detail}\n").as_bytes());
            }
            let _ = ctx.host().write_err(b"Execution halted\n");
            1
        }
    }
}

fn script(ctx: &mut Ctx, options: &Options) -> EvalResult<()> {
    let code = crate::description::read_text(ctx, &options.input)?;
    if !options.quiet {
        let _ = ctx
            .host()
            .write_out(b"Roxide 0.1.0 (targeting GNU R 4.6.2)\n\n");
    }
    if options.restore {
        match ctx.host().metadata(Path::new(".RData"), true) {
            Ok(_) => {
                ctx.run_script(
                    "<restore>",
                    "load('.RData', envir = .GlobalEnv)",
                    BASE_ENV,
                    false,
                )?;
                if !options.quiet {
                    let _ = ctx
                        .host()
                        .write_out(b"[Previously saved workspace restored]\n\n");
                }
            }
            Err(error) if error.kind == HostErrorKind::NotFound => {}
            Err(error) => return Err(ctx.stop(SEXP_NIL, error.to_string())),
        }
    }
    ctx.run_script_with_opts(
        &options.input.to_string_lossy(),
        code.trim_start_matches('\u{feff}'),
        GLOBAL_ENV,
        ScriptOpts {
            auto_print: true,
            echo: options.echo,
        },
    )?;
    if options.echo {
        let _ = ctx.host().write_out(b"> \n");
    }
    if options.timing {
        let time = ctx.host().cpu_times();
        let footer = format!(
            "> proc.time()\n   user  system elapsed \n{:7.3} {:7.3} {:7.3} \n",
            time.user_secs, time.system_secs, time.elapsed_secs
        );
        let _ = ctx.host().write_out(footer.as_bytes());
    }
    if options.save {
        ctx.run_script(
            "<save>",
            "save(list = ls(envir = .GlobalEnv, all.names = TRUE), file = '.RData', envir = .GlobalEnv)",
            BASE_ENV,
            false,
        )?;
    }
    Ok(())
}

#[derive(Default)]
struct Arguments {
    full: Vec<String>,
    trailing: Vec<String>,
}

impl Trace for Arguments {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for Arguments {}

const ARGUMENTS: Module = Module {
    prims: &[internal("rox_batch_args", 1, command_args, Visibility::On)],
    prelude: "commandArgs <- function(trailingOnly = FALSE) .Internal(rox_batch_args(trailingOnly))",
};

fn command_args(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let trailing = ctx.session().lgl(args.get(0))?;
    let trailing = match (trailing.len() == 1).then(|| trailing.get(0)) {
        Some(RLogical::TRUE) => true,
        Some(RLogical::FALSE) => false,
        _ => return Err(ctx.stop(SEXP_NIL, "invalid 'trailingOnly' argument")),
    };
    let args = ctx.ext::<Arguments>();
    let values = if trailing { &args.trailing } else { &args.full }.clone();
    strings(ctx, values.iter().map(String::as_str))
}

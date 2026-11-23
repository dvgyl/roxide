#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Command-line runner for Roxide conformance cases.

use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use roxide_harness::parser::load_cases;
use roxide_harness::runner::{compare, run_case};

const HELP: &str = "\
Usage: roxide-harness [OPTIONS]

  --suite <name>         Select all, layer1, smoke, or a tag (default: all).
  --file <path>          Use one .rcase file. Tag filters still apply.
  --case <substring>     Select case names that contain the substring.
  --roxide <path>        Set the Roxide executable (default: ROXIDE_BIN or target/debug/roxide).
  --oracle <path>        Set the GNU R Rscript executable (default: ROXIDE_ORACLE_RSCRIPT).
  --oracle-only          Check the oracle exit code against expect_error.
  --ignore-stderr-text   Compare the exit status and stdout only.
  --timeout-ms <ms>      Set the timeout for each process (default: 10000).
  --fail-fast            Stop after the first failure.
  -h, --help             Show this help.
";

#[derive(Default)]
struct Options {
    suite: Option<String>,
    file: Option<PathBuf>,
    case_filter: Option<String>,
    roxide: Option<PathBuf>,
    oracle: Option<PathBuf>,
    oracle_only: bool,
    ignore_stderr_text: bool,
    timeout: Option<Duration>,
    fail_fast: bool,
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        let mut value = || {
            args.next()
                .filter(|value| !value.as_encoded_bytes().starts_with(b"--"))
                .ok_or_else(|| format!("missing value for {}", argument.to_string_lossy()))
        };
        let mut text = || {
            value()?
                .into_string()
                .map_err(|_| format!("{} requires a UTF-8 value", argument.to_string_lossy()))
        };
        match argument.to_str() {
            Some("--suite") => options.suite = Some(text()?),
            Some("--file") => options.file = Some(value()?.into()),
            Some("--case") => options.case_filter = Some(text()?),
            Some("--roxide") => options.roxide = Some(value()?.into()),
            Some("--oracle") => options.oracle = Some(value()?.into()),
            Some("--oracle-only") => options.oracle_only = true,
            Some("--ignore-stderr-text") => options.ignore_stderr_text = true,
            Some("--timeout-ms") => {
                let milliseconds = text()?
                    .parse::<u64>()
                    .map_err(|_| "--timeout-ms requires a nonnegative integer".to_owned())?;
                options.timeout = Some(Duration::from_millis(milliseconds));
            }
            Some("--fail-fast") => options.fail_fast = true,
            Some("--help" | "-h") => return Ok(None),
            _ => return Err(format!("unknown argument '{}'", argument.to_string_lossy())),
        }
    }
    Ok(Some(options))
}

fn run(options: Options, stdout: &mut impl Write) -> Result<ExitCode, String> {
    let suite = options.suite.as_deref().unwrap_or("all");
    let root = if suite == "layer1" {
        Path::new("conformance/layer1")
    } else {
        Path::new("conformance")
    };
    let tag = match suite {
        "all" | "layer1" => None,
        tag => Some(tag),
    };
    if let Some(file) = &options.file
        && file.is_dir()
    {
        return Err(format!("{}: --file requires a .rcase file", file.display()));
    }
    let mut cases = load_cases(options.file.as_deref().unwrap_or(root))?;
    cases.retain(|case| {
        tag.is_none_or(|tag| case.tags.iter().any(|case_tag| case_tag == tag))
            && options
                .case_filter
                .as_ref()
                .is_none_or(|filter| case.name.contains(filter))
    });
    if cases.is_empty() {
        writeln!(stdout, "PASS: 0, FAIL: 0, TOTAL: 0").map_err(|error| error.to_string())?;
        return Ok(ExitCode::FAILURE);
    }

    let oracle = options
        .oracle
        .or_else(|| env::var_os("ROXIDE_ORACLE_RSCRIPT").map(PathBuf::from))
        .ok_or("set ROXIDE_ORACLE_RSCRIPT or specify --oracle <path>")?;
    let roxide = if options.oracle_only {
        None
    } else {
        Some(
            options
                .roxide
                .or_else(|| env::var_os("ROXIDE_BIN").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("target/debug/roxide")),
        )
    };
    let timeout = options.timeout.unwrap_or(Duration::from_secs(10));
    let mut passed = 0;
    let mut failed = 0;

    for case in cases {
        let differences = match run_case(&case.script, &oracle, roxide.as_deref(), timeout) {
            Ok(output) => match output.roxide {
                Some(roxide) => compare(&output.oracle, &roxide, options.ignore_stderr_text),
                None => match output.oracle.status.code() {
                    Some(code) if (code != 0) == case.expect_error => Vec::new(),
                    _ => {
                        let expected = if case.expect_error { "nonzero" } else { "zero" };
                        vec![format!(
                            "expected a {expected} exit code from the oracle, got {}",
                            output.oracle.status
                        )]
                    }
                },
            },
            Err(error) => vec![error],
        };
        if differences.is_empty() {
            passed += 1;
        } else {
            failed += 1;
            writeln!(
                stdout,
                "FAIL {} ({}:{})",
                case.name,
                case.path.display(),
                case.line
            )
            .map_err(|error| error.to_string())?;
            for difference in differences {
                writeln!(stdout, "{difference}").map_err(|error| error.to_string())?;
            }
            if options.fail_fast {
                break;
            }
        }
    }

    let total = passed + failed;
    writeln!(stdout, "PASS: {passed}, FAIL: {failed}, TOTAL: {total}")
        .map_err(|error| error.to_string())?;
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn main() -> ExitCode {
    let mut stdout = io::stdout().lock();
    let result = match parse_args() {
        Ok(Some(options)) => run(options, &mut stdout),
        Ok(None) => stdout
            .write_all(HELP.as_bytes())
            .map(|()| ExitCode::SUCCESS)
            .map_err(|error| error.to_string()),
        Err(error) => Err(error),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "roxide-harness: {error}");
            ExitCode::FAILURE
        }
    }
}

#![forbid(unsafe_code)]

//! Tests for CLI evaluation, conditions, and replacement.

use std::process::{Command, Output};

fn run(source: &str, torture: bool) -> std::io::Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_roxide"));
    command.args(["-e", source]);
    if torture {
        command.env("ROXIDE_GC_TORTURE", "1");
    } else {
        command.env_remove("ROXIDE_GC_TORTURE");
    }
    command.output()
}

#[test]
fn deep_recursion_completes_on_the_cli_stack() -> std::io::Result<()> {
    let output = run(
        "f <- function(n) if (n == 0) 0 else 1 + f(n - 1); f(1000)",
        false,
    )?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"[1] 1000\n");
    Ok(())
}

#[test]
fn expression_overflow_runs_calling_handlers() -> std::io::Result<()> {
    for torture in [false, true] {
        let output = run(
            r#"
options(expressions = 50)
f <- function(n) f(n)
withCallingHandlers(f(1), error = function(e) cat(paste(class(e), collapse=","), "\n"))
"#,
            torture,
        )?;
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            output.stdout,
            b"expressionStackOverflowError,stackOverflowError,error,condition \n"
        );
        assert_eq!(
            output.stderr,
            b"Error: evaluation nested too deeply: infinite recursion / options(expressions=)?\nExecution halted\n"
        );
    }
    Ok(())
}

#[test]
fn replacements_keep_aliases_and_direct_primitive_inputs_unchanged() -> std::io::Result<()> {
    for torture in [false, true] {
        let output = run(
            r#"
y <- x <- list(c(1, 2))
x[[1]][2] <- 99
dput(y[[1]][2])
dput(x[[1]][2])
z <- c(1, 2)
dput(`[<-`(z, 1, 99))
dput(z)
z <- list(1, 2)
z[[1]] <- z
dput(z)
x <- y <- c(1, 2)
z <- c(3, 4)
x[{ z[1] <- 99; 1:2 }] <- z
dput(x)
dput(z)
"#,
            torture,
        )?;
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            output.stdout,
            b"2\n99\nc(99, 2)\nc(1, 2)\nlist(list(1, 2), 2)\nc(3, 4)\nc(99, 4)\n"
        );
    }
    Ok(())
}

#[test]
fn base_registration_preserves_optional_arguments_and_implicit_classes() -> std::io::Result<()> {
    let output = run(
        r#"
make <- function() function(x) x
x <- make()
y <- make()
dput(identical(x, y, ignore.environment = TRUE))
dput(identical(x, y, ignore.environment = FALSE))
dput(class(cumsum(structure(1:3, class = "foo"))))
dput(inherits(structure(1:3, class = "foo") + 1, "foo"))
dput(class(quote(if (a) b)))
dput(class(quote(f(x))))
dput(class(parse(text = "a = 1")[[1]]))
"#,
        true,
    )?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout,
        b"TRUE\nFALSE\n\"integer\"\nTRUE\n\"if\"\n\"call\"\n\"=\"\n"
    );
    Ok(())
}

#[test]
fn pipe_error_reports_position_in_the_current_expression() -> std::io::Result<()> {
    let output = run(
        "f <- function(x, y) x + y; x <- 8; print(x |> f(_, 2))",
        false,
    )?;
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stderr,
        b"Error in f(\"_\", 2) : \n  pipe placeholder can only be used as a named argument (<input>:1:13)\nExecution halted\n"
    );
    Ok(())
}

#![forbid(unsafe_code)]

//! Verify the compiler package through its R interfaces.

use std::env;
use std::process::Command;

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{HostResult, PureHost, forward_host};
use roxide_value::GLOBAL_ENV;

struct TestHost {
    inner: PureHost,
}

forward_host!(TestHost, inner;
    fn write_err(&mut self, bytes: &[u8]) -> HostResult<()> {
        eprint!("{}", String::from_utf8_lossy(bytes));
        Ok(())
    }
);

fn run(host: PureHost, script: &str) {
    let mut ctx =
        Ctx::new(Box::new(TestHost { inner: host }), GcPolicy::Torture).expect("Create R session");
    roxide_base::register_all(&mut ctx).expect("Register base functions");
    ctx.run_script("compiler parity", script, GLOBAL_ENV, false)
        .unwrap_or_else(|error| panic!("{error:?}\n{script}"));
}

#[test]
fn jit_levels_return_the_previous_level() {
    run(
        PureHost::new(),
        r#"
        previous <- compiler::enableJIT(-1L)
        stopifnot(identical(compiler::enableJIT(0L), previous))
        for (n in 1:3) {
            stopifnot(identical(compiler::enableJIT(n), n - 1L),
                      identical(compiler::enableJIT(-1L), n))
        }
        for (bad in list(-2L, 4L, NA_integer_, integer(), 1:2)) {
            stopifnot(identical(tryCatch(compiler::enableJIT(bad), error = conditionMessage),
                                "invalid JIT level"),
                      identical(compiler::enableJIT(-1L), 3L))
        }
        "#,
    );
}

#[test]
fn explicit_compilation_preserves_closure_parts_and_runs_with_jit_off() {
    run(
        PureHost::new(),
        r#"
        compiler::enableJIT(0L)
        f <- local({ offset <- 20L; function(x = 2L) x + offset })
        attr(f, "note") <- "test"
        seen <- FALSE
        side_effect <- function() { seen <<- TRUE; 41L }
        g <- compiler::cmpfun(f)
        h <- compiler::cmpfun(side_effect)
        stopifnot(!seen, identical(h(), 41L), seen, identical(g(), 22L),
                  identical(g(3L), 23L), identical(formals(g), formals(f)),
                  identical(body(g), body(f)), identical(environment(g), environment(f)),
                  identical(attributes(g), attributes(f)),
                  identical(compiler::cmpfun(sum), sum),
                  identical(tryCatch(compiler::cmpfun(1), error = conditionMessage),
                            "cannot compile a non-function"))
        out <- textConnection("printed", "w")
        sink(out)
        print(f)
        print(g)
        print(f)
        sink()
        close(out)
        bytecode <- which(grepl("^<bytecode: 0x[0-9a-f]+>$", printed))
        stopifnot(length(bytecode) == 1L, identical(printed[bytecode - 1L], "x + offset"),
                  grepl("^<environment: 0x[0-9a-f]+>$", printed[bytecode + 1L]))
        "#,
    );
}

#[test]
fn automatic_compilation_updates_the_closure_display() {
    run(
        PureHost::new(),
        r#"
        bytecode <- function(f) {
            out <- textConnection("printed", "w", local = TRUE)
            sink(out)
            print(f)
            sink()
            close(out)
            any(grepl("^<bytecode: 0x[0-9a-f]+>$", printed))
        }
        for (level in c(0L, 3L)) {
            compiler::enableJIT(level)
            f <- function(x) x + 1L
            g <- function(n) { total <- 0L; for (i in seq_len(n)) total <- total + i; total }
            stopifnot(!bytecode(f), !bytecode(g))
            stopifnot(identical(f(1L), 2L), identical(g(3L), 6L))
            stopifnot(!bytecode(f), bytecode(g) == (level != 0L))
            stopifnot(identical(f(2L), 3L), bytecode(f) == (level != 0L))
        }
        "#,
    );
}

#[test]
fn compiler_options_and_package_flag_keep_session_state() {
    run(
        PureHost::new(),
        r#"
        compiler::enableJIT(0L)
        get <- compiler::getCompilerOption
        set <- compiler::setCompilerOptions
        stopifnot(identical(get("optimize"), 2L), identical(get("suppressAll"), TRUE),
                  identical(get("suppressUndefined"), c(".Generic", ".Method", ".Random.seed", ".self")),
                  identical(get("suppressNoSuperAssignVar"), FALSE),
                  identical(get("optimize", list(optimize = 0L)), 0L),
                  identical(get("optimize", list(optimize = NULL)), NULL))
        update <- withVisible(set(optimize = 1L, suppressAll = FALSE,
                                  suppressUndefined = "foo", suppressNoSuperAssignVar = TRUE))
        stopifnot(!update$visible,
                  identical(names(update$value),
                            c("optimize", "suppressAll", "suppressUndefined", "suppressNoSuperAssignVar")),
                  identical(update$value$optimize, 2L),
                  identical(update$value$suppressAll, TRUE),
                  identical(update$value$suppressNoSuperAssignVar, FALSE),
                  identical(get("optimize"), 1L), identical(get("suppressUndefined"), "foo"),
                  !get("suppressAll"), get("suppressNoSuperAssignVar"),
                  identical(set(), list()), identical(set(optimize = 7L), list()),
                  identical(get("optimize"), 1L),
                  identical(tryCatch(set(unknown = TRUE), error = conditionMessage),
                            "'unknown' is not a valid compiler option"),
                  identical(compiler::compilePKGS(TRUE), FALSE),
                  identical(compiler::compilePKGS(FALSE), TRUE))
        "#,
    );
}

#[test]
fn compiled_expressions_use_the_evaluation_environment() {
    run(
        PureHost::new(),
        r#"
        compiler::enableJIT(0L)
        compiler_env <- new.env()
        compiler_env$x <- 100L
        e <- compiler::compile(quote({ x <- x + 1L; invisible(x) }), compiler_env)
        stopifnot(identical(typeof(e), "bytecode"), identical(compiler_env$x, 100L))
        target <- new.env()
        target$x <- 41L
        result <- withVisible(eval(e, target))
        stopifnot(identical(result$value, 42L), !result$visible,
                  identical(target$x, 42L), identical(eval(e, target), 43L))
        f <- compiler::compile(quote(x + 1L))
        expressions <- parse(text = c("1", "2"), keep.source = FALSE)
        stopifnot(identical(eval(f, list(x = 2L)), 3L),
                  identical(eval(compiler::compile(expressions)), expressions),
                  identical(eval(compiler::compile(NULL), emptyenv()), NULL),
                  grepl("Roxide IR", compiler::disassemble(f), fixed = TRUE),
                  identical(tryCatch(compiler::disassemble(identity), error = conditionMessage),
                            "function is not compiled"))
        "#,
    );
}

#[test]
fn compiled_files_round_trip_in_both_formats() {
    let mut host = PureHost::new();
    host.insert_file(
        "/src/input.r",
        r#"x <- 40L; x <- x + 2L; wd <- getwd(); s <- "é \\n"; real <- 1.25; empty <- """#
            .as_bytes(),
    );
    host.insert_file("/src/empty.R", b"");
    host.insert_file("/src/error.R", b"stop('test error')");
    host.insert_file("/src/invalid.Rc", b"invalid");
    run(
        host,
        r#"
        envir <- new.env()
        wd <- getwd()
        stopifnot(is.null(compiler::cmpfile("/src/input.r")), file.exists("/src/input.Rc"))
        stopifnot(is.null(compiler::loadcmp("/src/input.Rc", envir, chdir = TRUE)),
                  identical(getwd(), wd), identical(envir$wd, "/src"), identical(envir$x, 42L))
        stopifnot(is.null(compiler::cmpfile("/src/input.r", "/ascii.Rc", ascii = TRUE, verbose = TRUE)),
                  identical(readLines("/ascii.Rc", n = 1L), "RDA3"))
        ascii_env <- new.env()
        compiler::loadcmp("/ascii.Rc", ascii_env)
        stopifnot(identical(ascii_env$x, envir$x), identical(ascii_env$s, envir$s),
                  identical(ascii_env$real, 1.25), identical(ascii_env$empty, ""))
        empty_warning <- tryCatch(compiler::cmpfile("/src/empty.R"), warning = conditionMessage)
        stopifnot(identical(empty_warning, "empty input file; no output written"),
                  !file.exists("/src/empty.Rc"))
        compiler::cmpfile("/src/error.R")
        stopifnot(identical(tryCatch(compiler::loadcmp("/src/error.Rc", envir, chdir = TRUE),
                                     error = conditionMessage), "test error"),
                  identical(getwd(), wd),
                  inherits(try(compiler::loadcmp("/src/invalid.Rc"), silent = TRUE), "try-error"))
        "#,
    );
}

#[test]
fn compiler_namespace_attaches_through_the_cli_loader() {
    let Some(cli) = env::var_os("ROXIDE_COMPILER_TEST_CLI") else {
        return;
    };
    let output = Command::new(cli)
        .args([
            "--vanilla",
            "-e",
            r#"
            stopifnot(requireNamespace("compiler", quietly = TRUE),
                      !("package:compiler" %in% search()),
                      identical(sort(getNamespaceExports("compiler")),
                                c("cmpfile", "cmpfun", "compile", "compilePKGS", "disassemble",
                                  "enableJIT", "getCompilerOption", "loadcmp", "setCompilerOptions")))
            library(compiler)
            stopifnot("package:compiler" %in% search(), identical(cmpfun, compiler::cmpfun),
                      identical(cmpfun(function(x) x + 1L)(1L), 2L))
            cmpfun(function(x) x + 1L)
            "#,
        ])
        .output()
        .expect("Run Roxide");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).expect("Decode Roxide output");
    assert!(
        stdout.lines().any(|line| line.starts_with("<bytecode: 0x")),
        "{stdout}"
    );
}

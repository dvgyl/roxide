set_testing <- function() {
  before <- Sys.getenv("TESTTHAT", unset = NA_character_)
  Sys.setenv(TESTTHAT = "true")
  before
}

restore_testing <- function(before) {
  if (identical(before, NA_character_)) Sys.unsetenv("TESTTHAT") else
    Sys.setenv(TESTTHAT = before)
}

test_that <- function(desc, code) {
  reporter <- getOption("testthat.reporter")
  standalone <- is.null(reporter)
  if (standalone) {
    reporter <- new_reporter()
    before <- set_testing()
    on.exit(restore_testing(before), add = TRUE)
  }
  file <- getOption("testthat.file", "<interactive>")
  expr <- substitute(code)
  env <- new.env(parent = parent.frame())

  if (edition_get() >= 3L) local_reproducible_output()
  tryCatch(withCallingHandlers(
    eval(expr, env),
    expectation = function(e) {
      kind <- if (inherits(e, "expectation_success")) "pass" else "fail"
      report_result(reporter, kind, file, desc, conditionMessage(e))
      invokeRestart("continue_test")
    },
    warning = function(e) {
      report_result(reporter, "warn", file, desc)
      muffle_condition(e)
    },
    message = muffle_condition
  ), skip = function(e) {
    report_result(reporter, "skip", file, desc)
  }, error = function(e) {
    report_result(reporter, "fail", file, desc, conditionMessage(e))
  })

  if (standalone) finish_report(reporter, TRUE)
  invisible(NULL)
}

eval_test_file <- function(path, env, reporter) {
  file <- basename(path)
  old <- options(testthat.file = file)
  on.exit(options(old), add = TRUE)
  tryCatch(withCallingHandlers(
    for (expr in parse(file = path, keep.source = FALSE)) eval(expr, env),
    expectation = function(e) {
      kind <- if (inherits(e, "expectation_success")) "pass" else "fail"
      report_result(reporter, kind, file, "Top-level expectation", conditionMessage(e))
      invokeRestart("continue_test")
    },
    warning = function(e) {
      report_result(reporter, "warn", file, "File setup")
      muffle_condition(e)
    },
    message = muffle_condition
  ), skip = function(e) {
    report_result(reporter, "skip", file, "File setup")
  }, error = function(e) {
    report_result(reporter, "fail", file, "File evaluation", conditionMessage(e))
  })
  invisible(NULL)
}

run_test_files <- function(path, files, package, env, stop_on_failure, load_helpers = TRUE) {
  reporter <- new_reporter()
  before <- set_testing()
  on.exit(restore_testing(before), add = TRUE)

  # Use the test directory for paths used directly by package tests.
  old_dir <- getwd()
  setwd(path)
  on.exit(setwd(old_dir), add = TRUE)

  if (!is.null(package)) library(package, character.only = TRUE)
  if (is.null(env)) env <- if (is.null(package)) parent.frame() else asNamespace(package)
  shared <- new.env(parent = env)
  old <- options(testthat.reporter = reporter, testthat.path = getwd(),
                 testthat.package = package, testthat.edition = edition_get())
  on.exit(options(old), add = TRUE)

  if (load_helpers) {
    for (file in sort(list.files(".", "^helper.*\\.[rR]$"), method = "radix")) {
      eval_test_file(file, shared, reporter)
    }
  }
  for (file in sort(list.files(".", "^setup.*\\.[rR]$"), method = "radix")) {
    eval_test_file(file, shared, reporter)
  }
  for (file in files) {
    eval_test_file(file, new.env(parent = shared), reporter)
  }
  finish_report(reporter, stop_on_failure)
}

test_file <- function(path, env = parent.frame(), ..., stop_on_failure = TRUE) {
  run_test_files(dirname(path), basename(path), NULL, env, stop_on_failure)
}

test_dir <- function(path, package = NULL, stop_on_failure = TRUE, ..., env = NULL,
                     load_helpers = TRUE) {
  files <- sort(list.files(path, "^test.*\\.[rR]$"), method = "radix")
  if (!length(files)) stop("No test files found.")
  if (is.null(env) && is.null(package)) env <- parent.frame()
  run_test_files(path, files, package, env, stop_on_failure, load_helpers)
}

test_check <- function(package, ...) {
  path <- if (dir.exists("testthat")) "testthat" else "tests/testthat"
  test_dir(path, package = package, ...)
}

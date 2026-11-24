set_testing <- function() {
  before <- Sys.getenv("TESTTHAT", unset = NA_character_)
  Sys.setenv(TESTTHAT = "true")
  before
}

restore_testing <- function(before) {
  if (identical(before, NA_character_)) Sys.unsetenv("TESTTHAT") else
    Sys.setenv(TESTTHAT = before)
}

run_test_block <- function(desc, expr, parent, group = FALSE) {
  reporter <- getOption("testthat.reporter")
  standalone <- is.null(reporter)
  if (standalone) {
    reporter <- new_reporter()
    before <- set_testing()
    on.exit(restore_testing(before), add = TRUE)
    old <- options(testthat.reporter = reporter)
    on.exit(options(old), add = TRUE)
  }
  file <- getOption("testthat.file", "<interactive>")
  prefix <- getOption("testthat.description")
  if (!is.null(prefix)) desc <- paste(prefix, desc, sep = " / ")
  if (group) {
    old_group <- options(testthat.description = desc)
    on.exit(options(old_group), add = TRUE)
  } else if (edition_get() >= 3L) {
    local_reproducible_output()
  }

  env <- new.env(parent = parent)
  report_test_code(eval(expr, env), reporter, file, desc)
  if (standalone) finish_report(reporter, TRUE)
  invisible(NULL)
}

test_that <- function(desc, code) {
  run_test_block(desc, substitute(code), parent.frame())
}

describe <- function(description, code) {
  run_test_block(description, substitute(code), parent.frame(), group = TRUE)
}

it <- function(description, code = NULL) {
  expr <- substitute(code)
  if (is.null(expr)) expr <- quote(testthat::skip("Empty test."))
  run_test_block(description, expr, parent.frame())
}

report_test_code <- function(code, reporter, file, desc, error_desc = desc, skip_desc = desc) {
  tryCatch(withCallingHandlers(
    code,
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
    report_result(reporter, "skip", file, skip_desc)
  }, error = function(e) {
    kind <- if (is_missing_optional_package(e)) "skip" else "fail"
    report_result(reporter, kind, file, error_desc, conditionMessage(e))
  })
  invisible(NULL)
}

eval_test_file <- function(path, env, reporter) {
  file <- basename(path)
  actions <- list()
  register <- function(action) actions[[length(actions) + 1L]] <<- action
  old <- options(testthat.file = file, testthat.register_teardown = register)
  on.exit(options(old), add = TRUE)
  report_test_code(
    for (expr in parse(file = path, keep.source = FALSE)) eval(expr, env),
    reporter, file, "Top-level expectation",
    error_desc = "File evaluation", skip_desc = "File setup"
  )
  for (action in actions) report_test_code(action(), reporter, file, "Teardown")
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
                 testthat.package = package, testthat.edition = edition_get(),
                 testthat.description = NULL)
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

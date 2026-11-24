restore_options <- function(old, frame) {
  action <- substitute(options(OLD), list(OLD = old))
  do.call(on.exit, list(action, add = TRUE, after = FALSE), envir = frame)
}

local_edition <- function(edition, .env = parent.frame()) {
  restore_options(options(testthat.edition = edition), .env)
  invisible(edition)
}

edition_get <- function() getOption("testthat.edition", 2L)
is_testing <- function() identical(Sys.getenv("TESTTHAT"), "true")
testing_package <- function() getOption("testthat.package")
test_path <- function(...) file.path(getOption("testthat.path", "tests/testthat"), ...)

local_reproducible_output <- function(width = 80L, .env = parent.frame(), ...) {
  old <- options(width = width, crayon.enabled = FALSE, cli.num_colors = 1L,
                 useFancyQuotes = FALSE, OutDec = ".")
  restore_options(old, .env)
  invisible(NULL)
}

capture_output <- function(code, print = FALSE, width = 80L) {
  old <- options(width = width)
  on.exit(options(old), add = TRUE)
  expr <- substitute(code)
  env <- parent.frame()
  if (print) {
    lines <- capture.output(eval(expr, env))
  } else {
    lines <- capture.output(value <- eval(expr, env))
  }
  paste(lines, collapse = "\n")
}

capture_condition_text <- function(code, kind) {
  text <- c()
  withCallingHandlers(code, condition = function(cond) {
    if (inherits(cond, kind)) {
      text <<- c(text, conditionMessage(cond))
      muffle_condition(cond)
    }
  })
  if (is.null(text)) character(0L) else text
}

capture_messages <- function(code) capture_condition_text(code, "message")
capture_warnings <- function(code) capture_condition_text(code, "warning")
evaluate_promise <- function(code) invisible(code)

setup <- function(code, env = new.env(parent = parent.frame())) {
  invisible(eval(substitute(code), env))
}

teardown <- function(code, env = parent.frame()) {
  register <- getOption("testthat.register_teardown")
  if (is.null(register)) stop("Call teardown() during a test file.")
  expr <- substitute(code)
  force(env)
  register(function() eval(expr, env))
  invisible(NULL)
}

expect_snapshot <- function(...) skip("snapshot tests not supported by minimal shim")
expect_snapshot_output <- expect_snapshot
expect_snapshot_value <- expect_snapshot
expect_snapshot_file <- expect_snapshot
expect_snapshot_error <- expect_snapshot
expect_snapshot_warning <- expect_snapshot
expect_snapshot_message <- expect_snapshot
announce_snapshot_file <- expect_snapshot

library(testthat)

test_that("checks values and labels", {
  expect(TRUE, "Unused")
  succeed()
  expect_equal(1, 1 + 1e-10)
  expect_identical(1L, 1L)
  expect_equivalent(c(a = 1), c(b = 1))
  expect_setequal(c(NA, 2, 2, 1), c(1, 2, NA))
  expect_mapequal(list(a = 1, b = 2), list(b = 2, a = 1))
  expect_true(TRUE)
  expect_false(c(a = FALSE))
  expect_null(NULL)
  expect_length(1:3, 3)
  expect_named(c(a = 1))
  expect_named(1, NULL)
  expect_named(c(B = 1, a = 2), c("A", "b"), ignore.order = TRUE, ignore.case = TRUE)
  expect_gt(2, 1)
  expect_gte(2, 2)
  expect_lt(1, 2)
  expect_lte(2, 2)
  expect_type(sum, "builtin")
  obj <- structure(1L, class = c("second", "first"))
  expect_s3_class(obj, "first")
  expect_s3_class(obj, c("second", "first"), exact = TRUE)
  expect_is(obj, "first")
  s4 <- methods::new("classRepresentation")
  expect_s4_class(s4, "classRepresentation")
  expect_match(c("ab", "ac"), "^a")
  expect_match(c("b", "ac"), "^a", all = FALSE)
  expect_no_match(c("ab", "ac"), "z")
})

test_that("checks conditions and captures their values", {
  problem <- structure(list(message = "an error", call = NULL),
                       class = c("probe_error", "error", "condition"))
  expect_identical(expect_error(stop(problem), "an error", class = "probe_error"), problem)
  expect_identical(expect_error(2L, NA), 2L)
  warnings <- capture_warnings({
    caught <- expect_warning({
      warning("first")
      warning("wanted")
      warning("last")
    }, "wanted")
  })
  expect_identical(warnings, c("first", "last"))
  expect_identical(conditionMessage(caught), "wanted")
  messages <- capture_messages({
    expect_message({
      message("first")
      message("wanted")
      message("last")
    }, "wanted", fixed = TRUE)
  })
  expect_identical(messages, c("first\n", "last\n"))
  caught <- expect_condition(signalCondition(structure(
    list(message = "probe", call = NULL), class = c("probe", "condition")
  )), "probe", class = "probe")
  expect_identical(conditionMessage(caught), "probe")
  expect_no_error(1)
  expect_no_warning(2)
  expect_no_message(3)
  expect_silent(4)
  expect_identical(expect_output({ cat("a\nb\n"); 5L }, "a\nb", fixed = TRUE), 5L)
  expect_output(invisible(4), NA)
  expect_identical(expect_invisible(invisible(9L)), 9L)
  expect_identical(expect_visible(11L), 11L)
})

test_that("runs legacy expectations", {
  context("legacy")
  expect_that(1, equals(1 + 1e-10))
  expect_that(1L, is_identical_to(1L))
  expect_that(c(a = 1), is_equivalent_to(1))
  expect_that(1L, is_a("integer"))
  expect_that(TRUE, is_true())
  expect_that(FALSE, is_false())
  expect_that(NULL, is_null())
  expect_that("text", matches("text"))
  expect_that("screen", prints_text("screen"))
  expect_that(stop("error"), throws_error("error"))
  expect_that(warning("warning"), gives_warning("warning"))
  expect_that(message("message"), shows_message("message"))
})

test_that("restores local options and captures output", {
  start <- getOption("width")
  original_edition <- edition_get()
  check <- function() {
    local_edition(3L)
    local_reproducible_output(61L)
    local_reproducible_output(67L)
    expect_identical(edition_get(), 3L)
    expect_identical(getOption("width"), 67L)
    expect_false(getOption("crayon.enabled"))
    expect_identical(getOption("cli.num_colors"), 1L)
    expect_false(getOption("useFancyQuotes"))
    expect_identical(getOption("OutDec"), ".")
  }
  check()
  expect_identical(getOption("width"), start)
  expect_identical(edition_get(), original_edition)
  expect_identical(capture_output({ cat("printed\n"); 3 }), "printed")
  expect_identical(capture_output(3, print = TRUE), "[1] 3")
  expect_identical(capture_output(invisible(3), print = TRUE), "")
  expect_identical(evaluate_promise(12L), 12L)
  expect_true(is_testing())
  expect_null(testing_package())
})

test_that("loads helpers and setup before sorted tests and reports all outcomes", {
  path <- tempfile("shim-fixture")
  dir.create(path)
  on.exit(unlink(path, recursive = TRUE), add = TRUE)
  writeLines('value <- "helper"', file.path(path, "helper.R"))
  writeLines('value <- paste(value, "setup")', file.path(path, "setup.R"))
  writeLines(c(
    'test_that("a", {',
    '  expect_identical(value, "helper setup")',
    '  expect_equal(1, 2)',
    '  warning("record this warning")',
    '  succeed()',
    '})',
    'test_that("abort", stop("first line\\nsecond line"))',
    'test_that("caf\\u00e9", expect_snapshot(stop("This expression must stay unevaluated.")))'
  ), file.path(path, "test-A.R"))
  writeLines(c(
    'test_that("b", {',
    '  expect_identical(test_path("data"), file.path(getwd(), "data"))',
    '  skip_if(FALSE)',
    '  skip_if_not(TRUE)',
    '  skip_if_not_installed("base", "3.0")',
    '  skip_if_offline()',
    '  stop("This line must stay unevaluated.")',
    '})'
  ), file.path(path, "test-b.r"))

  lines <- capture.output(result <- test_dir(path, stop_on_failure = FALSE))
  expect_identical(result, list(fail = 2L, warn = 1L, skip = 2L, pass = 3L))
  expect_identical(lines[[1L]], "ok 1 - test-A.R: a")
  expect_true("  1 not equal to 2." %in% lines)
  expect_true("  second line" %in% lines)
  expect_true("skip 5 - test-A.R: caf?" %in% lines)
  expect_identical(lines[[length(lines)]], "[ FAIL 2 | WARN 1 | SKIP 2 | PASS 3 ]")
})

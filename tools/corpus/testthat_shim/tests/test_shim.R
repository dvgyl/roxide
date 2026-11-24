library(testthat)

test_that("checks values and labels", {
  expect(TRUE, "Unused")
  succeed()
  expect_equal(1, 1 + 1e-10)
  expect_identical(1L, 1L)
  expect_equivalent(c(a = 1), c(b = 1))
  expect_setequal(c(NA, 2, 2, 1), c(1, 2, NA))
  expect_mapequal(list(a = 1, b = 2), list(b = 2, a = 1))
  expect_contains(c(1, 2, NA), c(NA, 2, 2))
  expect_in(c(NA, 2, 2), c(1, 2, NA))
  expect_failure(expect_contains(1, 1:2))
  expect_failure(expect_in(1:2, 1))
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
  expect_vector(1:3, integer(), size = 3)
  expect_vector(matrix(1:6, nrow = 2L), size = 2)
  expect_failure(expect_vector(1:3, character()))
  expect_failure(expect_vector(1:3, size = 2))
  expect_failure(expect_vector(sum))
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
  expect_identical(expect_no_condition(4L), 4L)
  expect_failure(expect_no_error(stop("probe")), "probe", fixed = TRUE)
  expect_failure(expect_no_warning(warning("probe")), "probe", fixed = TRUE)
  expect_failure(expect_no_message(message("probe")), "probe", fixed = TRUE)
  emit <- function() signalCondition(simpleCondition("probe"))
  expect_failure(expect_no_condition(emit(), message = "^probe$"))
  expect_no_condition(emit(), message = "^other$")
  expect_silent(4)
  expect_identical(expect_output({ cat("a\nb\n"); 5L }, "a\nb", fixed = TRUE), 5L)
  expect_output(invisible(4), NA)
  expect_identical(expect_invisible(invisible(9L)), 9L)
  expect_identical(expect_visible(11L), 11L)
})

test_that("captures the call trace before unwinding", {
  innermost <- function() stop("trace probe")
  outermost <- function() force(innermost())
  caught <- expect_error(outermost(), "trace probe")
  calls <- tail(caught$trace$call, 3L)
  expect_identical(calls, list(quote(outermost()), quote(force(innermost())), quote(innermost())))

  original <- simpleError("existing trace")
  original$trace <- list(call = list(quote(saved_call())))
  expect_identical(expect_error(stop(original)), original)
})

test_that("checks internal expectations without adding their results", {
  expect_success(expect_true(TRUE))
  expect_failure(expect_true(FALSE), "is not TRUE", fixed = TRUE)
  expect_failure(expect_success(NULL), "exactly one")
  expect_failure(expect_success({ succeed(); succeed() }), "exactly one")
  expect_failure(expect_success({ succeed(); fail() }), "exactly one")
  expect_failure(expect_failure({ fail(); fail() }), "exactly one")
  expect_failure(expect_failure(fail("found"), "wanted"), "does not match")
  expect_error(expect_failure(stop("ordinary error")), "ordinary error")
  caught <- tryCatch(expect_success(skip("nested skip")), skip = identity)
  expect_identical(conditionMessage(caught), "nested skip")
})

test_that("evaluates comparison operands once in order and preserves random draws", {
  check <- function() {
    seed_env <- globalenv()
    original <- get0(".Random.seed", envir = seed_env, inherits = FALSE)
    on.exit({
      if (is.null(original)) remove(".Random.seed", envir = seed_env) else
        assign(".Random.seed", original, envir = seed_env)
    })
    set.seed(42L)
    preserve <- function(expr) {
      seed <- get(".Random.seed", envir = seed_env, inherits = FALSE)
      on.exit(assign(".Random.seed", seed, envir = seed_env))
      expr
    }
    order <- character()
    expect_equal(
      { order <- c(order, "object"); preserve(runif(3L)) },
      { order <- c(order, "expected"); runif(3L) }
    )
    expect_identical(order, c("object", "expected"))
    expect_identical(preserve(runif(3L)), runif(3L))
  }
  check()
})

test_that("skips each snapshot helper before it evaluates its arguments", {
  helpers <- list(expect_snapshot, expect_snapshot_file, expect_snapshot_error,
                  expect_snapshot_message, expect_snapshot_warning,
                  expect_snapshot_output, expect_snapshot_value, announce_snapshot_file)
  for (helper in helpers) {
    outcome <- tryCatch(helper(stop("Unexpected evaluation.")), skip = identity)
    expect_identical(conditionMessage(outcome), "snapshot tests not supported by minimal shim")
  }
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

test_that("runs groups, fixtures, and optional package checks at the correct scope", {
  path <- tempfile("shim-scope")
  dir.create(path)
  on.exit(unlink(path, recursive = TRUE), add = TRUE)
  events <- new.env()
  events$order <- character()
  writeLines(c(
    'setup({ setup_only_probe <- "local"; events$order <- c(events$order, "setup") })',
    'teardown(events$order <- c(events$order, "first"))',
    'teardown(events$order <- c(events$order, "second"))',
    'describe("group", {',
    '  shared <- 1L',
    '  it("first", { expect_identical(shared, 1L); shared <- 2L })',
    '  describe("nested", {',
    '    it("keeps its parent", expect_identical(shared, 1L))',
    '    it("empty")',
    '  })',
    '})',
    'describe("skipped group", { skip("Unavailable"); stop("Unexpected evaluation.") })',
    'test_that("sees setup effects", {',
    '  expect_false(exists("setup_only_probe"))',
    '  expect_identical(events$order, "setup")',
    '})',
    'stop("File stopped.")'
  ), file.path(path, "test-A.R"))
  writeLines(c(
    'test_that("sees cleanup", expect_identical(events$order, c("setup", "first", "second")))',
    'test_that("skips a declared optional package", loadNamespace("roxideMissingOptional"))',
    'test_that("skips through a negative expectation",',
    '  expect_no_error(loadNamespace("roxideMissingOptional")))',
    'test_that("can assert an optional package error",',
    '  expect_error(loadNamespace("roxideMissingOptional"), class = "packageNotFoundError"))',
    'test_that("fails for an undeclared package", loadNamespace("roxideMissingRequired"))'
  ), file.path(path, "test-B.R"))

  old <- options(testthat.optional_packages = "roxideMissingOptional")
  on.exit(options(old), add = TRUE)
  lines <- capture.output(result <- test_dir(path, stop_on_failure = FALSE))
  expect_identical(result, list(fail = 2L, warn = 0L, skip = 4L, pass = 6L))
  expect_true("ok 2 - test-A.R: group / nested / keeps its parent" %in% lines)
  expect_true("skip 3 - test-A.R: group / nested / empty" %in% lines)
  expect_true("skip 4 - test-A.R: skipped group" %in% lines)
})

expect <- function(ok, failure_message, info = NULL, srcref = NULL) {
  passed <- isTRUE(ok)
  text <- if (passed) "" else paste(failure_message, collapse = "\n")
  if (!passed && !is.null(info)) text <- paste(text, info, sep = "\n")
  classes <- if (passed) {
    c("expectation_success", "expectation", "condition")
  } else {
    c("expectation_failure", "expectation", "error", "condition")
  }
  result <- structure(list(message = text, call = NULL, srcref = srcref),
                      class = classes)
  withRestarts({
    if (passed) signalCondition(result) else stop(result)
  }, continue_test = function() NULL)
  invisible(result)
}

succeed <- function() expect(TRUE, "")
fail <- function(message = "Test failed.", ...) expect(FALSE, message, ...)

capture_expectations <- function(expr) {
  results <- list()
  withCallingHandlers(expr, expectation = function(cond) {
    results[[length(results) + 1L]] <<- cond
    invokeRestart("continue_test")
  })
  results
}

expect_success <- function(expr) {
  results <- capture_expectations(expr)
  expect(length(results) == 1L && inherits(results[[1L]], "expectation_success"),
         "Expected exactly one successful expectation.")
}

expect_failure <- function(expr, message = NULL, ...) {
  results <- capture_expectations(expr)
  if (length(results) != 1L || !inherits(results[[1L]], "expectation_failure")) {
    return(expect(FALSE, "Expected exactly one failed expectation."))
  }
  if (is.null(message)) succeed() else expect_match(conditionMessage(results[[1L]]), message, ...)
}

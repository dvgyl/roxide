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

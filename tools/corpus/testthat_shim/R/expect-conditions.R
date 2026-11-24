matches_condition <- function(cond, regexp, class, inherit, fixed, perl, ...) {
  if (inherits(cond, c("expectation", "skip"))) return(FALSE)
  if (!is.null(class)) {
    matched <- if (inherit) inherits(cond, class) else class(cond)[[1L]] %in% class
    if (!matched) return(FALSE)
  }
  is.null(regexp) || isTRUE(grepl(regexp, conditionMessage(cond), fixed = fixed, perl = perl, ...))
}

muffle_condition <- function(cond) {
  restart <- if (inherits(cond, "warning")) "muffleWarning" else
    if (inherits(cond, "message")) "muffleMessage"
  if (!is.null(restart)) tryCatch(invokeRestart(restart), error = function(e) NULL)
}

check_condition <- function(object, kind, regexp, class, inherit, fixed, perl, ...,
                            all = FALSE, info = NULL, label, absent = FALSE) {
  captured <- NULL
  if (identical(regexp, NA) || identical(regexp, NA_character_)) {
    absent <- TRUE
    regexp <- NULL
  }
  handler <- function(cond) {
    if (!inherits(cond, kind) ||
        !matches_condition(cond, regexp, class, inherit, fixed, perl, ...)) {
      return(invisible(NULL))
    }
    if (absent || is.null(captured) || all) {
      if (is.null(captured)) captured <<- cond
      muffle_condition(cond)
    }
  }

  value <- tryCatch(withCallingHandlers(object, condition = handler), error = function(cond) {
    if ((absent && is_missing_optional_package(cond)) || !inherits(cond, kind) ||
        !matches_condition(cond, regexp, class, inherit, fixed, perl, ...)) {
      stop(cond)
    }
    captured <<- cond
    NULL
  })
  ok <- if (absent) is.null(captured) else !is.null(captured)
  expect(ok, if (absent) {
    paste0(label, " signalled an unexpected ", kind, ": ", conditionMessage(captured))
  } else {
    paste(label, "did not signal the expected", kind)
  }, info)
  invisible(if (absent) value else captured)
}

error_trace <- function(calls) {
  # Remove the calling handler and the frames that signal the error.
  n <- length(calls) - 1L
  while (n > 0L) {
    fn <- calls[[n]][[1L]]
    if (!identical(fn, quote(.handleSimpleError)) && !identical(fn, quote(stop))) break
    n <- n - 1L
  }
  list(call = calls[seq_len(n)])
}

expect_error <- function(object, regexp = NULL, class = NULL, ...,
                         fixed = FALSE, perl = FALSE, inherit = TRUE, info = NULL,
                         label = expression_label(substitute(object))) {
  absent <- identical(regexp, NA) || identical(regexp, NA_character_)
  trace <- NULL
  caught <- tryCatch(withCallingHandlers(list(value = object), error = function(cond) {
    if (!absent && inherits(cond, "simpleError") && is.null(cond[["trace"]])) {
      trace <<- error_trace(sys.calls())
    }
  }), error = function(cond) {
    if (inherits(cond, c("expectation", "skip")) ||
        (absent && is_missing_optional_package(cond))) stop(cond)
    if (!is.null(trace)) cond$trace <- trace
    list(error = cond)
  })
  cond <- caught$error
  ok <- if (absent) is.null(cond) else !is.null(cond) &&
    matches_condition(cond, regexp, class, inherit, fixed, perl, ...)
  expect(ok, paste(label, "did not have the expected error result.",
                  if (!is.null(cond)) conditionMessage(cond)), info)
  invisible(if (absent) caught$value else cond)
}

expect_warning <- function(object, regexp = NULL, class = NULL, ...,
                           fixed = FALSE, perl = FALSE, inherit = TRUE, all = FALSE,
                           info = NULL, label = expression_label(substitute(object))) {
  check_condition(object, "warning", regexp, class, inherit, fixed, perl, ...,
                  all = all, info = info, label = label)
}

expect_message <- function(object, regexp = NULL, class = NULL, ...,
                           fixed = FALSE, perl = FALSE, inherit = TRUE, all = FALSE,
                           info = NULL, label = expression_label(substitute(object))) {
  check_condition(object, "message", regexp, class, inherit, fixed, perl, ...,
                  all = all, info = info, label = label)
}

expect_condition <- function(object, regexp = NULL, class = NULL, ...,
                             fixed = FALSE, perl = FALSE, inherit = TRUE, all = FALSE,
                             info = NULL, label = expression_label(substitute(object))) {
  check_condition(object, "condition", regexp, class, inherit, fixed, perl, ...,
                  all = all, info = info, label = label)
}

expect_no_error <- function(object, ..., message = NULL, class = NULL) {
  check_condition(object, "error", message, class, TRUE, FALSE, FALSE, ...,
                  label = expression_label(substitute(object)), absent = TRUE)
}

expect_no_warning <- function(object, ..., message = NULL, class = NULL) {
  check_condition(object, "warning", message, class, TRUE, FALSE, FALSE, ...,
                  label = expression_label(substitute(object)), absent = TRUE)
}

expect_no_message <- function(object, ..., message = NULL, class = NULL) {
  check_condition(object, "message", message, class, TRUE, FALSE, FALSE, ...,
                  label = expression_label(substitute(object)), absent = TRUE)
}

expect_no_condition <- function(object, ..., message = NULL, class = NULL) {
  check_condition(object, "condition", message, class, TRUE, FALSE, FALSE, ...,
                  label = expression_label(substitute(object)), absent = TRUE)
}

expect_silent <- function(object) {
  noisy <- FALSE
  mark <- function(cond) {
    noisy <<- TRUE
    muffle_condition(cond)
  }
  output <- capture.output(withCallingHandlers(value <- object, warning = mark, message = mark))
  expect(!noisy && length(output) == 0L, "Expected no output, warnings, or messages.")
  invisible(value)
}

expect_output <- function(object, regexp = NULL, ...,
                          fixed = FALSE, perl = FALSE, info = NULL,
                          label = expression_label(substitute(object)), width = 80L) {
  old <- options(width = width)
  on.exit(options(old), add = TRUE)
  lines <- capture.output(value <- object)
  output <- paste(lines, collapse = "\n")
  absent <- identical(regexp, NA) || identical(regexp, NA_character_)
  ok <- if (absent) length(lines) == 0L else if (is.null(regexp)) length(lines) > 0L else
    grepl(regexp, output, fixed = fixed, perl = perl, ...)
  expect(ok, paste(label, "did not produce the expected output:", output), info)
  invisible(value)
}

expect_invisible <- function(call, label = expression_label(substitute(call))) {
  result <- withVisible(eval(substitute(call), parent.frame()))
  expect(!result$visible, paste(label, "returned a visible value."))
  invisible(result$value)
}

expect_visible <- function(call, label = expression_label(substitute(call))) {
  result <- withVisible(eval(substitute(call), parent.frame()))
  expect(result$visible, paste(label, "returned an invisible value."))
  invisible(result$value)
}

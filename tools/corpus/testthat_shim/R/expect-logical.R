expect_true <- function(object, info = NULL, label = expression_label(substitute(object))) {
  expect(isTRUE(object), paste(label, "is not TRUE."), info)
  invisible(object)
}

expect_false <- function(object, info = NULL, label = expression_label(substitute(object))) {
  expect(isFALSE(object), paste(label, "is not FALSE."), info)
  invisible(object)
}

expect_null <- function(object, info = NULL, label = expression_label(substitute(object))) {
  expect(is.null(object), paste(label, "is not NULL."), info)
  invisible(object)
}

expect_length <- function(object, n, info = NULL, label = expression_label(substitute(object))) {
  expect(length(object) == n, sprintf("%s has length %s. Expected %s.", label, length(object), n), info)
  invisible(object)
}

expect_named <- function(object, expected, ignore.order = FALSE, ignore.case = FALSE,
                         info = NULL, label = expression_label(substitute(object))) {
  actual <- names(object)
  if (missing(expected)) {
    ok <- !is.null(actual)
  } else {
    if (ignore.case) {
      if (!is.null(actual)) actual <- tolower(actual)
      if (!is.null(expected)) expected <- tolower(expected)
    }
    if (ignore.order) {
      if (!is.null(actual)) actual <- sort(actual, na.last = TRUE, method = "radix")
      if (!is.null(expected)) expected <- sort(expected, na.last = TRUE, method = "radix")
    }
    ok <- identical(actual, expected)
  }
  expect(ok, paste(label, "does not have the expected names."), info)
  invisible(object)
}

expect_gt <- function(object, expected, info = NULL, label = expression_label(substitute(object))) {
  expect(object > expected, paste(label, "is not greater than", expected), info)
  invisible(object)
}

expect_gte <- function(object, expected, info = NULL, label = expression_label(substitute(object))) {
  expect(object >= expected, paste(label, "is less than", expected), info)
  invisible(object)
}

expect_lt <- function(object, expected, info = NULL, label = expression_label(substitute(object))) {
  expect(object < expected, paste(label, "is not less than", expected), info)
  invisible(object)
}

expect_lte <- function(object, expected, info = NULL, label = expression_label(substitute(object))) {
  expect(object <= expected, paste(label, "is greater than", expected), info)
  invisible(object)
}

expect_type <- function(object, type, info = NULL, label = expression_label(substitute(object))) {
  expect(identical(typeof(object), type), paste(label, "does not have type", type), info)
  invisible(object)
}

expect_s3_class <- function(object, class, exact = FALSE) {
  actual <- attr(object, "class")
  ok <- !isS4(object) && !is.null(actual) &&
    if (exact) identical(actual, class) else subset_of(class, actual)
  expect(ok, paste("Expected S3 class", paste(class, collapse = ", ")))
  invisible(object)
}

subset_of <- function(x, y) {
  for (included in x %in% y) if (!included) return(FALSE)
  TRUE
}

expect_s4_class <- function(object, class) {
  expect(isS4(object) && inherits(object, class), paste("Expected S4 class", class))
  invisible(object)
}

expect_is <- function(object, class, info = NULL, label = expression_label(substitute(object))) {
  expect(inherits(object, class), paste(label, "does not inherit from", class), info)
  invisible(object)
}

expect_match <- function(object, regexp, perl = FALSE, fixed = FALSE, ...,
                         all = TRUE, info = NULL, label = expression_label(substitute(object))) {
  matched <- grepl(regexp, object, perl = perl, fixed = fixed, ...)
  ok <- if (all) !(FALSE %in% matched) else TRUE %in% matched
  expect(ok, paste(label, "does not match", regexp), info)
  invisible(object)
}

expect_no_match <- function(object, regexp, perl = FALSE, fixed = FALSE, ...,
                            info = NULL, label = expression_label(substitute(object))) {
  matched <- grepl(regexp, object, perl = perl, fixed = fixed, ...)
  expect(!(TRUE %in% matched), paste(label, "matches", regexp), info)
  invisible(object)
}

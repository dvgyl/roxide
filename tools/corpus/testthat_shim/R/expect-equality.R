expression_label <- function(code) paste(deparse(code, width.cutoff = 500L), collapse = " ")

comparison_message <- function(label, expected.label, result) {
  paste0(label, " not equal to ", expected.label, ".\n",
         paste(result, collapse = "\n"))
}

default_tolerance <- function() {
  tryCatch(.Machine$double.eps ^ 0.5, error = function(e) 1.490116e-08)
}

# Both editions use all.equal(). The shim has no waldo dependency.
expect_equal <- function(object, expected, ..., tolerance = default_tolerance(),
                         info = NULL, label = expression_label(substitute(object)),
                         expected.label = expression_label(substitute(expected))) {
  actual <- object
  target <- expected
  result <- all.equal(target, actual, ..., tolerance = tolerance)
  expect(isTRUE(result), comparison_message(label, expected.label, result), info)
  invisible(actual)
}

expect_identical <- function(object, expected, ..., info = NULL,
                             label = expression_label(substitute(object)),
                             expected.label = expression_label(substitute(expected))) {
  expect(identical(object, expected, ...),
         comparison_message(label, expected.label, "Values differ."), info)
  invisible(object)
}

expect_equivalent <- function(object, expected, ..., tolerance = default_tolerance(),
                              info = NULL, label = expression_label(substitute(object)),
                              expected.label = expression_label(substitute(expected))) {
  expect_equal(object, expected, ..., check.attributes = FALSE, tolerance = tolerance,
               info = info, label = label, expected.label = expected.label)
}

same_set <- function(x, y) {
  subset_of(x, y) && subset_of(y, x)
}

expect_setequal <- function(object, expected) {
  expect(same_set(object, expected), "The sets have different elements.")
  invisible(object)
}

expect_contains <- function(x, y) {
  expect(subset_of(y, x), "The first vector does not contain all values in the second vector.")
  invisible(x)
}

expect_in <- function(x, y) {
  expect(subset_of(x, y), "The second vector does not contain all values in the first vector.")
  invisible(x)
}

expect_mapequal <- function(object, expected) {
  keys <- names(expected)
  result <- same_set(names(object), keys) &&
    isTRUE(all.equal(expected, object[keys], tolerance = default_tolerance()))
  expect(result, "The maps have different names or values.")
  invisible(object)
}

expression_label <- function(code) paste(deparse(code, width.cutoff = 500L), collapse = " ")

comparison_message <- function(label, expected.label, result) {
  paste0(label, " not equal to ", expected.label, ".\n",
         paste(result, collapse = "\n"))
}

default_tolerance <- function() {
  tryCatch(.Machine$double.eps ^ 0.5, error = function(e) 1.490116e-08)
}

compare_proxy <- function(x, path = "x") UseMethod("compare_proxy")
compare_proxy.default <- function(x, path = "x") list(object = x, path = path)

comparison_proxy <- function(x, path, caller) {
  dispatch <- new.env(parent = caller)
  packages <- loadedNamespaces()
  namespaces <- lapply(c(intersect("waldo", packages), setdiff(packages, "waldo")), asNamespace)
  found <- FALSE
  for (name in paste0("compare_proxy.", class(x))) {
    method <- get0(name, envir = caller, mode = "function", inherits = TRUE)
    for (ns in namespaces) {
      if (!is.null(method)) break
      table <- get0(".__S3MethodsTable__.", envir = ns, inherits = FALSE)
      if (is.environment(table)) {
        method <- get0(name, envir = table, mode = "function", inherits = FALSE)
      }
      if (is.null(method)) {
        method <- get0(name, envir = ns, mode = "function", inherits = FALSE)
      }
    }
    if (!is.null(method)) {
      assign(name, method, envir = dispatch)
      found <- TRUE
    }
  }
  if (!found) return(x)
  dispatch$compare_proxy <- compare_proxy
  dispatch$x <- x
  dispatch$path <- path
  eval(quote(compare_proxy(x, path)), dispatch)$object
}

expect_equal <- function(object, expected, ..., tolerance = default_tolerance(),
                         info = NULL, label = expression_label(substitute(object)),
                         expected.label = expression_label(substitute(expected))) {
  actual <- object
  target <- expected
  value <- actual
  if (edition_get() >= 3L) {
    value <- comparison_proxy(actual, "object", parent.frame())
    target <- comparison_proxy(target, "expected", parent.frame())
  }
  result <- all.equal(target, value, ..., tolerance = tolerance)
  expect(isTRUE(result), comparison_message(label, expected.label, result), info)
  invisible(actual)
}

expect_identical <- function(object, expected, ..., info = NULL,
                             label = expression_label(substitute(object)),
                             expected.label = expression_label(substitute(expected))) {
  actual <- object
  target <- expected
  value <- actual
  if (edition_get() >= 3L) {
    value <- comparison_proxy(actual, "object", parent.frame())
    target <- comparison_proxy(target, "expected", parent.frame())
  }
  expect(identical(value, target, ...),
         comparison_message(label, expected.label, "Values differ."), info)
  invisible(actual)
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

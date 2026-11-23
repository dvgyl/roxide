context <- function(desc) invisible(NULL)
expect_that <- function(object, condition, info = NULL, label = NULL) {
  condition(object, info = info, label = label)
}

equals <- function(expected, ...) {
  function(object, info = NULL, label = NULL) {
    expect_equal(object, expected, ..., info = info, label = label)
  }
}

is_identical_to <- function(expected, ...) {
  function(object, info = NULL, label = NULL) {
    expect_identical(object, expected, ..., info = info, label = label)
  }
}

is_equivalent_to <- function(expected, ...) {
  function(object, info = NULL, label = NULL) {
    expect_equivalent(object, expected, ..., info = info, label = label)
  }
}

is_a <- function(class) {
  function(object, info = NULL, label = NULL) expect_is(object, class, info, label)
}

is_true <- function() expect_true
is_false <- function() expect_false
is_null <- function() expect_null

matches <- function(regexp, ...) {
  function(object, info = NULL, label = NULL) {
    expect_match(object, regexp, ..., info = info, label = label)
  }
}

prints_text <- function(regexp = NULL, ...) {
  function(object, info = NULL, label = NULL) {
    output <- capture_output(object, print = TRUE)
    if (is.null(regexp)) expect(nchar(output) > 0L, "Expected printed text.", info) else
      expect_match(output, regexp, ..., info = info, label = label)
    invisible(object)
  }
}

throws_error <- function(regexp = NULL, ...) {
  function(object, info = NULL, label = NULL) {
    expect_error(object, regexp, ..., info = info, label = label)
  }
}

gives_warning <- function(regexp = NULL, ...) {
  function(object, info = NULL, label = NULL) {
    expect_warning(object, regexp, ..., info = info, label = label)
  }
}

shows_message <- function(regexp = NULL, ...) {
  function(object, info = NULL, label = NULL) {
    expect_message(object, regexp, ..., info = info, label = label)
  }
}

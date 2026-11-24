# SPDX-License-Identifier: MIT OR Apache-2.0
dll <- dyn.load(Sys.getenv("ROXIDE_FIXTURE_SO"))
result <- .Call("fixture_vectors", PACKAGE = dll[["name"]])
expected <- list(
    matrix(1:4, 2L),
    array(c(0.5, 1.5, 2.5, 3.5), c(2L, 1L, 2L)),
    c(TRUE, NA),
    as.raw(c(0L, 255L)),
    2 - 3i,
    rep("caf\u00e9", 2L),
    list(matrix(c(99L, 2L:4L), 2L)),
    list(matrix(1:4, 2L)),
    matrix(as.double(1:4), 2L),
    pairlist(7L, fixture_label = 9.5),
    quote(list(7L, fixture_label = 9.5)),
    quote(list()),
    quote(list(7L)),
    quote(list(7L, 9.5)),
    quote(list(7L, 9.5, TRUE)),
    as.raw(165L),
    4 + 5i
)
attr(expected, "fixture_label") <- "caf\u00e9"
stopifnot(identical(result, expected))
stopifnot(identical(Encoding(result[[6L]]), c("latin1", "UTF-8")))
cat("Vector, attribute, and protection checks passed.\n")
stopifnot(identical(.Call("fixture_imbalanced", PACKAGE = dll[["name"]]), 123L))

# SPDX-License-Identifier: MIT OR Apache-2.0
dll <- dyn.load(Sys.getenv("ROXIDE_FIXTURE_SO"))
invoke <- function(name, ...) .Call(name, ..., PACKAGE = dll[["name"]])
make <- function(value, mode = 0L) invoke("fixture_alt_new", value, mode)

stopifnot(identical(invoke("fixture_alt_integer", make(c(2L, 3L, 5L)), c(4L, 6L, 8L)), 14L))
stopifnot(identical(invoke("fixture_alt_duplicate", make(c(2L, 3L))), 32L))
stopifnot(identical(invoke("fixture_alt_real", make(c(1.25, 2.5))), 3.75))
stopifnot(identical(invoke("fixture_alt_duplicate", make(c(1.25, 2.5))), c(1.25, 2.5)))
stopifnot(invoke("fixture_alt_mutate", make(c("a", "b")), make(list(NULL))))
stopifnot(identical(sum(make(c(2L, 3L, 5L))), 10L))
stopifnot(is.null(sum(make(c(2L, 3L, 5L), 1L))))
stopifnot(identical(sum(make(c(2L, 3L, 5L), 2L)), 123L))
stopifnot(identical(make(c(2L, 3L, 5L))[c(TRUE, FALSE)], c(101L, 103L)))

err <- tryCatch(invoke("fixture_alt_no_pointer", make(c(2L, 3L), 3L)), error = identity)
stopifnot(inherits(err, "error"),
          identical(conditionMessage(err),
                    "cannot access data pointer for this ALTVEC object [class: fixture_no_pointer, pkg: fixture]"))
err <- tryCatch(invoke("fixture_alt_duplicate", make(c(2L, 3L), 3L)), error = identity)
stopifnot(inherits(err, "error"),
          identical(conditionMessage(err),
                    "cannot access data pointer for this ALTVEC object [class: fixture_no_pointer, pkg: fixture]"))
cat("Foreign ALTREP checks passed.\n")

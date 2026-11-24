# SPDX-License-Identifier: MIT OR Apache-2.0
dll <- dyn.load(Sys.getenv("ROXIDE_FIXTURE_SO"))
pkg <- dll[["name"]]
object <- new.env(parent = emptyenv())
reference <- .Call("fixture_raii_weak_ref", object, PACKAGE = pkg)
stopifnot(identical(.Call("fixture_raii", function() 42L, object, PACKAGE = pkg), 42L))
stopifnot(identical(.Call("fixture_raii_counts", PACKAGE = pkg), c(0L, 1L)))
failure <- tryCatch(
    .Call("fixture_raii", function() stop("C++ callback error."), object, PACKAGE = pkg),
    error = identity
)
stopifnot(inherits(failure, "error"), identical(conditionMessage(failure), "C++ callback error."))
stopifnot(identical(.Call("fixture_raii_counts", PACKAGE = pkg), c(0L, 2L)))
stopifnot(identical(.Call("fixture_raii_weak_key", reference, PACKAGE = pkg), object))
object <- NULL
invisible(gc())
stopifnot(is.null(.Call("fixture_raii_weak_key", reference, PACKAGE = pkg)))
cat("C++ destructors released both buffers and R objects.\n")
cat(conditionMessage(failure), "\n", sep = "")

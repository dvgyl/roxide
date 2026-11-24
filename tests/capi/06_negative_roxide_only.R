# SPDX-License-Identifier: MIT OR Apache-2.0
dll <- dyn.load(Sys.getenv("ROXIDE_FIXTURE_SO"))
cpp <- dyn.load(Sys.getenv("ROXIDE_FIXTURE_CPP_SO"))

check_error <- function(expr, expected, exact = TRUE) {
    error <- tryCatch(force(expr), error = identity)
    stopifnot(inherits(error, "error"))
    observed <- conditionMessage(error)
    if (exact) {
        stopifnot(identical(observed, expected))
    } else {
        stopifnot(grepl(expected, observed, fixed = TRUE))
    }
    cat(observed, "\n", sep = "")
}

check_error(.C("fixture_overrun", raw(1), PACKAGE = dll[["name"]]),
            'array over-run in .C("fixture_overrun") argument 1')
check_error(.C("fixture_underrun", raw(1), PACKAGE = dll[["name"]]),
            'array under-run in .C("fixture_underrun") argument 1')
check_error(dyn.load(Sys.getenv("ROXIDE_FIXTURE_FOREIGN_SO")),
            "foreign R dependency '@rpath/libR.dylib'", exact = FALSE)
stopifnot(!file.exists(Sys.getenv("ROXIDE_FIXTURE_LOAD_MARKER")))
check_error(.Call("fixture_throw", PACKAGE = cpp[["name"]]),
            "C++ exception escaped native routine: Fixture C++ exception.")

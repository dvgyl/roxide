# SPDX-License-Identifier: MIT OR Apache-2.0
dll <- dyn.load(Sys.getenv("ROXIDE_FIXTURE_SO"))
pkg <- dll[["name"]]
stopifnot(identical(getLoadedDLLs()[[pkg]], dll), isTRUE(dll[["dynamicLookup"]]))
routines <- getDLLRegisteredRoutines(dll)
stopifnot(identical(names(routines), c(".C", ".Call", ".Fortran", ".External")))
stopifnot(identical(names(routines[[".C"]]), "fixture_c"),
          identical(names(routines[[".Call"]]),
                    c("fixture_plus_one", "fixture_callable", "fixture_null", "fixture_lookup_mode")),
          identical(names(routines[[".Fortran"]]), "fixture_fortran_text"),
          identical(names(routines[[".External"]]), c("fixture_external", "fixture_external2")))

lookup <- function(name) getNativeSymbolInfo(name, PACKAGE = dll, withRegistrationInfo = TRUE)
external2 <- lookup("fixture_external2")
stopifnot(identical(external2[["numParameters"]], 1L))
plus_one <- lookup("fixture_plus_one")
mode <- lookup("fixture_lookup_mode")
stopifnot(inherits(plus_one, "CallRoutine"), is.loaded("fixture_plus_one", PACKAGE = pkg))
stopifnot(identical(.Call(plus_one, 41L), 42L))
stopifnot(identical(.Call("fixture_callable", "plus_one", 41L, PACKAGE = pkg), 42L))
stopifnot(identical(.External("fixture_external", value = 7L, PACKAGE = pkg), 7L))
args <- .External2("fixture_external2", value = 8L, PACKAGE = pkg)
stopifnot(identical(args, list("fixture_external2", 8L)))

check_error <- function(expr, expected) {
    error <- tryCatch(force(expr), error = identity)
    stopifnot(inherits(error, "error"), identical(conditionMessage(error), expected))
    cat(conditionMessage(error), "\n", sep = "")
}

check_warning <- function(expr, expected) {
    messages <- character()
    result <- withCallingHandlers(force(expr), warning = function(w) {
        messages <<- c(messages, conditionMessage(w))
        invokeRestart("muffleWarning")
    })
    stopifnot(identical(messages, expected))
    cat(messages, "\n", sep = "")
    result
}

check_error(.Call("fixture_callable", "missing", 1L, PACKAGE = pkg),
            "function 'missing' not provided by package 'fixture_dispatch'")
check_error(.Call(plus_one), "Incorrect number of arguments (0), expecting 1 for 'fixture_plus_one'")
stopifnot(is.null(check_warning(.Call("fixture_null", PACKAGE = pkg),
                                "converting NULL pointer to R NULL")))

c_call <- function(i = 3L, l = TRUE, d = 1.5, z = 1 + 2i, s = "abc", naok = FALSE) {
    .C("fixture_c", i = i, l = l, d = d, z = z, s = s, NAOK = naok, PACKAGE = pkg)
}
stopifnot(identical(c_call(), list(i = 5L, l = FALSE, d = 2, z = 1 + 3i, s = "Xbc")))
check_error(c_call(i = NA_integer_), "NAs in foreign function call (arg 1)")
check_error(c_call(l = NA), "NAs in foreign function call (arg 2)")
for (value in c(NA_real_, NaN, Inf)) {
    check_error(c_call(d = value), "NA/NaN/Inf in foreign function call (arg 3)")
}
for (value in c(NA_complex_, complex(real = NaN), complex(imaginary = Inf))) {
    check_error(c_call(z = value), "complex NA/NaN/Inf in foreign function call (arg 4)")
}
stopifnot(identical(c_call(s = NA_character_)[["s"]], "XA"))
stopifnot(identical(c_call(d = Inf, naok = TRUE)[["d"]], Inf))

stopifnot(identical(.Fortran("FIXTURE_NATIVE_FORTRAN", value = 7L, PACKAGE = pkg)[["value"]], 21L))
fortran <- .Fortran("fixture_fortran_text", "abc", 0L, PACKAGE = pkg)
stopifnot(identical(fortran[[2L]], 1L))
fortran <- check_warning(.Fortran("fixture_fortran_text", c("abc", "def"), 0L, PACKAGE = pkg),
                         "only the first string in a char vector used in .Fortran")
stopifnot(identical(fortran[[1L]], "abc"), identical(fortran[[2L]], 1L))

invisible(.Call(mode, FALSE, FALSE))
stopifnot(!is.loaded("fixture_native_fortran", PACKAGE = pkg, type = "Fortran"))
stopifnot(is.loaded("fixture_plus_one", PACKAGE = pkg))
invisible(.Call(mode, FALSE, TRUE))
stopifnot(!is.loaded("fixture_plus_one", PACKAGE = pkg), identical(.Call(plus_one, 1L), 2L))
check_error(.Call("fixture_plus_one", 1L, PACKAGE = pkg),
            paste0('"fixture_plus_one" not available for .Call() for package "', pkg, '"'))
invisible(.Call(mode, TRUE, FALSE))
dyn.unload(dll[["path"]])
stopifnot(is.null(getLoadedDLLs()[[pkg]]), !is.loaded("fixture_plus_one", PACKAGE = pkg))

# SPDX-License-Identifier: MIT OR Apache-2.0
dll <- dyn.load(Sys.getenv("ROXIDE_FIXTURE_SO"))
pkg <- dll[["name"]]
options(width = 72L)
env <- .Call("fixture_environment", PACKAGE = pkg)
stopifnot(environmentIsLocked(env), bindingIsLocked("x", env))
stopifnot(identical(ls(env, all.names = TRUE), c(".hidden", "x")))
failure <- quote(stop("Eval fixture error.", call. = FALSE))
stopifnot(identical(.Call("fixture_eval", "x + 1L", failure, env, PACKAGE = pkg), 43L))
stopifnot(identical(.Call("fixture_catch", quote(17L), PACKAGE = pkg), 17L))
stopifnot(identical(.Call("fixture_catch", failure, PACKAGE = pkg), "Eval fixture error."))
stopifnot(identical(.Call("fixture_unwind", quote(19L), PACKAGE = pkg), 19L))
message <- tryCatch(
    .Call("fixture_unwind", failure, PACKAGE = pkg),
    error = conditionMessage
)
stopifnot(identical(message, "Eval fixture error."))
call <- quote(fixture_example())
signal <- function(kind) .Call("fixture_signal", call, kind, PACKAGE = pkg)
for (kind in 1L:2L) {
    observed <- NULL
    withCallingHandlers(signal(kind), warning = function(w) {
        observed <<- w
        invokeRestart("muffleWarning")
    })
    stopifnot(inherits(observed, "warning"))
    expected <- c("Plain C warning.", "C warning with a call.")[[kind]]
    stopifnot(identical(conditionMessage(observed), expected))
    if (kind == 2L) stopifnot(identical(conditionCall(observed), call))
    cat(conditionMessage(observed), "\n", sep = "")
}
for (kind in 3L:4L) {
    observed <- tryCatch(signal(kind), error = identity)
    stopifnot(inherits(observed, "error"))
    expected <- c("Plain C error.", "C error with a call.")[[kind - 2L]]
    stopifnot(identical(conditionMessage(observed), expected))
    if (kind == 4L) stopifnot(identical(conditionCall(observed), call))
    cat(conditionMessage(observed), "\n", sep = "")
}

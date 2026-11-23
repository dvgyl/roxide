optim <- function(par, fn, gr = NULL, ...,
                  method = c("Nelder-Mead", "BFGS", "CG", "L-BFGS-B", "SANN", "Brent"),
                  lower = -Inf, upper = Inf, control = list(), hessian = FALSE) {
    method <- .Internal(C_optim_match_arg(method, c("Nelder-Mead", "BFGS", "CG", "L-BFGS-B", "SANN", "Brent")))
    objective <- function(x) fn(x, ...)
    gradient <- if (is.null(gr)) NULL else function(x) gr(x, ...)
    .Internal(C_optim(par, objective, gradient, method, lower, upper, as.list(control), hessian))
}

optimHess <- function(par, fn, gr = NULL, ..., control = list()) {
    objective <- function(x) fn(x, ...)
    gradient <- if (is.null(gr)) NULL else function(x) gr(x, ...)
    .Internal(C_optimhess(par, objective, gradient, as.list(control)))
}

.stats_optimizer_fun <- function(f, env) {
    if (is.function(f)) return(f)
    if (is.character(f) && length(f) == 1L) return(get(f, envir = env, mode = "function"))
    stop("argument is not a function")
}

optimize <- function(f, interval, ..., lower = min(interval), upper = max(interval),
                     maximum = FALSE, tol = 2.220446049250313e-16^0.25) {
    f <- .stats_optimizer_fun(f, parent.frame())
    objective <- function(x) f(x, ...)
    .Internal(C_fmin(objective, lower, upper, maximum, tol))
}
optimise <- optimize

uniroot <- function(f, interval, ..., lower = min(interval), upper = max(interval),
                    f.lower = f(lower, ...), f.upper = f(upper, ...),
                    extendInt = c("no", "yes", "downX", "upX"), check.conv = FALSE,
                    tol = 2.220446049250313e-16^0.25, maxiter = 1000, trace = 0) {
    if (!missing(interval) && length(interval) != 2L) stop("'interval' must be a vector of length 2")
    if (!(lower < upper)) stop("lower < upper  is not fulfilled")
    extendInt <- .Internal(C_optim_match_arg(extendInt, c("no", "yes", "downX", "upX")))
    objective <- function(x) f(x, ...)
    .Internal(C_zeroin2(objective, lower, upper, f.lower, f.upper, extendInt, check.conv, tol, maxiter, trace))
}

integrate <- function(f, lower, upper, ..., subdivisions = 100L, rel.tol = 2.220446049250313e-16^0.25,
                      abs.tol = rel.tol, stop.on.error = TRUE, keep.xy = FALSE, aux = NULL) {
    f <- .stats_optimizer_fun(f, parent.frame())
    integrand <- function(x) f(x, ...)
    if (is.finite(lower) && is.finite(upper))
        .Internal(C_dqags(integrand, lower, upper, subdivisions, rel.tol, abs.tol, stop.on.error, match.call()))
    else
        .Internal(C_dqagi(integrand, lower, upper, subdivisions, rel.tol, abs.tol, stop.on.error, match.call()))
}

print.integrate <- function(x, digits = getOption("digits"), ...) {
    .Internal(C_print_integrate(x, digits))
    invisible(x)
}

nlminb <- function(start, objective, gradient = NULL, hessian = NULL, ...,
                   scale = 1, control = list(), lower = -Inf, upper = Inf) {
    f <- function(x) objective(x, ...)
    g <- if (is.null(gradient)) NULL else function(x) gradient(x, ...)
    h <- if (is.null(hessian)) NULL else function(x) hessian(x, ...)
    defaults <- list(eval.max = 200L, iter.max = 150L, trace = 0L, abs.tol = 0,
                     rel.tol = 1e-10, x.tol = 1.5e-8, xf.tol = 2.2e-14,
                     step.min = 1, step.max = 1, sing.tol = 1e-10,
                     diff.g = 2.220446049250313e-16)
    for (name in names(control)) {
        if (!is.na(.model_match(name, names(defaults)))) defaults[[name]] <- control[[name]]
        else warning(paste("unrecognized control element named", paste0("'", name, "'"), "ignored"))
    }
    n <- length(start)
    out <- .Internal(C_smooth_nonlinear(start, f, g, h, rep(scale, length.out = n),
        vapply(defaults, as.double, 0), rep(lower, length.out = n), rep(upper, length.out = n), TRUE))
    names(out) <- c("par", "objective", "convergence", "iterations", "evaluations", "message")
    names(out$evaluations) <- c("function", "gradient")
    out
}

nlm <- function(f, p, ..., hessian = FALSE, typsize = rep(1, length(p)),
                fscale = 1, print.level = 0, ndigit = 12, gradtol = 1e-6,
                stepmax = max(1000 * sqrt(sum((p / typsize)^2)), 1000),
                steptol = 1e-6, iterlim = 100, check.analyticals = TRUE) {
    objective <- function(x) f(x, ...)
    out <- .Internal(C_smooth_nonlinear(p, objective, NULL, NULL, typsize,
        c(fscale, print.level, ndigit, gradtol, stepmax, steptol, iterlim,
          check.analyticals, hessian), NULL, NULL, FALSE))
    names(out) <- c("minimum", "estimate", "gradient",
                    if (hessian) "hessian", "code", "iterations")
    out
}

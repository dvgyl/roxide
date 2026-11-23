loess.control <- function(surface = c("interpolate", "direct"),
                          statistics = c("approximate", "exact", "none"),
                          trace.hat = c("exact", "approximate"), cell = .2,
                          iterations = 4, iterTrace = FALSE, ...) {
    iterations <- as.integer(iterations)
    if (length(iterations) != 1L || is.na(iterations) || iterations <= 0L)
        stop("as.integer(iterations) > 0L is not TRUE")
    list(surface = .smooth_match(surface, c("interpolate", "direct")),
         statistics = .smooth_match(statistics, c("approximate", "exact", "none")),
         trace.hat = .smooth_match(trace.hat, c("exact", "approximate")),
         cell = cell, iterations = iterations, iterTrace = iterTrace)
}

.smooth_loess_options <- function(d, pars, statistics) {
    c(d, pars$degree, pars$span, .model_match(statistics, c("none", "approximate", "exact")) - 1L,
      pars$iterations, pars$surface == "interpolate", pars$normalize,
      pars$cell, pars$parametric, pars$drop.square)
}

.smooth_loess_flags <- function(value, labels, arg) {
    n <- length(labels)
    if (is.character(value)) {
        if (any(is.na(.model_match(value, labels)))) stop(paste("invalid", arg, "specification"))
        return(!is.na(.model_match(labels, value)))
    }
    if (!is.logical(value) || !length(value) || any(is.na(value)))
        stop(paste("invalid", arg, "specification"))
    rep(value, length.out = n)
}

loess <- function(formula, data, weights, subset, na.action, model = FALSE,
                  span = .75, enp.target, degree = 2, parametric = FALSE,
                  drop.square = FALSE, normalize = TRUE,
                  family = c("gaussian", "symmetric"), method = c("loess", "model.frame"),
                  control = loess.control(...), ...) {
    cl <- match.call()
    frame <- eval(.lm_frame_call(cl), parent.frame())
    method <- .smooth_match(method, c("loess", "model.frame"))
    if (method == "model.frame") return(frame)
    tt <- terms(frame)
    y <- model.response(frame, "double")
    d <- length(attr(tt, "variables")) - 2L
    if (d < 1L || d > 4L) stop("only 1-4 predictors are allowed")
    if (!length(y)) stop("invalid 'y'")
    columns <- seq_len(d) + 1L
    xnames <- names(frame)[columns]
    xlist <- unclass(frame)[columns]
    if (!all(vapply(xlist, is.numeric, TRUE)) || any(vapply(xlist, is.matrix, TRUE)))
        stop("predictors must all be numeric")
    x <- as.matrix(frame[columns])
    dimnames(x) <- list(rownames(frame), xnames)
    family <- .smooth_match(family, c("gaussian", "symmetric"))
    degree <- as.integer(degree)
    if (length(degree) != 1L || is.na(degree) || degree < 0L || degree > 2L)
        stop("'degree' must be 0, 1 or 2")
    if (!missing(enp.target)) {
        if (!missing(span)) warning("both 'span' and 'enp.target' specified: using 'span'")
        else span <- 1.2 * choose(d + degree, degree) / enp.target
    }
    parametric <- .smooth_loess_flags(parametric, xnames, "parametric")
    drop.square <- .smooth_loess_flags(drop.square, xnames, "drop.square")
    if (degree != 2L && any(drop.square)) stop("drop.square can only be true when degree = 2")
    if (d == 1L && any(drop.square)) stop("cannot drop.square with only one predictor")
    if (all(parametric)) stop("all predictors are parametric")
    w <- model.weights(frame)
    if (is.null(w)) w <- rep(1, length(y)) else w <- as.double(w)
    pars <- list(span = span, degree = degree, normalize = normalize, parametric = parametric,
                 drop.square = drop.square, surface = control$surface, cell = control$cell,
                 family = family, trace.hat = control$trace.hat,
                 iterations = if (family == "gaussian") 1L else control$iterations)
    result <- .Internal(C_smooth_loess(x, y, w,
        .smooth_loess_options(d, pars, control$statistics), NULL))
    stats <- result[[5L]]
    kd <- result[[6L]]
    if (!is.null(kd)) {
        names(kd) <- c("parameter", "a", "xi", "vert", "vval")
        names(kd$parameter) <- c("d", "n", "vc", "nc", "nv", "liv", "lv")
    }
    divisor <- result[[3L]]
    if (normalize && d > 1L) names(divisor) <- xnames
    names(xnames) <- xnames
    out <- list(n = length(y), fitted = result[[1L]],
                residuals = setNames(result[[2L]], names(y)), enp = stats[1L],
                s = stats[2L], one.delta = stats[3L], two.delta = stats[4L],
                trace.hat = stats[5L], divisor = divisor, robust = result[[4L]],
                pars = pars, kd = kd, call = cl, terms = tt, xnames = xnames,
                x = x, y = y, weights = w)
    action <- attr(frame, "na.action")
    if (model) out$model <- frame
    if (!is.null(action)) out$na.action <- action
    structure(out, class = "loess")
}

predict.loess <- function(object, newdata = NULL, se = FALSE, na.action = na.pass, ...) {
    tt <- delete.response(object$terms)
    xnames <- unname(object$xnames)
    d <- length(xnames)
    labels <- NULL
    original <- is.null(newdata)
    if (original) {
        x <- object$x
        labels <- rownames(x)
    } else if (is.data.frame(newdata) || is.list(newdata) || is.environment(newdata)) {
        frame <- model.frame(tt, newdata, na.action = na.action)
        x <- as.matrix(frame[xnames])
        labels <- rownames(frame)
    } else {
        x <- if (is.matrix(newdata)) newdata else matrix(newdata, ncol = d)
        if (ncol(x) != d) stop("wrong number of columns in 'newdata'")
        if (!is.null(colnames(x)) && all(!is.na(.model_match(xnames, colnames(x)))))
            x <- x[, xnames, drop = FALSE]
        labels <- rownames(x)
    }
    if (original && !se) return(napredict(object$na.action, object$fitted))
    out <- .Internal(C_smooth_loess_predict(object$x, object$y, object$weights,
        .smooth_loess_options(d, object$pars, "none"), x,
        c(object$s, se, object$robust)))
    fit <- setNames(out[[1L]], labels)
    if (!se) return(fit)
    err <- setNames(out[[2L]], labels)
    list(fit = fit, se.fit = err, residual.scale = object$s,
         df = object$one.delta^2 / object$two.delta)
}

summary.loess <- function(object, ...) {
    class(object) <- "summary.loess"
    object
}

print.loess <- function(x, digits = max(3L, getOption("digits") - 3L), ...) {
    cat("Call:\n")
    cat(paste(deparse(x$call), collapse = "\n"), "\n\n", sep = "")
    cat("Number of Observations:", x$n, "\n")
    cat("Equivalent Number of Parameters:", round(x$enp, 2L), "\n")
    cat("Residual Standard Error:", .smooth_format(x$s, digits), "\n")
    invisible(x)
}

print.summary.loess <- function(x, digits = max(3L, getOption("digits") - 3L), ...) {
    print.loess(x, digits = digits)
    p <- x$pars
    cat("Trace of smoother matrix: ", round(x$trace.hat, 2L),
        "  (", p$trace.hat, ")\n\n", sep = "")
    cat("Control settings:\n")
    cat("  span     : ", p$span, "\n")
    cat("  degree   : ", p$degree, "\n")
    cat("  family   :  ", p$family, "\n", sep = "")
    cat("  surface  : ", p$surface)
    if (p$surface == "interpolate") cat("\t  cell =", p$cell)
    cat("\n  normalize:  ", p$normalize, "\n", sep = "")
    cat(" parametric:  ", paste(p$parametric, collapse = " "), "\n", sep = "")
    cat("drop.square: ", p$drop.square, "\n")
    invisible(x)
}

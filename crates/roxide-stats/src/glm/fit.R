glm.control <- function(epsilon = 1e-8, maxit = 25, trace = FALSE) {
    if (!is.numeric(epsilon) || length(epsilon) != 1L || !is.finite(epsilon) || epsilon <= 0)
        stop("value of 'epsilon' must be > 0")
    if (!is.numeric(maxit) || length(maxit) != 1L || !is.finite(maxit) ||
        maxit <= 0 || maxit > 2147483647)
        stop("maximum number of iterations must be > 0")
    list(epsilon = epsilon, maxit = maxit, trace = trace)
}

.glm_valid <- function(family, eta, mu) {
    (is.null(family$valideta) || identical(as.logical(family$valideta(eta)), TRUE)) &&
        (is.null(family$validmu) || identical(as.logical(family$validmu(mu)), TRUE))
}

.glm_deviance <- function(family, y, mu, weights) {
    sum(family$dev.resids(y, mu, weights))
}

.glm_eta <- function(x, coefficients, offset) {
    as.double(x %*% coefficients) + offset
}

.glm_fit_qr <- function(x, raw, tol, good) {
    p <- ncol(x)
    rank <- raw[[8L]]
    pivot <- raw[[7L]]
    columns <- colnames(x)
    packed <- matrix(raw[[5L]], length(good), p, dimnames = list(rownames(x)[good], columns[pivot]))
    qr <- structure(list(qr = packed, rank = rank, qraux = raw[[6L]], pivot = pivot, tol = tol),
                    class = "qr")
    .Internal(C_lm_size(as.double(p) * p))
    upper <- matrix(0, p, p, dimnames = list(columns[pivot], columns[pivot]))
    span <- seq_len(min(length(good), p))
    upper[span, ] <- packed[span, , drop = FALSE]
    upper[lower.tri(upper)] <- 0
    tail <- rep("", length(good) - rank)
    labels <- if (is.null(columns)) c(tail, rep(NA_character_, rank))
              else c(columns[pivot[seq_len(rank)]], tail)
    list(R = upper, qr = qr, effects = setNames(raw[[3L]], labels))
}

glm.fit <- function(x, y, weights = rep(1, nobs), start = NULL, etastart = NULL,
                    mustart = NULL, offset = rep(0, nobs), family = gaussian(),
                    control = list(), intercept = TRUE, singular.ok = TRUE) {
    x <- as.matrix(x)
    nobs <- nrow(x)
    p <- ncol(x)
    if (is.null(weights)) weights <- rep(1, nobs)
    if (is.null(offset)) offset <- rep(0, nobs)
    if (length(weights) != nobs || length(offset) != nobs || .model_rows(y) != nobs)
        stop("incompatible dimensions")
    control <- do.call(glm.control, control)
    if (!is.function(family$variance) || !is.function(family$linkinv))
        stop("'family' argument seems not to be a valid family object")
    labels <- if (is.matrix(y)) rownames(y) else names(y)
    if (is.matrix(y) && ncol(y) == 1L) y <- y[, 1L]
    if (is.logical(y)) y <- as.double(y)
    supplied <- mustart
    n <- rep(1, nobs)
    eval(family$initialize, environment())
    if (!is.null(supplied)) mustart <- supplied
    names(y) <- labels
    if (length(y) != nobs || length(weights) != nobs || length(mustart) != nobs)
        stop("incompatible dimensions")

    prior <- .lm_which(weights > 0)
    work <- weights
    fit <- list(R = NULL, qr = NULL, effects = NULL)
    rank <- 0
    boundary <- !p
    converged <- !p
    iter <- 0L
    coefficients <- numeric()
    if (!p) {
        eta <- offset
        mu <- family$linkinv(eta)
        if (!.glm_valid(family, eta, mu)) stop("invalid fitted means in empty model")
        dev <- .glm_deviance(family, y, mu, weights)
        work <- weights * family$mu.eta(eta)^2 / family$variance(mu)
    } else {
        if (!is.null(start) && length(start) != p) {
            stop(paste0("length of 'start' should equal ", p,
                        " and correspond to initial coefs for ",
                        if (is.null(colnames(x))) "NULL" else paste(colnames(x), collapse = ", ")))
        }
        previous <- start
        eta <- if (!is.null(etastart)) etastart else if (!is.null(start)) .glm_eta(x, start, offset)
               else family$linkfun(mustart)
        if (length(eta) != nobs) stop("incompatible dimensions")
        mu <- family$linkinv(eta)
        if (!.glm_valid(family, eta, mu))
            stop("cannot find valid starting values: please specify some")
        dev <- .glm_deviance(family, y, mu, weights)
        tol <- min(1e-7, control$epsilon / 1000)
        raw <- NULL
        for (i in seq_len(control$maxit)) {
            iter <- i
            variance <- family$variance(mu)
            derivative <- family$mu.eta(eta)
            if (length(variance) != nobs || length(derivative) != nobs)
                stop("incompatible dimensions")
            if (any(is.na(variance[prior]))) stop("NAs in V(mu)")
            if (any(variance[prior] == 0)) stop("0s in V(mu)")
            if (any(is.na(derivative[prior]))) stop("NAs in d(mu)/d(eta)")
            good <- prior[derivative[prior] != 0]
            if (!length(good)) {
                warning(paste("no observations informative at iteration", i))
                break
            }
            working <- (eta - offset)[good] + (y - mu)[good] / derivative[good]
            scale <- sqrt(weights[good] * derivative[good]^2 / variance[good])
            raw <- .Internal(C_lm_dqrls(x[good, , drop = FALSE] * scale, working * scale,
                                        tol, singular.ok))
            rank <- raw[[8L]]
            work <- rep(0, nobs)
            work[good] <- scale^2
            candidate <- raw[[1L]]
            candidate[raw[[7L]][seq_len(p) > rank]] <- 0
            if (any(!is.finite(candidate))) {
                warning(paste("non-finite coefficients at iteration", i))
                break
            }
            proposed <- .glm_eta(x, candidate, offset)
            mean <- family$linkinv(proposed)
            newdev <- .glm_deviance(family, y, mean, weights)
            if (control$trace) cat("Deviance = ", newdev, " Iterations - ", i, "\n", sep = "")
            boundary <- FALSE
            finite <- is.finite(newdev)
            valid <- .glm_valid(family, proposed, mean)
            if (!finite || !valid) {
                if (is.null(previous))
                    stop("no valid set of coefficients has been found: please supply starting values")
                if (!finite) warning("step size truncated due to divergence", call. = FALSE)
                else warning("step size truncated: out of bounds", call. = FALSE)
                attempts <- 0L
                loop <- if (!finite) 1L else 2L
                while (!finite || !valid) {
                    attempts <- attempts + 1L
                    if (attempts > control$maxit)
                        stop(paste0("inner loop ", loop, "; cannot correct step size"))
                    candidate <- (candidate + previous) / 2
                    proposed <- .glm_eta(x, candidate, offset)
                    mean <- family$linkinv(proposed)
                    newdev <- .glm_deviance(family, y, mean, weights)
                    finite <- is.finite(newdev)
                    valid <- .glm_valid(family, proposed, mean)
                    if (control$trace) cat("Step halved: new deviance = ", newdev, "\n", sep = "")
                }
                boundary <- TRUE
            }
            coefficients <- candidate
            eta <- proposed
            mu <- mean
            converged <- is.finite(dev) && abs(newdev - dev) / (0.1 + abs(newdev)) < control$epsilon
            dev <- newdev
            if (converged) break
            previous <- candidate
        }
        if (!converged) warning("glm.fit: algorithm did not converge", call. = FALSE)
        if (boundary) warning("glm.fit: algorithm stopped at boundary value", call. = FALSE)
        if (is.null(raw)) stop("object 'fit' not found")
        if (family$family == "binomial" && any(mu > 1 - 10 * .glm_eps | mu < 10 * .glm_eps))
            warning("glm.fit: fitted probabilities numerically 0 or 1 occurred", call. = FALSE)
        if (family$family == "poisson" && any(mu < 10 * .glm_eps))
            warning("glm.fit: fitted rates numerically 0 occurred", call. = FALSE)
        if (!length(coefficients)) coefficients <- raw[[1L]]
        coefficients[raw[[7L]][seq_len(p) > rank]] <- NA_real_
        names(coefficients) <- colnames(x)
        fit <- .glm_fit_qr(x, raw, tol, good)
    }
    residuals <- (y - mu) / family$mu.eta(eta)
    nullmean <- if (intercept) sum(weights * y) / sum(weights) else family$linkinv(offset)
    null <- .glm_deviance(family, y, rep(nullmean, length.out = nobs), weights)
    count <- length(prior)
    list(coefficients = coefficients, residuals = setNames(residuals, labels),
         fitted.values = setNames(mu, labels), effects = fit$effects, R = fit$R, rank = rank,
         qr = fit$qr, family = family, linear.predictors = setNames(eta, labels),
         deviance = dev, aic = family$aic(y, n, mu, weights, dev) + 2 * rank,
         null.deviance = null, iter = iter, weights = setNames(work, labels),
         prior.weights = setNames(weights, labels), df.residual = count - rank,
         df.null = count - as.integer(intercept), y = y, converged = converged,
         boundary = boundary)
}

.glm_frame_call <- function(call, terms = NULL, extras = list()) {
    fields <- as.list(call)
    args <- fields[.glm_in(names(fields), c("etastart", "mustart"))]
    .lm_frame_call(call, terms, c(args, extras))
}

.glm_xlevels <- function(tt, frame) {
    vars <- as.list(attr(tt, "variables"))[-1L]
    labels <- .Internal(C_model_labels(vars))
    if (attr(tt, "response")) labels <- labels[-1L]
    levels <- lapply(frame[labels], function(v) {
        if (is.factor(v)) levels(v) else if (is.character(v)) levels(factor(v)) else NULL
    })
    keep <- !vapply(levels, is.null, FALSE)
    setNames(levels[keep], as.character(names(levels)[keep]))
}

glm <- function(formula, family = gaussian, data, weights, subset, na.action,
                start = NULL, etastart, mustart, offset, control = list(...),
                model = TRUE, method = "glm.fit", x = FALSE, y = TRUE,
                singular.ok = TRUE, contrasts = NULL, ...) {
    call <- match.call()
    caller <- parent.frame()
    if (is.character(family)) family <- get(family, envir = caller)
    if (is.function(family)) family <- family()
    if (is.null(family$family)) {
        print(family)
        stop("'family' not recognized")
    }
    frame <- .lm_response_names(.lm_drop_levels(eval(.glm_frame_call(call), caller)))
    if (identical(method, "model.frame")) return(frame)
    tt <- attr(frame, "terms")
    response <- model.response(frame)
    if (is.null(response)) stop("response is missing")
    if (is.numeric(response) && !is.matrix(response)) response <- setNames(as.double(response), names(response))
    w <- model.weights(frame)
    if (!is.null(w)) {
        if (!is.numeric(w)) stop("'weights' must be a numeric vector")
        if (any(w < 0)) stop("negative weights not allowed")
    }
    off <- model.offset(frame)
    design <- model.matrix(tt, frame, contrasts)
    intercept <- attr(tt, "intercept") != 0L
    control <- do.call(glm.control, control)
    fitter <- if (is.function(method)) method else get(method, envir = caller)
    fit <- fitter(x = design, y = response, weights = w, start = start,
                  etastart = frame[["(etastart)"]], mustart = frame[["(mustart)"]],
                  offset = off, family = family, control = control,
                  intercept = intercept, singular.ok = singular.ok)
    if (!is.null(off) && intercept) {
        one <- design[, 1L, drop = FALSE]
        null <- fitter(x = one, y = response, weights = w, offset = off, family = family,
                       control = control, intercept = TRUE)
        if (!null$converged) warning("fitting to calculate the null deviance did not converge")
        fit$null.deviance <- null$deviance
    }
    if (!y) fit$y <- NULL
    if (model) fit$model <- frame
    action <- attr(frame, "na.action")
    if (!is.null(action)) fit$na.action <- action
    if (x) fit$x <- design
    fit <- c(fit, list(call = call, formula = formula(tt), terms = tt,
                       data = if (missing(data)) environment(tt) else data, offset = off,
                       control = control, method = method, contrasts = attr(design, "contrasts"),
                       xlevels = .glm_xlevels(tt, frame)))
    structure(fit, class = c("glm", "lm"))
}

model.frame.glm <- function(formula, ...) {
    if (!is.null(formula[["model"]])) return(formula[["model"]])
    .lm_response_names(.lm_drop_levels(eval(.glm_frame_call(formula$call, terms(formula), list(...)),
                                          environment(terms(formula)))))
}

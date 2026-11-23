.lm_new_design <- function(object, data, na.action) {
    tt <- delete.response(terms(object))
    frame <- tryCatch(model.frame(tt, data, na.action = na.action, xlev = object[["xlevels"]]),
                      error = function(e) {
        message <- conditionMessage(e)
        if (grepl("^factor .* has new levels ", message) &&
            !grepl(", ", sub("^factor .* has new levels ", "", message))) {
            stop(sub(" has new levels ", " has new level ", message), call. = FALSE)
        }
        stop(e)
    })
    X <- model.matrix(tt, frame, contrasts.arg = object[["contrasts"]])
    off <- model.offset(frame)
    expression <- as.list(object$call)[["offset"]]
    if (!is.null(expression)) {
        value <- eval(expression, data, environment(tt))
        action <- attr(frame, "na.action")
        if (length(action)) value <- value[-as.integer(action)]
        off <- if (is.null(off)) value else off + value
    }
    list(design = X, offset = off)
}

.lm_nonestimable <- function(X, q, tol) {
    p <- ncol(X)
    k <- q$rank
    active <- seq_len(k)
    inactive <- seq_len(p)[-active]
    if (!k) inactive <- seq_len(p)
    # Form a basis for the null space of the packed design.
    basis <- matrix(0, p, length(inactive))
    basis[inactive, ] <- diag(length(inactive))
    if (k) {
        r <- q$qr[active, active, drop = FALSE]
        basis[active, ] <- -backsolve(r, q$qr[active, inactive, drop = FALSE])
    }
    orthogonal <- qr.Q(qr(basis))
    permuted <- X[, q$pivot, drop = FALSE]
    component <- permuted %*% orthogonal
    bad <- sqrt(rowSums(component^2)) > tol * sqrt(rowSums(permuted^2))
    setNames(.lm_which(bad), rownames(X)[.lm_which(bad)])
}
.lm_prediction_variance <- function(X, r, variance) {
    if (!ncol(X)) return(rep(0, nrow(X)))
    z <- backsolve(r, t(X), transpose = TRUE)
    colSums(z^2) * variance
}

predict.lm <- function(object, newdata, se.fit = FALSE, scale = NULL, df = Inf,
                       interval = c("none", "confidence", "prediction"), level = 0.95,
                       type = c("response", "terms"), terms = NULL, na.action = na.pass,
                       pred.var = res.var / weights, weights = 1,
                       rankdeficient = c("warnif", "simple", "non-estim", "NA", "NAwarn"),
                       tol = 1e-6, ...) {
    interval <- .lm_choice(interval, c("none", "confidence", "prediction"))
    type <- .lm_choice(type, c("response", "terms"))
    rankdeficient <- .lm_choice(rankdeficient, c("warnif", "simple", "non-estim", "NA", "NAwarn"))
    training <- missing(newdata) || is.null(newdata)
    if (training) {
        X <- model.matrix(object)
        off <- object[["offset"]]
        omit <- object[["na.action"]]
    } else {
        input <- .lm_new_design(object, newdata, na.action)
        X <- input$design
        off <- input$offset
        omit <- NULL
    }
    rank <- object$rank
    q <- if (ncol(X)) .lm_qr(object) else NULL
    active <- if (rank) q$pivot[seq_len(rank)] else integer()
    xx <- X[, active, drop = FALSE]
    b <- coef(object)[active]
    rows <- rownames(X)
    infer <- se.fit || interval != "none"
    if (infer) {
        if (is.null(scale)) {
            df <- df.residual(object)
            res.var <- deviance(object) / df
        } else res.var <- scale^2
        r <- if (rank) .lm_r(object) else NULL
    }
    if (interval == "prediction") {
        if (training) warning("predictions on current data refer to _future_ responses\n")
        fitted.weights <- object[["weights"]]
        if (missing(weights) && !is.null(fitted.weights) && missing(pred.var)) {
            if (training) {
                warning("assuming prediction variance inversely proportional to weights used for fitting\n")
                weights <- fitted.weights
            } else warning("Assuming constant prediction variance even though model fit is weighted\n")
        }
        if (inherits(weights, "formula")) {
            if (training) stop("'weights' as formula should only be used with 'newdata' argument")
            weights <- eval(weights[[2L]], newdata, environment(weights))
        }
    }

    nonest <- integer()
    if (rank < ncol(X)) {
        if (rankdeficient == "simple" && !training) {
            warning('prediction from rank-deficient fit; consider predict(., rankdeficient="NA")')
        } else if (!training) {
            nonest <- .lm_nonestimable(X, q, tol)
            if (length(nonest) && rankdeficient == "warnif") {
                warning('prediction from rank-deficient fit; attr(*, "non-estim") has doubtful cases')
            }
            if (length(nonest) && rankdeficient == "NAwarn") {
                warning("prediction from rank-deficient fit: NAs produced for non-estimable cases")
            }
        }
    }
    if (type == "response") {
        fit <- setNames(if (rank) as.double(xx %*% b) else rep(0, nrow(X)), rows)
        if (!is.null(off)) fit <- fit + off
        if ((rankdeficient == "NA" || rankdeficient == "NAwarn") && length(nonest)) fit[nonest] <- NA_real_
        if (infer) ip <- setNames(.lm_prediction_variance(xx, r, res.var), rows)
        if (interval != "none") {
            width <- qt((1 + level) / 2, df) * sqrt(ip + if (interval == "prediction") pred.var else 0)
            fit <- cbind(fit, lwr = fit - width, upr = fit + width)
            dimnames(fit) <- list(rows, c("fit", "lwr", "upr"))
        }
        if (rank < ncol(X) && !training &&
            (rankdeficient == "non-estim" || (rankdeficient == "warnif" && length(nonest)))) {
            attr(fit, "non-estim") <- nonest
        }
        fit <- napredict(omit, fit)
        if (!se.fit) return(fit)
        return(list(fit = fit, se.fit = napredict(omit, sqrt(ip)), df = df,
                    residual.scale = sqrt(res.var)))
    }

    tt <- object$terms
    labels <- attr(tt, "term.labels")
    if (!is.null(terms)) {
        chosen <- .model_match(terms, labels)
        if (any(is.na(chosen))) stop("subscript out of bounds")
    } else chosen <- seq_along(labels)
    old <- if (training) X else model.matrix(object)
    intercept <- attr(tt, "intercept") != 0L
    center <- if (intercept && rank) colMeans(old[, active, drop = FALSE]) else rep(0, rank)
    if (intercept && rank) xx <- t(t(xx) - center)
    constant <- sum(center * b)
    n <- if (length(labels)) nrow(X) else nrow(old)
    dn <- if (length(labels)) list(rows, labels[chosen]) else NULL
    fit <- matrix(0, n, length(chosen), dimnames = dn)
    if (infer) ip <- fit
    group <- object[["assign"]][active]
    for (j in seq_along(chosen)) {
        columns <- group == chosen[[j]]
        if (any(columns)) {
            fit[, j] <- as.double(xx[, columns, drop = FALSE] %*% b[columns])
            if (infer) {
                block <- xx
                block[, !columns] <- 0
                ip[, j] <- .lm_prediction_variance(block, r, res.var)
            }
        }
    }
    attr(fit, "constant") <- constant
    if (interval != "none") {
        width <- qt((1 + level) / 2, df) * sqrt(ip + if (interval == "prediction") pred.var else 0)
        lwr <- napredict(omit, fit - width)
        upr <- napredict(omit, fit + width)
    }
    fit <- napredict(omit, fit)
    if (!infer) return(fit)
    errors <- sqrt(ip)
    if (se.fit) errors <- napredict(omit, errors)
    result <- list(fit = fit, se.fit = errors)
    if (interval != "none") result <- c(result, list(lwr = lwr, upr = upr))
    c(result, list(df = df, residual.scale = sqrt(res.var)))
}

predict.mlm <- function(object, newdata, se.fit = FALSE, na.action = na.pass, ...) {
    if (se.fit) stop("the 'se.fit' argument is not yet implemented for \"mlm\" objects")
    if (missing(newdata)) return(fitted(object))
    input <- .lm_new_design(object, newdata, na.action)
    X <- input$design
    b <- coef(object)
    b[is.na(b)] <- 0
    value <- X %*% b
    if (!is.null(input$offset)) value <- value + input$offset
    value
}

.lm_which <- function(x) seq_along(x)[!is.na(x) & x]

.lm_method <- function(method) {
    if (method != "qr") warning(paste0("method = '", method, "' is not supported. Using 'qr'"))
}
.lm_extra <- function(call) {
    dots <- as.list(call)[["..."]]
    if (length(dots)) {
        labels <- names(as.list(dots))
        warning(paste0("In ", deparse1(call), " :\n extra argument",
                       if (length(dots) == 1L) "" else "s", " ",
                       paste0("‘", labels, "’", collapse = ", "), " will be disregarded"),
                call. = FALSE)
    }
}
lm.fit <- function(x, y, offset = NULL, method = "qr", tol = 1e-7, singular.ok = TRUE, ...) {
    .lm_method(method)
    .lm_extra(match.call(expand.dots = FALSE))
    .lm_fit(x, y, NULL, offset, tol, singular.ok)
}
lm.wfit <- function(x, y, w, offset = NULL, method = "qr", tol = 1e-7, singular.ok = TRUE, ...) {
    .lm_method(method)
    .lm_extra(match.call(expand.dots = FALSE))
    if (is.null(w)) stop("incompatible dimensions")
    .lm_fit(x, y, w, offset, tol, singular.ok)
}

.lm_fit <- function(x, y, w, offset, tol, singular.ok) {
    if (!is.matrix(x)) stop("'x' must be a matrix")
    n <- nrow(x)
    p <- ncol(x)
    if (!n) stop("0 (non-NA) cases")
    weighted <- !is.null(w)
    if (weighted && (any(is.na(w)) || any(w < 0))) stop("missing or negative weights not allowed")
    if (weighted && length(w) != n) stop("incompatible dimensions")
    if (is.matrix(y) && ncol(y) == 1L) y <- setNames(as.double(y), rownames(y))
    multi <- is.matrix(y)
    if (multi && !ncol(y)) stop("incorrect number of subscripts on matrix")
    if (weighted && !is.null(offset)) y <- y - offset
    if (!p) {
        fit <- y
        fit[] <- 0
        result <- list(coefficients = numeric(), residuals = y, fitted.values = fit)
        if (weighted) result["weights"] <- list(w)
        result$rank <- if (weighted) 0L else 0
        result$df.residual <- length(y)
        return(result)
    }
    if (!weighted && !is.null(offset)) y <- y - offset
    if (.model_rows(y) != n) stop("incompatible dimensions")
    keep <- if (weighted) .lm_which(w != 0) else seq_len(n)
    used <- length(keep)
    if (!used) {
        empty <- if (multi) y[keep, , drop = FALSE] else y[keep]
        fit <- empty
        fit[] <- 0
        return(list(coefficients = rep(NA_real_, p), residuals = empty, fitted.values = fit,
                    weights = numeric(), rank = 0L, df.residual = 0L))
    }
    design <- x
    response <- y
    if (used < n) {
        design <- x[keep, , drop = FALSE]
        response <- if (multi) y[keep, , drop = FALSE] else y[keep]
    }
    if (weighted) {
        scale <- sqrt(w[keep])
        design <- design * scale
        response <- response * scale
    }

    raw <- .Internal(C_lm_dqrls(design, response, tol, singular.ok))
    pivot <- raw[[7L]]
    rank <- raw[[8L]]
    pivoted <- any(pivot != seq_len(p))
    columns <- colnames(x)
    labels <- if (is.null(columns)) paste0("x", seq_len(p)) else columns
    active <- pivot[seq_len(rank)]
    effect.names <- c(labels[active], rep("", used - rank))
    if (multi) {
        responses <- colnames(y)
        coefficients <- matrix(raw[[1L]], p, ncol(y), dimnames = list(labels, responses))
        effects <- matrix(raw[[3L]], used, ncol(y), dimnames = list(effect.names, responses))
        r <- matrix(raw[[2L]], used, ncol(y))
        f <- matrix(raw[[4L]], used, ncol(y))
    } else {
        coefficients <- setNames(raw[[1L]], labels)
        effects <- setNames(raw[[3L]], effect.names)
        r <- raw[[2L]]
        f <- raw[[4L]]
    }
    if (weighted) {
        r <- r / scale
        f <- f / scale
    }
    residuals <- fitted <- y
    if (multi) {
        residuals[keep, ] <- r
        fitted[keep, ] <- f
    } else {
        residuals[keep] <- r
        fitted[keep] <- f
    }
    if (used < n) {
        zero <- .lm_which(w == 0)
        value <- x[zero, active, drop = FALSE] %*%
            if (multi) coefficients[active, , drop = FALSE] else coefficients[active]
        if (multi) {
            fitted[zero, ] <- value
            residuals[zero, ] <- y[zero, , drop = FALSE] - value
        } else {
            fitted[zero] <- as.double(value)
            residuals[zero] <- y[zero] - as.double(value)
        }
    }
    if (!is.null(offset)) fitted <- fitted + offset
    packed <- raw[[5L]]
    attributes(packed) <- attributes(design)
    if (pivoted && !is.null(columns)) dimnames(packed) <- list(rownames(design), columns[pivot])
    qr <- structure(list(qr = packed, qraux = raw[[6L]], pivot = pivot,
                         tol = tol, rank = rank), class = "qr")
    if (weighted) {
        result <- list(coefficients = coefficients, residuals = residuals, fitted.values = fitted,
                       effects = effects, weights = w, rank = rank)
    } else {
        result <- list(coefficients = coefficients, residuals = residuals, effects = effects,
                       rank = rank, fitted.values = fitted)
    }
    c(result, list(assign = attr(x, "assign"), qr = qr, df.residual = used - rank))
}

.lm_frame_call <- function(call, terms = NULL, extras = list()) {
    parts <- as.list(call)
    fields <- c("formula", "data", "subset", "weights", "na.action", "offset")
    fields <- fields[!is.na(.model_match(fields, names(parts)))]
    args <- parts[fields]
    if (!is.null(terms)) {
        args["formula"] <- list(.Internal(C_lm_call(quote(quote), list(terms))))
    }
    .Internal(C_lm_call(quote(model.frame), c(args, extras)))
}

.lm_drop_levels <- function(frame) {
    for (name in names(frame)) {
        value <- frame[[name]]
        if (is.factor(value)) {
            levels <- levels(value)
            seen <- !is.na(.model_match(levels, as.character(value)))
            if (!all(seen)) {
                if (!is.null(attr(value, "contrasts"))) {
                    warning(paste0("contrasts dropped from factor ", name, " due to missing levels"),
                            call. = FALSE)
                }
                frame[[name]] <- factor(value, levels = levels[seen], ordered = inherits(value, "ordered"))
            }
        }
    }
    frame
}

.lm_response_names <- function(frame) {
    tt <- attr(frame, "terms")
    if (!attr(tt, "response") || !is.matrix(frame[[1L]])) return(frame)
    response <- frame[[1L]]
    expression <- tt[[2L]]
    if (typeof(expression) != "language" || !identical(expression[[1L]], quote(cbind))) {
        return(frame)
    }
    inputs <- as.list(expression)[-1L]
    if (length(inputs) != ncol(response)) return(frame)
    labels <- colnames(response)
    if (is.null(labels)) labels <- rep("", length(inputs))
    # Use the name of each bare symbol when the binding has no column label.
    for (i in seq_along(inputs)) {
        if (!nzchar(labels[[i]]) && typeof(inputs[[i]]) == "symbol") {
            labels[[i]] <- as.character(inputs[[i]])
        }
    }
    if (any(nzchar(labels))) {
        dimnames(response) <- list(rownames(response), labels)
        columns <- unclass(frame)
        columns[[1L]] <- response
        class(columns) <- class(frame)
        frame <- columns
    }
    frame
}

lm <- function(formula, data, subset, weights, na.action, method = "qr",
               model = TRUE, x = FALSE, y = FALSE, qr = TRUE, singular.ok = TRUE,
               contrasts = NULL, offset, ...) {
    call <- match.call()
    frame <- .lm_drop_levels(eval(.lm_frame_call(call), parent.frame()))
    frame <- .lm_response_names(frame)
    if (method == "model.frame") return(frame)
    tt <- attr(frame, "terms")
    response <- model.response(frame, "numeric")
    w <- model.weights(frame)
    if (!is.null(w)) {
        if (!is.numeric(w)) stop("'weights' must be a numeric vector")
        attributes(w) <- NULL
    }
    off <- model.offset(frame)
    if (!is.null(off) && length(off) != .model_rows(response)) {
        stop(paste0("number of offsets is ", length(off), " but should equal ", .model_rows(response),
                    " (number of observations)"))
    }
    .lm_method(method)
    multi <- is.matrix(response) && ncol(response) > 1L
    design <- NULL
    if (!length(attr(tt, "term.labels")) && !attr(tt, "intercept")) {
        r <- if (is.null(off)) response else response - off
        fit <- response
        fit[] <- 0
        empty <- if (multi) matrix(numeric(), 0L, ncol(response)) else numeric()
        z <- list(coefficients = empty, residuals = r, fitted.values = fit,
                  weights = w, rank = 0L, df.residual = length(response))
    } else {
        design <- model.matrix(tt, frame, contrasts)
        z <- if (is.null(w)) lm.fit(design, response, offset = off, singular.ok = singular.ok, ...)
             else lm.wfit(design, response, w, offset = off, singular.ok = singular.ok, ...)
    }
    class(z) <- if (multi) c("mlm", "lm") else "lm"
    z$na.action <- attr(frame, "na.action")
    z$offset <- off
    if (!is.null(design)) {
        z$contrasts <- attr(design, "contrasts")
        vars <- as.list(attr(tt, "variables"))[-1L]
        labels <- .Internal(C_model_labels(vars))
        if (attr(tt, "response")) labels <- labels[-1L]
        if (length(labels)) {
            levels <- lapply(frame[labels], function(v) {
                if (is.factor(v)) levels(v) else if (is.character(v)) levels(factor(v)) else NULL
            })
            z$xlevels <- levels[!vapply(levels, is.null, FALSE)]
        }
    }
    z$call <- call
    z$terms <- tt
    if (model) z$model <- frame
    if (x && !is.null(design)) z$x <- design
    if (y) z$y <- response
    if (!qr) z$qr <- NULL
    z
}

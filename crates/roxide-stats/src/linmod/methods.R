.lm_choice <- function(value, choices) {
    if (is.null(value) || identical(value, choices)) return(choices[[1L]])
    if (length(value) != 1L) stop("'arg' must be of length 1")
    exact <- .model_match(value, choices)
    if (!is.na(exact)) return(choices[[exact]])
    hits <- choices[substr(choices, 1L, nchar(value)) == value]
    if (length(hits) == 1L) return(hits[[1L]])
    stop(paste0("'arg' should be one of ", paste0("“", choices, "”", collapse = ", ")))
}

terms.lm <- function(x, ...) x[["terms"]]
formula.lm <- function(x, ...) formula(terms(x))
model.frame.lm <- function(formula, ...) {
    object <- formula
    if (!is.null(object[["model"]])) return(object[["model"]])
    call <- .lm_frame_call(object$call, terms(object), list(...))
    .lm_response_names(.lm_drop_levels(eval(call, environment(terms(object)))))
}
model.matrix.lm <- function(object, ...) {
    if (!is.null(object[["x"]])) return(object[["x"]])
    model.matrix.default(terms(object), model.frame(object), contrasts.arg = object[["contrasts"]], ...)
}
getCall <- function(x, ...) UseMethod("getCall")
getCall.default <- function(x, ...) x[["call"]]
update.default <- function(object, formula., ..., evaluate = TRUE) {
    call <- getCall(object)
    if (is.null(call)) stop("need an object with call component")
    fields <- as.list(call)[-1L]
    if (!missing(formula.)) fields["formula"] <- list(update(formula(object), formula.))
    extras <- as.list(substitute(list(...)))[-1L]
    for (i in seq_along(extras)) {
        name <- names(extras)[i]
        if (length(name) && nzchar(name)) fields[name] <- extras[i]
        else fields <- c(fields, extras[i])
    }
    call <- .Internal(C_lm_call(call[[1L]], fields))
    if (evaluate) eval(call, parent.frame()) else call
}

coef <- function(object, ...) UseMethod("coef")
coef.default <- function(object, complete = TRUE, ...) {
    b <- object[["coefficients"]]
    if (complete) b else b[!is.na(b)]
}
coef.lm <- function(object, complete = TRUE, ...) {
    b <- object[["coefficients"]]
    if (complete) return(b)
    if (is.matrix(b)) return(b[!is.na(b[, 1L]), , drop = FALSE])
    b[!is.na(b)]
}
coefficients <- coef
residuals <- function(object, ...) UseMethod("residuals")
resid <- residuals
residuals.default <- function(object, ...) naresid(object[["na.action"]], object[["residuals"]])
residuals.lm <- function(object, type = c("working", "response", "deviance", "pearson", "partial"), ...) {
    type <- .lm_choice(type, c("working", "response", "deviance", "pearson", "partial"))
    r <- object[["residuals"]]
    w <- weights(object)
    if ((type == "deviance" || type == "pearson") && !is.null(w)) r <- sqrt(w) * r
    r <- naresid(object[["na.action"]], r)
    if (type == "partial") r <- r + predict(object, type = "terms")
    r
}
fitted <- function(object, ...) UseMethod("fitted")
fitted.default <- function(object, ...) {
    value <- object[["fitted.values"]]
    if (is.null(value)) value <- object[["fitted"]]
    napredict(object[["na.action"]], value)
}
fitted.values <- fitted
weights <- function(object, ...) UseMethod("weights")
weights.default <- function(object, ...) object[["weights"]]
df.residual <- function(object, ...) UseMethod("df.residual")
df.residual.default <- function(object, ...) object[["df.residual"]]
deviance <- function(object, ...) UseMethod("deviance")
deviance.default <- function(object, ...) object[["deviance"]]
deviance.lm <- function(object, ...) {
    r <- object[["residuals"]]
    w <- weights(object)
    square <- if (is.null(w)) r^2 else w * r^2
    if (is.matrix(r)) colSums(square) else sum(square)
}
sigma <- function(object, ...) UseMethod("sigma")
sigma.default <- function(object, use.fallback = TRUE, ...) sqrt(deviance(object) / df.residual(object))
sigma.mlm <- function(object, ...) sqrt(colSums(object$residuals^2) / df.residual(object))
nobs <- function(object, ...) UseMethod("nobs")
nobs.lm <- function(object, ...) {
    w <- weights(object)
    if (is.null(w)) .model_rows(object[["residuals"]]) else sum(w != 0)
}
nobs.default <- function(object, use.fallback = FALSE, ...) {
    w <- weights(object)
    if (!is.null(w)) return(sum(w != 0))
    if (use.fallback && !is.null(object[["residuals"]])) return(.model_rows(object[["residuals"]]))
    stop("no 'nobs' method is available")
}

.lm_qr <- function(object) {
    q <- object[["qr"]]
    if (is.null(q)) stop("lm object does not have a proper 'qr' component.\n Rank zero or should not have used lm(.., qr=FALSE).")
    q
}
.lm_r <- function(object) {
    q <- .lm_qr(object)
    q$qr[seq_len(q$rank), seq_len(q$rank), drop = FALSE]
}
.lm_unscaled <- function(object) {
    if (!object$rank) return(matrix(numeric(), 0L, 0L))
    r <- .lm_r(object)
    result <- chol2inv(r)
    labels <- colnames(r)
    dimnames(result) <- list(labels, labels)
    result
}

summary.lm <- function(object, correlation = FALSE, symbolic.cor = FALSE, ...) {
    p <- object$rank
    r <- object[["residuals"]]
    f <- object[["fitted.values"]]
    b <- coef(object)
    w <- weights(object)
    df <- df.residual(object)
    ans <- object[c("call", "terms")]
    if (!is.null(w)) ans$weights <- w
    df.info <- c(p, df, length(b))
    alias <- is.na(b)
    headings <- c("Estimate", "Std. Error", "t value", "Pr(>|t|)")
    rv <- deviance(object) / df
    if (!is.null(w)) r <- sqrt(w) * r
    if (!p) {
        ans$aliased <- alias
        ans$residuals <- r
        ans$df <- df.info
        ans$coefficients <- matrix(numeric(), 0L, 4L, dimnames = list(NULL, headings))
        ans$sigma <- sqrt(rv)
        ans$adj.r.squared <- 0
        ans$r.squared <- 0
        ans$cov.unscaled <- matrix(numeric(), 0L, 0L)
        return(structure(ans, class = "summary.lm"))
    }
    q <- .lm_qr(object)
    active <- q$pivot[seq_len(p)]
    intercept <- attr(terms(object), "intercept")
    if (!is.null(object[["offset"]])) f <- f - object[["offset"]]
    if (intercept) f <- f - if (is.null(w)) (sum(f) / length(f)) else sum(w * f) / sum(w)
    ms <- if (is.null(w)) sum(f^2) else sum(w * f^2)
    if (is.finite(rv) && rv < ((sum(object[["fitted.values"]]) / length(object[["fitted.values"]]))^2 +
                               var(object[["fitted.values"]])) * 1e-30) {
        warning("essentially perfect fit: summary may be unreliable")
    }
    cov <- .lm_unscaled(object)
    se <- sqrt(diag(cov) * rv)
    est <- b[active]
    tvalue <- est / se
    cm <- cbind(est, se, tvalue, 2 * pt(abs(tvalue), df, lower.tail = FALSE))
    dimnames(cm) <- list(names(est), headings)
    ans$residuals <- r
    ans$coefficients <- cm
    ans$aliased <- alias
    ans$sigma <- sqrt(rv)
    ans$df <- df.info
    if (p != intercept) {
        rss <- deviance(object)
        ans$r.squared <- ms / (ms + rss)
        ans$adj.r.squared <- 1 - (1 - ans$r.squared) * ((nrow(q$qr) - intercept) / df)
        ans$fstatistic <- c(value = (ms / (p - intercept)) / rv, numdf = p - intercept, dendf = df)
    } else {
        ans$adj.r.squared <- 0
        ans$r.squared <- 0
    }
    ans$cov.unscaled <- cov
    if (correlation) {
        root <- sqrt(diag(cov))
        ans$correlation <- cov / (root %o% root)
        ans$symbolic.cor <- symbolic.cor
    }
    ans$na.action <- object[["na.action"]]
    structure(ans, class = "summary.lm")
}

.lm_response <- function(object, i) {
    for (field in c("residuals", "fitted.values", "coefficients", "effects", "y")) {
        if (!is.null(object[[field]])) object[[field]] <- object[[field]][, i]
    }
    class(object) <- "lm"
    object
}
summary.mlm <- function(object, ...) {
    y <- object$coefficients
    labels <- colnames(y)
    if (is.null(labels)) labels <- seq_len(ncol(y))
    ans <- lapply(seq_len(ncol(y)), function(i) summary.lm(.lm_response(object, i), ...))
    structure(setNames(ans, paste("Response", labels)), class = "listof")
}

vcov <- function(object, ...) UseMethod("vcov")
.lm_complete_cov <- function(V, aliased, complete) {
    if (!complete || !any(aliased)) return(V)
    labels <- names(aliased)
    out <- matrix(NA_real_, length(labels), length(labels), dimnames = list(labels, labels))
    selected <- .model_match(rownames(V), labels)
    out[selected, selected] <- V
    out
}
vcov.lm <- function(object, complete = TRUE, ...) vcov.summary.lm(summary.lm(object, ...), complete)
vcov.summary.lm <- function(object, complete = TRUE, ...) {
    .lm_complete_cov(object$cov.unscaled * object$sigma^2, object$aliased, complete)
}
.mlm_coefficient_names <- function(terms, responses, count) {
    if (!length(terms)) return(character())
    if (is.null(responses)) responses <- as.character(seq_len(count))
    paste(rep(responses, each = length(terms)), rep(terms, times = length(responses)), sep = ":")
}
vcov.mlm <- function(object, complete = TRUE, ...) {
    r <- object$residuals
    w <- weights(object)
    if (!is.null(w)) stop("'mlm' objects with weights are not supported")
    if (!length(object$coefficients)) return(matrix(numeric(), 0L, 0L))
    b <- object$coefficients[, 1L]
    cov <- .lm_complete_cov(.lm_unscaled(object), is.na(b), complete)
    out <- kronecker(crossprod(r) / df.residual(object), cov)
    labels <- .mlm_coefficient_names(rownames(cov), colnames(r), ncol(r))
    dimnames(out) <- list(labels, labels)
    out
}

confint <- function(object, parm, level = 0.95, ...) UseMethod("confint")
.lm_confint <- function(object, parm, level, critical, ...) {
    b <- coef(object)
    covariance <- vcov(object, ...)
    if (is.matrix(b)) {
        labels <- .mlm_coefficient_names(rownames(b), colnames(b), ncol(b))
        b <- setNames(as.double(b), labels)
    }
    labels <- names(b)
    if (missing(parm)) parm <- labels
    if (is.numeric(parm)) parm <- labels[parm]
    endpoints <- c((1 - level) / 2, (1 + level) / 2)
    ci <- matrix(NA_real_, length(parm), 2L, dimnames =
                 list(parm, paste(sprintf("%.3g", 100 * endpoints), "%")))
    s <- sqrt(diag(covariance))[parm]
    ci[, 1L] <- b[parm] + s * critical(endpoints[[1L]])
    ci[, 2L] <- b[parm] + s * critical(endpoints[[2L]])
    ci
}
confint.default <- function(object, parm, level = 0.95, ...) {
    .lm_confint(object, parm, level, qnorm, ...)
}
confint.lm <- function(object, parm, level = 0.95, ...) {
    .lm_confint(object, parm, level, function(p) qt(p, df.residual(object)), ...)
}

logLik <- function(object, ...) UseMethod("logLik")
logLik.lm <- function(object, REML = FALSE, ...) {
    if (is.matrix(object$residuals)) stop("'logLik.lm' does not support multiple responses")
    w <- weights(object)
    if (is.null(w)) w <- rep(1, length(object$residuals))
    n <- sum(w != 0)
    logw <- sum(log(w[w != 0]))
    p <- object$rank
    used <- if (REML) n - p else n
    value <- (logw - used * (log(2 * 3.141592653589793) + 1 + log(deviance(object) / used))) / 2
    if (REML && p) value <- value - sum(log(abs(diag(.lm_r(object)))))
    structure(value, nall = n, nobs = used, df = p + 1, class = "logLik")
}
AIC <- function(object, ..., k = 2) UseMethod("AIC")
BIC <- function(object, ...) UseMethod("BIC")
.lm_ic <- function(object, others, labels, name, penalty) {
    models <- c(list(object), others)
    ll <- lapply(models, logLik)
    df <- vapply(ll, function(x) attr(x, "df"), 0)
    n <- vapply(ll, function(x) as.double(attr(x, "nobs")), 0)
    score <- -2 * vapply(ll, as.double, 0) + penalty(n) * df
    if (!length(others)) return(score[[1L]])
    if (any(n != n[[1L]])) warning("models are not all fitted to the same number of observations")
    structure(setNames(list(df, score), c("df", name)), class = "data.frame", row.names = labels)
}
AIC.default <- function(object, ..., k = 2) {
    expressions <- as.list(match.call())[-1L]
    expressions <- expressions[names(expressions) != "k"]
    labels <- vapply(expressions, deparse1, "")
    .lm_ic(object, list(...), labels, "AIC", function(n) k)
}
BIC.default <- function(object, ...) {
    labels <- vapply(as.list(match.call())[-1L], deparse1, "")
    .lm_ic(object, list(...), labels, "BIC", log)
}

hatvalues <- function(model, ...) UseMethod("hatvalues")
hatvalues.lm <- function(model, infl = NULL, ...) {
    if (!is.null(infl)) return(infl$hat)
    r <- model$residuals
    w <- weights(model)
    keep <- if (is.null(w)) seq_len(.model_rows(r)) else .lm_which(w != 0)
    labels <- if (is.matrix(r)) rownames(r) else names(r)
    h <- rep(0, length(keep))
    p <- model$rank
    if (p) {
        # The retained columns of Q span the weighted design.
        q <- qr.Q(.lm_qr(model))[, seq_len(p), drop = FALSE]
        h <- rowSums(q^2)
        h[h > 1 - 1e-10] <- 1
    }
    action <- model[["na.action"]]
    h <- naresid(action, setNames(h, labels[keep]))
    if (inherits(action, "exclude")) h[as.integer(action)] <- 0
    h
}
.lm_influence_residuals <- function(model) {
    r <- model$residuals
    w <- weights(model)
    if (!is.null(w)) {
        keep <- w != 0
        r <- if (is.matrix(r)) r[keep, , drop = FALSE] * sqrt(w[keep]) else r[keep] * sqrt(w[keep])
    }
    naresid(model[["na.action"]], r)
}
.lm_column_values <- function(x, values) {
    if (is.matrix(x)) rep(values, each = nrow(x)) else values
}
rstandard <- function(model, ...) UseMethod("rstandard")
rstandard.lm <- function(model, infl = NULL, sd = sigma(model),
                         type = c("sd.1", "predictive"), ...) {
    type <- .lm_choice(type, c("sd.1", "predictive"))
    r <- if (is.null(infl)) .lm_influence_residuals(model) else infl$wt.res
    h <- hatvalues(model, infl = infl)
    value <- if (type == "predictive") r / (1 - h) else (r / sqrt(1 - h)) / .lm_column_values(r, sd)
    value[is.infinite(value)] <- NaN
    value
}
rstudent <- function(model, ...) UseMethod("rstudent")
rstudent.lm <- function(model, infl = NULL, ...) {
    r <- if (is.null(infl)) .lm_influence_residuals(model) else infl$wt.res
    h <- hatvalues(model, infl = infl)
    sd <- if (is.null(infl)) sqrt((.lm_column_values(r, deviance(model)) - r^2 / (1 - h)) / (df.residual(model) - 1L)) else infl$sigma
    value <- r / (sd * sqrt(1 - h))
    value[is.infinite(value) | is.infinite(sd)] <- NaN
    value[is.infinite(sd)] <- 0
    value
}
cooks.distance <- function(model, ...) UseMethod("cooks.distance")
cooks.distance.lm <- function(model, infl = NULL, res = NULL,
                              sd = sqrt(deviance(model) / df.residual(model)),
                              hat = hatvalues(model, infl = infl), ...) {
    if (is.null(res)) res <- if (is.null(infl)) .lm_influence_residuals(model) else infl$wt.res
    result <- ((res / (1 - hat)) / .lm_column_values(res, sd))^2 * hat / model$rank
    at.one <- .lm_which(hat == 1)
    if (is.matrix(result)) result[at.one, ] <- NaN else result[at.one] <- NaN
    action <- model[["na.action"]]
    if (inherits(action, "exclude") && length(action)) {
        missing <- as.integer(action)
        w <- weights(model)
        omitted <- missing
        if (!is.null(w)) {
            zero <- naresid(action, w) == 0
            zero[is.na(zero)] <- FALSE
            omitted <- missing - cumsum(zero)[missing]
        }
        labels <- if (is.matrix(res)) rownames(res) else names(res)
        if (!is.null(labels)) {
            retained <- labels[-missing]
            labels[] <- NA_character_
            labels[-omitted] <- retained
        }
        if (is.matrix(result)) {
            result[missing, ] <- 0
            result[omitted, ] <- NA_real_
            dimnames(result) <- list(labels, colnames(result))
        } else {
            result[missing] <- 0
            result[omitted] <- NA_real_
            names(result) <- labels
        }
    }
    result
}

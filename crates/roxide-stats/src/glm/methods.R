.glm_dispersion <- function(object) {
    fixed <- family(object)$dispersion
    if (!is.null(fixed) && !is.na(fixed)) return(fixed)
    if (.glm_in(family(object)$family, c("poisson", "binomial"))) return(1)
    w <- object$weights
    keep <- w > 0
    if (any(!keep))
        warning("observations with zero weight not used for calculating dispersion")
    if (object$df.residual <= 0) return(NaN)
    sum(w[keep] * object$residuals[keep]^2) / object$df.residual
}

summary.glm <- function(object, dispersion = NULL, correlation = FALSE, symbolic.cor = FALSE, ...) {
    df <- object$df.residual
    rank <- object$rank
    fam <- family(object)
    fixed <- fam$dispersion
    estimated <- is.null(dispersion) && (is.null(fixed) || is.na(fixed)) &&
        !.glm_in(fam$family, c("poisson", "binomial"))
    if (is.null(dispersion)) dispersion <- .glm_dispersion(object)
    prefix <- c("call", "terms", "family", "deviance", "aic", "contrasts", "df.residual",
                "null.deviance", "df.null", "iter")
    ans <- object[prefix]
    if (!is.null(object[["na.action"]])) ans$na.action <- object[["na.action"]]
    ans$deviance.resid <- residuals(object, type = "deviance")
    alias <- is.na(coef(object))
    labels <- if (estimated) c("Estimate", "Std. Error", "t value", "Pr(>|t|)")
              else c("Estimate", "Std. Error", "z value", "Pr(>|z|)")
    if (rank) {
        cov <- .lm_unscaled(object)
        scaled <- dispersion * cov
        active <- object$qr$pivot[seq_len(rank)]
        estimate <- coef(object)[active]
        se <- sqrt(diag(scaled))
        statistic <- estimate / se
        prob <- if (!estimated) 2 * pnorm(abs(statistic), lower.tail = FALSE)
                else if (df > 0) 2 * pt(abs(statistic), df, lower.tail = FALSE)
                else rep(NaN, rank)
        table <- cbind(estimate, se, statistic, prob)
        dimnames(table) <- list(names(estimate), labels)
    } else {
        table <- matrix(NA, 0L, 4L, dimnames = list(NULL, labels))
        cov <- scaled <- matrix(NA, 0L, 0L)
    }
    ans <- c(ans, list(coefficients = table, aliased = alias, dispersion = dispersion,
                       df = c(rank, df, length(alias)), cov.unscaled = cov, cov.scaled = scaled))
    if (correlation && rank) {
        roots <- sqrt(diag(cov))
        ans$correlation <- cov / (roots %o% roots)
        ans$symbolic.cor <- symbolic.cor
    }
    structure(ans, class = "summary.glm")
}

.glm_response_residual <- function(object) {
    if (!is.null(object[["y"]])) return(object$y - object$fitted.values)
    object$residuals * family(object)$mu.eta(object$linear.predictors)
}

residuals.glm <- function(object, type = c("deviance", "pearson", "working", "response", "partial"), ...) {
    type <- .lm_choice(type, c("deviance", "pearson", "working", "response", "partial"))
    mu <- object$fitted.values
    w <- object$prior.weights
    fam <- family(object)
    if (type == "working" || type == "partial") value <- object$residuals
    else {
        response <- .glm_response_residual(object)
        value <- response
        if (type == "pearson") value <- response * sqrt(w / fam$variance(mu))
        if (type == "deviance") {
            y <- object[["y"]]
            if (is.null(y)) y <- response + mu
            value <- sign(response) * sqrt(.glm_floor(fam$dev.resids(y, mu, w), 0))
        }
    }
    value <- naresid(object[["na.action"]], setNames(value, names(object$residuals)))
    if (type == "partial") value <- value + predict(object, type = "terms")
    value
}

fitted.glm <- function(object, ...) napredict(object[["na.action"]], object$fitted.values)
weights.glm <- function(object, type = c("prior", "working"), ...) {
    type <- .lm_choice(type, c("prior", "working"))
    w <- if (type == "prior") object$prior.weights else object$weights
    napredict(object[["na.action"]], w)
}
deviance.glm <- function(object, ...) object$deviance
df.residual.glm <- function(object, ...) object$df.residual
nobs.glm <- function(object, ...) sum(object$prior.weights != 0)
effects <- function(object, ...) UseMethod("effects")
effects.glm <- function(object, ...) object$effects
effects.lm <- function(object, ...) object$effects

vcov.glm <- function(object, complete = TRUE, ...) vcov.summary.glm(summary.glm(object, ...), complete)
vcov.summary.glm <- function(object, complete = TRUE, ...) {
    .lm_complete_cov(object$cov.scaled, object$aliased, complete)
}

logLik.glm <- function(object, ...) {
    fam <- family(object)
    df <- object$rank
    if ((is.null(fam$dispersion) && !.glm_in(fam$family, c("poisson", "binomial"))) ||
        (!is.null(fam$dispersion) && is.na(fam$dispersion))) df <- df + 1
    structure(df - object$aic / 2, nobs = length(object$residuals), df = df, class = "logLik")
}

confint.glm <- function(object, parm, level = 0.95, ...) {
    .lm_confint(object, parm, level, qnorm)
}

predict.glm <- function(object, newdata = NULL, type = c("link", "response", "terms"),
                        se.fit = FALSE, dispersion = NULL, terms = NULL, na.action = na.pass, ...) {
    type <- .lm_choice(type, c("link", "response", "terms"))
    pad <- missing(newdata)
    training <- is.null(newdata)
    action <- object[["na.action"]]
    if (training && !se.fit && type != "terms") {
        value <- if (type == "link") object$linear.predictors else object$fitted.values
        return(if (pad) napredict(action, value) else value)
    }
    if (se.fit && is.null(dispersion)) dispersion <- .glm_dispersion(object)
    residual.scale <- if (se.fit) sqrt(dispersion) else NULL
    object["na.action"] <- list(NULL)
    if (type == "terms") object$assign <- attr(model.matrix(object), "assign")
    value <- predict.lm(object, newdata, se.fit = se.fit, scale = residual.scale,
                        type = if (type == "terms") "terms" else "response", terms = terms,
                        na.action = na.action, ...)
    if (!se.fit) {
        if (type == "response") value <- family(object)$linkinv(value)
        return(if (pad) napredict(action, value) else value)
    }
    fit <- value$fit
    errors <- value$se.fit
    if (type == "response") {
        fam <- family(object)
        errors <- errors * abs(fam$mu.eta(fit))
        fit <- fam$linkinv(fit)
    }
    if (!object$rank && type != "terms") names(errors) <- NULL
    if (pad) {
        fit <- napredict(action, fit)
        errors <- napredict(action, errors)
    }
    list(fit = fit, se.fit = errors, residual.scale = residual.scale)
}

influence <- function(model, ...) UseMethod("influence")

.glm_influence <- function(model, do.coef, pearson, deviance) {
    w <- model[["weights"]]
    r <- model[["residuals"]]
    keep <- if (is.null(w)) seq_along(r) else .lm_which(w != 0)
    w <- if (is.null(w)) rep(1, length(keep)) else w[keep]
    used <- r[keep] * sqrt(w)
    rank <- model$rank
    hat <- rep(0, length(keep))
    if (rank) {
        fac <- .lm_qr(model)
        q <- qr.Q(fac)[, seq_len(rank), drop = FALSE]
        hat <- rowSums(q^2)
    }
    hat[hat > 1 - 1e-10] <- 1
    names(hat) <- names(used)
    remain <- 1 - hat
    rss <- sum(used^2)
    sigma <- sqrt(.glm_floor((rss - used^2 / remain) / (model$df.residual - 1), 0))
    sigma[hat == 1] <- sqrt(rss / model$df.residual)
    coeff <- NULL
    if (do.coef) {
        cols <- if (rank) colnames(fac$qr)[seq_len(rank)] else NULL
        coeff <- matrix(0, length(keep), rank, dimnames = list(names(used), cols))
        if (rank) {
            changes <- t(backsolve(.lm_r(model), t(q)))
            factor <- used / remain
            factor[hat == 1] <- 0
            coeff[,] <- changes * factor
        }
    }
    action <- model[["na.action"]]
    hat <- naresid(action, hat)
    sigma <- naresid(action, sigma)
    if (inherits(action, "exclude")) {
        hat[as.integer(action)] <- 0
        sigma[as.integer(action)] <- sqrt(rss / model$df.residual)
    }
    if (do.coef) coeff <- naresid(action, coeff)
    ans <- list(hat = hat, coefficients = coeff)
    if (!do.coef && rank) names(ans) <- c("hat", NA_character_)
    ans$sigma <- sigma
    ans$wt.res <- naresid(action, used)
    if (!is.null(pearson)) ans$pear.res <- naresid(action, pearson[keep])
    if (!is.null(deviance)) ans$dev.res <- naresid(action, deviance[keep])
    ans
}

influence.lm <- function(model, do.coef = TRUE, ...) .glm_influence(model, do.coef, NULL, NULL)
influence.glm <- function(model, do.coef = TRUE, ...) {
    raw <- model
    raw["na.action"] <- list(NULL)
    .glm_influence(model, do.coef, residuals(raw, "pearson"), residuals(raw, "deviance"))
}
hatvalues.glm <- function(model, infl = influence(model, do.coef = FALSE), ...) infl$hat

rstandard.glm <- function(model, infl = influence(model, do.coef = FALSE),
                          type = c("pearson", "deviance"), ...) {
    type <- .lm_choice(type, c("pearson", "deviance"))
    r <- if (type == "pearson") infl$pear.res else infl$dev.res
    value <- r / sqrt(.glm_dispersion(model) * (1 - infl$hat))
    value[is.infinite(value)] <- NaN
    value
}

rstudent.glm <- function(model, infl = influence(model, do.coef = FALSE), ...) {
    fam <- family(model)
    if (.glm_in(fam$family, c("binomial", "poisson")) || (!is.null(fam$dispersion) && !is.na(fam$dispersion))) {
        return(rstandard.glm(model, infl))
    }
    if (!model$rank) return(rstandard.glm(model, infl))
    value <- infl$pear.res / (sqrt(1 - infl$hat) * infl$sigma)
    value[is.infinite(value)] <- NaN
    value
}

cooks.distance.glm <- function(model, infl = influence(model, do.coef = FALSE), res = infl$pear.res,
                               dispersion = .glm_dispersion(model), hat = infl$hat, ...) {
    value <- (res / (1 - hat))^2 * hat / (dispersion * model$rank)
    value[hat == 1 & !is.na(hat)] <- NaN
    value
}

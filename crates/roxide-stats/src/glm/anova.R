.glm_analysis_columns <- function(out, rdf, dev, ddf, delta, test, model, dispersion, score = NULL) {
    if (is.null(test)) return(out)
    test <- .lm_choice(test, c("Rao", "LRT", "Chisq", "F", "Cp"))
    fam <- family(model)
    fixed <- fam$dispersion
    if (is.null(fixed) && .glm_in(fam$family, c("binomial", "poisson"))) fixed <- 1
    denominator <- if (is.null(fixed) || is.na(fixed)) model$df.residual else Inf
    if (is.null(dispersion)) dispersion <- .glm_dispersion(model)
    if (test == "Cp") {
        out$Cp <- dev + 2 * dispersion * (model$df.null + attr(terms(model), "intercept") - rdf)
        return(out)
    }
    skip <- is.na(ddf) | ddf == 0
    if (test == "F") {
        if (is.infinite(denominator))
            warning(paste0("using F test with a '", fam$family, "' family is inappropriate"))
        f <- delta / ddf / dispersion
        f[skip | (!is.na(f) & f < 0)] <- NA_real_
        out$F <- f
        out["Pr(>F)"] <- list(pf(f, abs(ddf), denominator, lower.tail = FALSE))
    } else {
        if (test == "Rao") {
            out$Rao <- score
            delta <- score
        }
        statistic <- delta / dispersion * sign(ddf)
        statistic[skip | (!is.na(statistic) & statistic < 0)] <- NA_real_
        out["Pr(>Chi)"] <- list(pchisq(statistic, abs(ddf), lower.tail = FALSE))
    }
    out
}

.glm_analysis_data <- function(object) {
    frame <- model.frame(object)
    list(x = model.matrix(object), y = model.response(frame), weights = model.weights(frame),
         offset = object[["offset"]], intercept = attr(terms(object), "intercept") != 0L)
}

.glm_refit <- function(object, data, columns) {
    glm.fit(data$x[, columns, drop = FALSE], data$y, weights = data$weights,
            offset = data$offset, family = family(object), control = object$control,
            intercept = data$intercept)
}

.glm_rao <- function(reduced, design) {
    weights <- reduced$weights
    r <- reduced$residuals
    tol <- if (is.null(reduced$qr)) 1e-11 else reduced$qr$tol
    wfit <- lm.wfit(design, r, weights, tol = tol)
    sum((r^2 - wfit$residuals^2) * weights)
}

anova.glm <- function(object, ..., dispersion = NULL, test = NULL) {
    others <- list(...)
    if (length(others)) return(.glm_analysis_models(c(list(object), others), dispersion, test))
    if (is.null(test) && (.glm_in(family(object)$family, c("poisson", "binomial")) || !is.null(dispersion)))
        test <- "Chisq"
    if (is.null(test)) {
        fixed <- family(object)$dispersion
        if (!is.null(fixed) && !is.na(fixed)) test <- "Chisq"
        else test <- "F"
    }
    data <- .glm_analysis_data(object)
    assignment <- attr(data$x, "assign")
    labels <- attr(terms(object), "term.labels")
    terms.count <- length(labels)
    rdf <- c(object$df.null, rep(NA_integer_, terms.count))
    dev <- c(object$null.deviance, rep(NA_real_, terms.count))
    score <- rep(NA_real_, terms.count + 1L)
    test <- .lm_choice(test, c("Rao", "LRT", "Chisq", "F", "Cp"))
    need.score <- test == "Rao"
    if (need.score && terms.count) previous <- .glm_refit(object, data, assignment == 0)
    for (j in seq_along(labels)) {
        fit <- if (j == terms.count) object else .glm_refit(object, data, assignment <= j)
        rdf[j + 1L] <- fit$df.residual
        dev[j + 1L] <- fit$deviance
        if (need.score) {
            score[j + 1L] <- .glm_rao(previous, data$x[, assignment <= j, drop = FALSE])
            previous <- fit
        }
    }
    ddf <- c(NA_integer_, -diff(rdf))
    delta <- c(NA_real_, -diff(dev))
    out <- list(Df = ddf, Deviance = delta, `Resid. Df` = rdf, `Resid. Dev` = dev)
    out <- .glm_analysis_columns(out, rdf, dev, ddf, delta, test, object, dispersion, score)
    fam <- family(object)
    heading <- paste0("Analysis of Deviance Table\n\nModel: ", fam$family, ", link: ", fam$link,
                      "\n\nResponse: ", .lm_response_label(object),
                      "\n\nTerms added sequentially (first to last)\n\n")
    .lm_anova_table(out, c("NULL", labels), heading)
}

.glm_analysis_models <- function(models, dispersion, test) {
    response <- vapply(models, .lm_response_label, "")
    different <- response != response[[1L]]
    if (any(different)) {
        warning(paste0("models with response ", paste0("“", response[different], "”", collapse = ", "),
                       " removed because response differs from model 1"))
        models <- models[!different]
    }
    sizes <- vapply(models, function(model) length(model$residuals), 0L)
    if (any(sizes != sizes[[1L]])) stop("models were not all fitted to the same size of dataset")
    rdf <- vapply(models, function(model) as.double(model$df.residual), 0)
    dev <- vapply(models, function(model) model$deviance, 0)
    ddf <- c(NA_real_, -diff(rdf))
    delta <- c(NA_real_, -diff(dev))
    largest <- models[[.lm_which(rdf == min(rdf))[[1L]]]]
    if (is.null(test)) {
        fam <- family(largest)
        fixed <- fam$dispersion
        test <- if (!is.null(dispersion) || (!is.null(fixed) && !is.na(fixed))) "Chisq" else "F"
    }
    score <- rep(NA_real_, length(models))
    test <- .lm_choice(test, c("Rao", "LRT", "Chisq", "F", "Cp"))
    if (test == "Rao" && length(models) > 1L) {
        for (j in seq_len(length(models) - 1L) + 1L) {
            smaller <- if (ddf[j] >= 0) j - 1L else j
            larger <- if (smaller == j) j - 1L else j
            score[j] <- sign(ddf[j]) * .glm_rao(models[[smaller]], model.matrix(models[[larger]]))
        }
    }
    out <- list(`Resid. Df` = rdf, `Resid. Dev` = dev, Df = ddf, Deviance = delta)
    out <- .glm_analysis_columns(out, rdf, dev, ddf, delta, test, largest, dispersion, score)
    indices <- seq_along(models)
    formulas <- vapply(models, function(model) {
        f <- formula(model)
        attributes(f) <- NULL
        deparse1(f)
    }, "")
    heading <- c("Analysis of Deviance Table\n", paste0("Model ", indices, ": ", formulas, collapse = "\n"))
    .lm_anova_table(out, as.character(indices), heading)
}

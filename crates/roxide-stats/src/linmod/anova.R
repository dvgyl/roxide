anova <- function(object, ...) UseMethod("anova")
.lm_anova_table <- function(columns, rows, heading) {
    structure(columns, row.names = rows, class = c("anova", "data.frame"), heading = heading)
}
.lm_response_label <- function(object) {
    tt <- terms(object)
    if (!attr(tt, "response")) return("")
    deparse1(tt[[2L]])
}

anova.lm <- function(object, ..., scale = 0, test = "F") {
    others <- list(...)
    if (length(others)) return(.lm_anova_models(c(list(object), others), scale, test))
    rss <- deviance(object)
    rdf <- df.residual(object)
    if (rss < 1e-10 * sum(object$fitted.values^2)) {
        warning("ANOVA F-tests on an essentially perfect fit are unreliable")
    }
    labels <- attr(terms(object), "term.labels")
    n <- length(labels)
    counts <- integer(n)
    squares <- numeric(n)
    if (object$rank) {
        pivot <- .lm_qr(object)$pivot[seq_len(object$rank)]
        group <- object[["assign"]][pivot]
        effect <- object$effects[seq_len(object$rank)]^2
        for (j in seq_len(n)) {
            counts[[j]] <- sum(group == j)
            squares[[j]] <- sum(effect[group == j])
        }
    }
    keep <- counts > 0
    d <- counts[keep]
    sq <- squares[keep]
    ms <- sq / d
    f <- ms / (rss / rdf)
    .lm_anova_table(list(Df = c(d, rdf), `Sum Sq` = c(sq, rss),
                         `Mean Sq` = c(ms, rss / rdf), `F value` = c(f, NA_real_),
                         `Pr(>F)` = c(pf(f, d, rdf, lower.tail = FALSE), NA_real_)),
                    c(labels[keep], "Residuals"),
                    c("Analysis of Variance Table\n", paste0("Response: ", .lm_response_label(object))))
}

.lm_anova_models <- function(models, scale, test) {
    response <- vapply(models, .lm_response_label, "")
    if (any(response != response[[1L]])) {
        omitted <- response != response[[1L]]
        warning(paste0("models with response ", paste0("“", response[omitted], "”", collapse = ", "),
                       " removed because response differs from model 1"))
        models <- models[!omitted]
    }
    sizes <- vapply(models, function(model) length(model$residuals), 0L)
    if (any(sizes != sizes[[1L]])) stop("models were not all fitted to the same size of dataset")
    n <- length(models)
    rdf <- vapply(models, function(model) as.double(df.residual(model)), 0)
    rss <- vapply(models, deviance, 0)
    d <- c(NA_real_, -diff(rdf))
    sq <- c(NA_real_, -diff(rss))
    largest <- .lm_which(rdf == min(rdf))[[1L]]
    variance <- if (scale > 0) scale else rss[[largest]] / rdf[[largest]]
    out <- list(Res.Df = rdf, RSS = rss, Df = d, `Sum of Sq` = sq)
    if (!is.null(test)) {
        test <- .lm_choice(test, c("Rao", "LRT", "Chisq", "F", "Cp"))
        if (test == "Cp") out$Cp <- rss + 2 * variance * (sizes - rdf)
        else if (test == "F") {
            f <- sq / d / variance
            f[!is.na(d) & (d == 0 | f < 0)] <- NA_real_
            out$F <- f
            out["Pr(>F)"] <- list(pf(f, abs(d), rdf[[largest]], lower.tail = FALSE))
        } else {
            statistic <- sq / variance * sign(d)
            statistic[!is.na(d) & (d == 0 | statistic < 0)] <- NA_real_
            out["Pr(>Chi)"] <- list(pchisq(statistic, abs(d), lower.tail = FALSE))
        }
    }
    formulae <- vapply(models, function(model) deparse1(formula(model)), "")
    heading <- paste0("Model ", seq_len(n), ": ", formulae, collapse = "\n")
    .lm_anova_table(out, as.character(seq_len(n)), c("Analysis of Variance Table\n", heading))
}

.mlm_pillai <- function(h, e, df, residual) {
    if (!df) return(c(0, NaN, 0, 0, NA_real_))
    responses <- nrow(e)
    s <- min(df, responses)
    trace <- sum(diag(solve(e + h, h)))
    num <- s * max(df, responses)
    den <- s * (residual - responses + s)
    f <- (den / num) * trace / (s - trace)
    c(trace, f, num, den, pf(f, num, den, lower.tail = FALSE))
}

anova.mlm <- function(object, ..., test = "Pillai") {
    .lm_choice(test, "Pillai")
    models <- c(list(object), list(...))
    n <- length(models)
    rss <- function(model) {
        r <- model$residuals
        w <- weights(model)
        crossprod(if (is.null(w)) r else sqrt(w) * r)
    }
    rdf <- vapply(models, function(model) as.double(df.residual(model)), 0)
    errors <- lapply(models, rss)
    largest <- .lm_which(rdf == min(rdf))[[1L]]
    residual <- rdf[[largest]]
    e <- errors[[largest]]
    heading <- "Analysis of Variance Table\n"
    if (n == 1L) {
        if (!object$rank) stop("object 'ss' not found")
        active <- seq_len(object$rank)
        group <- object[["assign"]][.lm_qr(object)$pivot[active]]
        labels <- c("(Intercept)", attr(terms(object), "term.labels"))
        groups <- seq_along(labels) - 1L
        counts <- vapply(groups, function(term) as.integer(sum(group == term)), 0L)
        keep <- counts > 0L
        tests <- lapply(groups[keep], function(term) {
            effect <- object$effects[active[group == term], , drop = FALSE]
            .mlm_pillai(crossprod(effect), e, nrow(effect), residual)
        })
        tests <- c(tests, list(rep(NA_real_, 5L)))
        out <- list(Df = as.integer(c(counts[keep], residual)))
        rows <- c(labels[keep], "Residuals")
    } else {
        if (any(!vapply(models, inherits, FALSE, "mlm"))) stop("all models must be 'mlm' objects")
        sizes <- vapply(models, function(model) nrow(model$residuals), 0L)
        if (any(sizes != sizes[[1L]])) stop("models were not all fitted to the same size of dataset")
        d <- diff(rdf)
        tests <- lapply(seq_along(d), function(i) {
            h <- (errors[[i + 1L]] - errors[[i]]) * sign(d[[i]])
            .mlm_pillai(h, e, abs(d[[i]]), residual)
        })
        tests <- c(list(rep(NA_real_, 5L)), tests)
        generalized <- vapply(seq_len(n), function(i) det(errors[[i]] / rdf[[i]])^(1 / nrow(e)), 0)
        out <- list(Res.Df = rdf, Df = c(NA_real_, d), Gen.var. = generalized)
        formulae <- vapply(models, function(model) {
            f <- formula(model)
            attributes(f) <- NULL
            deparse1(f)
        }, "")
        heading <- c(heading, paste0("Model ", seq_len(n), ": ", formulae, collapse = "\n"))
        rows <- as.character(seq_len(n))
    }
    columns <- lapply(seq_len(5L), function(j) vapply(tests, function(value) value[[j]], 0))
    names(columns) <- c("Pillai", "approx F", "num Df", "den Df", "Pr(>F)")
    .lm_anova_table(c(out, columns), rows, heading)
}

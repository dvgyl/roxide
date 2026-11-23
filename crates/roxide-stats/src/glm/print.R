.glm_na_note <- function(action) {
    n <- length(action)
    if (n) {
        cat("  (", n, if (n == 1L) " observation" else " observations",
            " deleted due to missingness)\n", sep = "")
    }
}

print.glm <- function(x, digits = max(3L, getOption("digits", 7L) - 3L), ...) {
    cat("\nCall:  ", paste(deparse(x$call), collapse = "\n"), "\n\n", sep = "")
    b <- coef(x)
    if (length(b)) {
        cat("Coefficients:\n")
        .lm_print_vector(b, digits, gap = 2L)
        cat("\n")
    } else cat("No coefficients\n\n\n")
    cat("Degrees of Freedom:", x$df.null, "Total (i.e. Null); ", x$df.residual, "Residual\n")
    .glm_na_note(x[["na.action"]])
    cat("Null Deviance:\t    ", .lm_scalar(x$null.deviance, digits), " \n",
        "Residual Deviance: ", .lm_scalar(x$deviance, digits), " \tAIC: ",
        .lm_scalar(x$aic, digits), "\n", sep = "")
    invisible(x)
}

.glm_print_coefficients <- function(table, digits, signif.stars, signif.legend = signif.stars, ...) {
    test.digits <- max(1L, min(5L, digits - 1L))
    estimates <- table[, 1:2, drop = FALSE]
    decimals <- .lm_trim_decimals(estimates, .lm_decimals(estimates, digits, smallest = TRUE))
    p <- table[, 4L]
    normal <- !is.na(p) & p >= 1e-4
    probability <- .lm_format_p(p, test.digits, .glm_eps)
    probability[normal] <- .lm_fixed(p[normal], .lm_decimals(p[normal], test.digits, smallest = TRUE))
    columns <- list(.lm_fixed(table[, 1L], decimals, trim = FALSE),
                    .lm_fixed(table[, 2L], decimals, trim = FALSE),
                    .lm_fixed(table[, 3L], test.digits), probability)
    stars <- if (signif.stars) .lm_symbols(p) else NULL
    .lm_print_columns(columns, colnames(table), rownames(table), stars, legend = signif.legend)
}

print.summary.glm <- function(x, digits = max(3L, getOption("digits", 7L) - 3L),
                              symbolic.cor = x$symbolic.cor,
                              signif.stars = getOption("show.signif.stars", TRUE), ...) {
    .lm_print_call(x$call)
    table <- x$coefficients
    rank <- x$df[1L]
    if (rank) {
        missing <- sum(x$aliased)
        if (missing) {
            cat("Coefficients: (", missing, " not defined because of singularities)\n", sep = "")
            labels <- names(x$aliased)
            padded <- matrix(NA_real_, length(labels), ncol(table),
                             dimnames = list(labels, colnames(table)))
            padded[rownames(table), ] <- table
            table <- padded
        } else cat("Coefficients:\n")
        .glm_print_coefficients(table, digits, signif.stars, ...)
    } else cat("No Coefficients\n")
    cat("\n(Dispersion parameter for ", x$family$family, " family taken to be ",
        .lm_scalar(x$dispersion, getOption("digits", 7L)), ")\n\n", sep = "")
    value <- .lm_display(c(x$null.deviance, x$deviance), max(5L, digits + 1L))
    value <- .lm_pad(value, max(nchar(value)))
    cat("    Null deviance: ", value[1L], "  on ", x$df.null, "  degrees of freedom\n",
        "Residual deviance: ", value[2L], "  on ", x$df.residual, "  degrees of freedom\n", sep = "")
    .glm_na_note(x[["na.action"]])
    cat("AIC: ", .lm_scalar(x$aic, digits + 1L), "\n\n",
        "Number of Fisher Scoring iterations: ", x$iter, "\n\n", sep = "")
    if (!is.null(x$correlation) && ncol(x$correlation) > 1L)
        .glm_print_correlations(x$correlation, symbolic.cor)
    invisible(x)
}

.glm_print_correlations <- function(correlation, symbolic) {
    cat("Correlation of Coefficients:\n")
    n <- ncol(correlation)
    labels <- colnames(correlation)
    if (!is.null(symbolic) && symbolic) {
        columns <- lapply(seq_len(n), function(i) {
            value <- abs(correlation[, i])
            text <- rep(" ", n)
            for (j in seq_along(value)) {
                if (j == i) text[j] <- "1"
                if (j > i) {
                    r <- value[j]
                    index <- 1L + sum(r >= c(0.3, 0.6, 0.8, 0.9, 0.95, 1))
                    text[j] <- if (is.na(r)) "?" else c(" ", ".", ",", "+", "*", "B", "1")[index]
                }
            }
            text
        })
        .lm_print_columns(columns, rep(" ", n), labels, right = FALSE)
        cat("attr(,\"legend\")\n[1] 0 ‘ ’ 0.3 ‘.’ 0.6 ‘,’ 0.8 ‘+’ 0.9 ‘*’ 0.95 ‘B’ 1\n")
    } else {
        columns <- lapply(seq_len(n - 1L), function(i) {
            j <- seq.int(i, n - 1L)
            value <- rep("", n - 1L)
            value[j] <- .lm_fixed(correlation[j + 1L, i], 2L, trim = FALSE)
            value
        })
        .lm_print_columns(columns, labels[-n], labels[-1L], right = FALSE)
    }
    cat("\n")
}

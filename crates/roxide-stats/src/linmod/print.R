.lm_significance <- "Signif. codes:  0 ‘***’ 0.001 ‘**’ 0.01 ‘*’ 0.05 ‘.’ 0.1 ‘ ’ 1\n"

.lm_pad <- function(x, width, left = FALSE) {
    vapply(x, function(s) {
        fill <- paste(rep(" ", max(0L, width - nchar(s))), collapse = "")
        if (left) paste0(s, fill) else paste0(fill, s)
    }, "")
}
.lm_scalar <- function(x, digits) {
    if (is.na(x)) return(if (is.nan(x)) "NaN" else "NA")
    if (is.infinite(x)) return(if (x < 0) "-Inf" else "Inf")
    sprintf(paste0("%.", digits, "g"), x)
}
.lm_decimals <- function(x, digits, smallest = FALSE) {
    size <- abs(x[is.finite(x) & x != 0])
    if (!length(size)) return(0L)
    extent <- if (smallest) min(size) else max(size)
    min(22L, max(0L, digits - 1L - floor(log(extent, 10))))
}
.lm_trim_decimals <- function(x, decimals) {
    clean <- x[is.finite(x)]
    while (decimals > 0 && all(round(clean, decimals) == round(clean, decimals - 1L))) {
        decimals <- decimals - 1L
    }
    decimals
}
.lm_fixed <- function(x, decimals, trim = TRUE, na = "NA") {
    if (trim) decimals <- .lm_trim_decimals(x, decimals)
    vapply(x, function(v) {
        if (is.na(v) && !is.nan(v)) return(na)
        if (!is.finite(v)) return(.lm_scalar(v, 3L))
        if (round(v, decimals) == 0) v <- 0
        sprintf(paste0("%.", decimals, "f"), v)
    }, "")
}
.lm_display <- function(x, digits, na = "NA") {
    out <- .lm_fixed(x, .lm_decimals(x, digits), na = na)
    finite <- is.finite(x)
    if (any(finite)) {
        compact <- vapply(x[finite], .lm_scalar, "", digits = digits)
        if (max(nchar(out[finite])) > max(nchar(compact)) &&
            all(abs(x[finite]) >= 10^digits | abs(x[finite]) < 1e-4)) out[finite] <- compact
    }
    out
}
.lm_format_p <- function(x, digits, eps, na = "NA") {
    digits <- max(1L, digits)
    out <- vapply(x, .lm_scalar, "", digits = digits)
    out[is.na(x) & !is.nan(x)] <- na
    fixed <- !is.na(x) & x >= max(eps, 1e-4)
    if (any(fixed)) {
        out[fixed] <- .lm_fixed(x[fixed], .lm_decimals(x[fixed], digits), na = na)
    }
    tiny <- !is.na(x) & x < eps
    out[tiny] <- paste("<", .lm_scalar(eps, digits))
    out
}
.lm_symbols <- function(p) {
    vapply(p, function(x) {
        if (is.na(x)) return("")
        if (x <= 0.001) return("***")
        if (x <= 0.01) return("**")
        if (x <= 0.05) return("*")
        if (x <= 0.1) return(".")
        ""
    }, "")
}
.lm_print_columns <- function(columns, labels, rows, stars = NULL, right = TRUE, legend = TRUE) {
    n <- length(columns)
    if (is.null(rows)) rows <- paste0("[", seq_along(columns[[1L]]), ",]")
    if (is.null(labels)) labels <- paste0("[,", seq_len(n), "]")
    row.width <- max(0L, nchar(rows))
    heading <- .lm_pad("", row.width)
    output <- .lm_pad(rows, row.width, left = TRUE)
    for (j in seq_len(n)) {
        label <- labels[[j]]
        cells <- columns[[j]]
        width <- max(nchar(label), nchar(cells))
        heading <- paste(heading, .lm_pad(label, width, left = !right))
        output <- paste(output, .lm_pad(cells, width, left = !right))
    }
    if (length(stars) && any(nzchar(stars))) {
        width <- max(nchar(stars))
        heading <- paste(heading, .lm_pad("", width))
        output <- paste(output, .lm_pad(stars, width, left = TRUE))
    }
    cat(c(heading, output), sep = "\n")
    if (legend && length(stars) && any(nzchar(stars))) cat("---\n", .lm_significance, sep = "")
}
.lm_print_vector <- function(x, digits, gap = 1L) {
    labels <- names(x)
    rows <- .lm_display(x, digits)
    if (is.null(labels)) {
        cat("[1]", paste(rows, collapse = " "), "\n")
    } else {
        name.line <- value.line <- ""
        sep <- paste(rep(" ", gap), collapse = "")
        width <- max(nchar(labels), nchar(rows))
        for (i in seq_along(x)) {
            name.line <- paste0(name.line, .lm_pad(labels[[i]], width), sep)
            value.line <- paste0(value.line, .lm_pad(rows[[i]], width), sep)
        }
        cat(name.line, value.line, sep = "\n")
    }
}

printCoefmat <- function(x, digits = max(3L, getOption("digits", 7L) - 2L),
                         signif.stars = getOption("show.signif.stars", TRUE),
                         signif.legend = signif.stars, dig.tst = max(1L, min(5L, digits - 1L)),
                         cs.ind = seq_len(k), tst.ind = k + 1L,
                         zap.ind = integer(), P.values = NULL, has.Pvalue = nc >= 4L &&
                         !is.null(cn) && any(substr(cn[nc], 1L, 3L) == c("Pr(", "p-v")),
                         eps.Pvalue = 2^-52, na.print = "NA", quote = FALSE, right = TRUE, ...) {
    if (!is.matrix(x) && !is.data.frame(x)) stop("'x' must be coefficient matrix/data frame")
    nc <- ncol(x)
    cn <- colnames(x)
    if (!nc || !nrow(x)) return(invisible(x))
    k <- max(0L, nc - if (has.Pvalue) 2L else 1L)
    if (is.null(P.values)) P.values <- has.Pvalue
    cs.ind <- cs.ind[cs.ind > 0L & cs.ind <= nc]
    tst.ind <- tst.ind[tst.ind > 0L & tst.ind <= nc]
    cs <- x[, cs.ind, drop = FALSE]
    digits.cs <- .lm_trim_decimals(cs, .lm_decimals(cs, digits, smallest = TRUE))
    values <- lapply(seq_len(nc), function(j) {
        column <- x[, j]
        if (any(j == zap.ind)) column <- as.double(vapply(column, .lm_scalar, "", digits = digits))
        if (j == nc && P.values) return(.lm_format_p(column, dig.tst, eps.Pvalue, na.print))
        if (any(j == cs.ind)) return(.lm_fixed(column, digits.cs, trim = FALSE, na = na.print))
        if (any(j == tst.ind)) return(.lm_fixed(column, dig.tst, na = na.print))
        .lm_display(column, digits, na = na.print)
    })
    if (!P.values && has.Pvalue) {
        values <- values[-nc]
        cn <- cn[-nc]
    }
    stars <- if (P.values && signif.stars) .lm_symbols(x[, nc]) else NULL
    if (quote) values <- lapply(values, function(v) paste0('"', v, '"'))
    .lm_print_columns(values, cn, rownames(x), stars, right = right, legend = signif.legend)
    invisible(x)
}

.lm_print_call <- function(call) {
    fields <- as.list(call)[-1L]
    if (inherits(fields[["formula"]], "formula")) {
        formula <- fields[["formula"]]
        attributes(formula) <- NULL
        fields["formula"] <- list(formula)
        call <- .Internal(C_lm_call(call[[1L]], fields))
    }
    cat("\nCall:\n", paste(deparse(call), collapse = "\n"), "\n\n", sep = "")
}
print.lm <- function(x, digits = max(3L, getOption("digits", 7L) - 3L), ...) {
    .lm_print_call(x$call)
    b <- coef(x)
    if (!length(b)) cat("No coefficients\n")
    else {
        cat("Coefficients:\n")
        if (is.matrix(b)) {
            columns <- lapply(seq_len(ncol(b)), function(i) .lm_display(b[, i], digits))
            .lm_print_columns(columns, colnames(b), rownames(b))
        } else .lm_print_vector(b, digits, gap = 2L)
    }
    cat("\n")
    invisible(x)
}
print.summary.lm <- function(x, digits = max(3L, getOption("digits", 7L) - 3L),
                             signif.stars = getOption("show.signif.stars", TRUE),
                             symbolic.cor = x$symbolic.cor, ...) {
    .lm_print_call(x$call)
    cat(if (is.null(x$weights)) "Residuals:\n" else "Weighted Residuals:\n")
    rdf <- x$df[[2L]]
    r <- x$residuals
    if (!rdf) cat("ALL ", length(r), " residuals are 0: no residual degrees of freedom!\n", sep = "")
    else if (rdf > 5L) {
        r <- setNames(quantile(r, names = FALSE), c("Min", "1Q", "Median", "3Q", "Max"))
        .lm_print_vector(r, digits)
    } else .lm_print_vector(r, digits)
    if (nrow(x$coefficients)) {
        aliased <- x$aliased
        cm <- x$coefficients
        if (any(aliased)) {
            cat("\nCoefficients: (", sum(aliased), " not defined because of singularities)\n", sep = "")
            cm <- matrix(NA_real_, length(aliased), ncol(cm),
                         dimnames = list(names(aliased), colnames(cm)))
            cm[rownames(x$coefficients), ] <- x$coefficients
        } else cat("\nCoefficients:\n")
        printCoefmat(cm, digits = digits, signif.stars = signif.stars, ...)
    } else cat("\nNo Coefficients\n")
    cat("\nResidual standard error: ", .lm_scalar(x$sigma, digits), " on ", rdf, " degrees of freedom\n", sep = "")
    omission <- naprint(x[["na.action"]])
    if (nzchar(omission)) cat("  (", omission, ")\n", sep = "")
    stat <- x$fstatistic
    if (length(stat)) {
        width <- digits + 2L
        cat("Multiple R-squared:  ", .lm_pad(.lm_scalar(x$r.squared, digits), width - 2L),
            ",\tAdjusted R-squared:  ", .lm_pad(.lm_scalar(x$adj.r.squared, digits), width - 2L),
            " \n", sep = "")
        cat("F-statistic: ", .lm_pad(.lm_scalar(stat[[1L]], digits), digits + 1L),
            " on ", stat[[2L]], " and ", stat[[3L]], " DF,  p-value: ",
            if (is.na(stat[[1L]])) "NA" else .lm_format_p(pf(stat[[1L]], stat[[2L]], stat[[3L]],
                            lower.tail = FALSE), digits, 2^-52), "\n", sep = "")
    }
    if (!is.null(x$correlation) && nrow(x$correlation) > 1L) {
        cat("\nCorrelation of Coefficients:\n")
        print(round(x$correlation, 2L))
    }
    cat("\n")
    invisible(x)
}

print.anova <- function(x, digits = max(3L, getOption("digits", 7L) - 2L),
                        signif.stars = getOption("show.signif.stars", TRUE), ...) {
    cat(attr(x, "heading"), sep = "\n")
    labels <- names(x)
    p <- substr(labels, 1L, 3L) == "Pr("
    columns <- lapply(seq_along(x), function(j) {
        col <- x[[j]]
        if (p[[j]]) .lm_format_p(col, digits, 2^-52, na = "")
        else .lm_display(col, digits, na = "")
    })
    stars <- if (signif.stars && any(p)) .lm_symbols(x[[.lm_which(p)[[1L]]]]) else NULL
    .lm_print_columns(columns, labels, rownames(x), stars)
    invisible(x)
}

print.dummy_coef <- function(x, ..., title = "Full coefficients are") {
    digits <- getOption("digits", 7L)
    cat(title, "\n")
    labels <- paste0(names(x), ":")
    width <- max(nchar(labels)) + 4L
    lines <- character()
    values <- lapply(x, function(value) .lm_display(value, digits))
    first <- max(vapply(seq_along(x), function(i) {
        label <- if (length(x[[i]]) > 1L) names(x[[i]])[1L] else ""
        max(nchar(values[[i]][1L]), nchar(label), na.rm = TRUE)
    }, 0))
    for (i in seq_along(x)) {
        value <- x[[i]]
        lead <- .lm_pad(labels[[i]], width, left = TRUE)
        formatted <- values[[i]]
        if (is.matrix(value)) {
            cat(lead, "\n", sep = "")
            print(value)
        } else if (length(value) == 1L) {
            lines <- c(lines, paste(lead, .lm_pad(formatted, first)))
        } else {
            heads <- nums <- ""
            for (j in seq_along(value)) {
                w <- if (j == 1L) first else max(nchar(names(value)[j]), nchar(formatted[j]))
                heads <- paste(heads, .lm_pad(names(value)[j], w))
                nums <- paste(nums, .lm_pad(formatted[j], w))
            }
            lines <- c(lines, paste0(lead, heads), paste0(.lm_pad("", width), nums))
        }
    }
    if (length(lines)) {
        w <- max(nchar(lines))
        cat(c(.lm_pad("", w), .lm_pad(lines, w, left = TRUE)), sep = "\n")
    }
    invisible(x)
}

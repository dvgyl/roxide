format.pval <- function(pv, digits = max(1L, getOption("digits") - 2L),
                        eps = 2.220446049250313e-16, na.form = "NA", ...) {
    ans <- rep(na.form, length(pv))
    ok <- !is.na(pv) & pv >= eps
    tiny <- !is.na(pv) & pv < eps
    scipen <- getOption("scipen", 0L)
    small <- ok & pv < 10^(-4-scipen)
    large <- ok & !small
    if (any(small)) ans[small] <- .Internal(C_htest_format(pv[small], digits, scipen))
    if (any(large)) ans[large] <- .Internal(C_htest_format(pv[large], digits, scipen))
    if (any(tiny)) {
        if (any(ok)) {
            eps.digits <- 1L
            short <- max(nchar(ans[ok])) < 6L
        } else {
            eps.digits <- max(1L,digits-2L)
            short <- digits <= 3L
        }
        ans[tiny] <- paste0(if (short) "<" else "< ",
                            .Internal(C_htest_format(eps, eps.digits, scipen)))
    }
    ans
}

.htest_print_estimates <- function(values, digits) {
    nums <- .Internal(C_htest_format(values, max(1L,digits), getOption("scipen", 0L)))
    labels <- names(values)
    if (is.null(labels)) {
        print(values, digits = digits)
        return(invisible(NULL))
    }
    width <- getOption("width", 80L)
    padding <- max(nchar(nums))
    names.line <- ""
    numbers.line <- ""
    count <- 0L
    for (i in seq_along(nums)) {
        field <- max(padding, nchar(labels[[i]]))
        if (count > 0L && count + field + 1L > width) {
            cat(names.line, "\n", numbers.line, "\n", sep="")
            names.line <- ""
            numbers.line <- ""
            count <- 0L
        }
        f <- paste0("%", field, "s ")
        names.line <- paste0(names.line, sprintf(f, labels[[i]]))
        numbers.line <- paste0(numbers.line, sprintf(f, nums[[i]]))
        count <- count + field + 1L
    }
    cat(names.line, "\n", numbers.line, "\n", sep="")
}
print.htest <- function(x, digits = getOption("digits"), prefix = "\t", ...) {
    fmt <- function(value, d) .Internal(C_htest_format(value, max(1L,d), getOption("scipen", 0L)))
    cat("\n", .Internal(C_htest_wrap(x$method, prefix, getOption("width", 80L))), "\n\n", sep = "")
    cat("data:  ", x$data.name, "\n", sep = "")
    fields <- c(paste(names(x$statistic), fmt(x$statistic, digits-2L), sep=" = "),
                paste(names(x$parameter), fmt(x$parameter, digits-2L), sep=" = "))
    fields <- fields[fields != " = "]
    p <- format.pval(x$p.value, digits = digits-3L)
    label <- if (substr(p, 1L, 1L) == "<") paste("p-value",p) else paste("p-value =",p)
    cat(paste(c(fields,label), collapse=", "), "\n", sep="")
    if (length(x$alternative)) {
        alt <- x$alternative
        if (length(x$null.value) == 1L) {
            alt <- paste("true", names(x$null.value), "is",
                         c(two.sided = "not equal to", less = "less than", greater = "greater than")[[alt]],
                         fmt(x$null.value, digits))
        }
        cat("alternative hypothesis: ", alt, "\n", sep="")
        if (length(x$null.value) > 1L) {
            cat("null values:\n")
            .htest_print_estimates(x$null.value, digits)
        }
    }
    if (!is.null(x$conf.int)) {
        cat(100*attr(x$conf.int, "conf.level"), " percent confidence interval:\n", sep="")
        interval <- fmt(x$conf.int, digits)
        cat(paste0(" ", sprintf(paste0("%", max(nchar(interval)), "s"), interval), collapse=""), "\n", sep="")
    }
    if (length(x$estimate)) {
        cat("sample estimates:\n")
        .htest_print_estimates(x$estimate, digits)
    }
    cat("\n")
    invisible(x)
}

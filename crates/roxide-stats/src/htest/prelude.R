.htest_choice <- function(x, choices) .Internal(C_optim_match_arg(x, choices))
.htest_alternatives <- c("two.sided", "less", "greater")
.htest_alt <- function(x) .htest_choice(x, .htest_alternatives)
.htest_tail <- function(alt, lower, upper) {
    if (alt == "less") lower else if (alt == "greater") upper else 2 * min(lower, upper)
}
.htest_alpha <- function(alt, level) (1 - level) / if (alt == "two.sided") 2 else 1
.htest_interval <- function(alt, low, high, level, minimum = -Inf, maximum = Inf) {
    if (alt == "less") low <- minimum
    if (alt == "greater") high <- maximum
    structure(c(low, high), conf.level = level)
}
.htest_level <- function(level, open = FALSE, message = "'conf.level' must be a single number between 0 and 1") {
    if (length(level) != 1L || !is.finite(level) || level < 0 || level > 1 ||
        (open && (level == 0 || level == 1))) stop(message)
}
.htest_mu <- function(mu) {
    if (length(mu) != 1L || !is.finite(mu)) stop("'mu' must be a single number")
}
.htest_make <- function(...) structure(list(...), class = "htest")
.htest_limit <- function(x) .Internal(C_htest_limit(x))
.htest_mean <- function(x) {
    n <- length(x)
    center <- sum(x)/n
    if (is.infinite(center) && all(is.finite(x))) center <- sum(x/n)
    if (is.finite(center)) center <- center + sum((x-center)/n)
    center
}
.htest_samples <- function(x, y, paired) {
    x <- .htest_limit(x)
    if (!is.null(y)) {
        y <- .htest_limit(y)
        if (paired) {
            keep <- complete.cases(x, y)
            return(list(x[keep] - y[keep], NULL))
        }
        y <- y[!is.na(y)]
    } else if (paired) stop("'y' is missing for paired test")
    list(x[!is.na(x)], y)
}

t.test <- function(x, ...) {
    if (missing(x)) UseMethod("t.test", eval(as.list(match.call())$formula, parent.frame()))
    else UseMethod("t.test")
}
t.test.default <- function(x, y = NULL, alternative = c("two.sided", "less", "greater"),
                           mu = 0, paired = FALSE, var.equal = FALSE, conf.level = 0.95, ...) {
    alternative <- .htest_alt(alternative)
    .htest_mu(mu)
    .htest_level(conf.level)
    label <- deparse1(substitute(x))
    if (!is.null(y)) label <- paste(label, "and", deparse1(substitute(y)))
    xy <- .htest_samples(x, y, paired)
    x <- xy[[1L]]
    y <- xy[[2L]]
    nx <- as.double(length(x))
    ny <- as.double(length(y))
    if (nx < 1 || (nx < 2 && (is.null(y) || !var.equal))) stop("not enough 'x' observations")
    if (!is.null(y) && (ny < 1 || (ny < 2 && !var.equal))) stop("not enough 'y' observations")
    if (!is.null(y) && nx + ny < 3) stop("not enough observations")
    mx <- .htest_mean(x)
    vx <- var(x)
    scale <- abs(mx)
    if (is.null(y)) {
        df <- nx - 1
        stderr <- sqrt(vx / nx)
        difference <- mx
        null <- if (paired) "mean difference" else "mean"
        estimate <- setNames(mx, if (paired) "mean difference" else "mean of x")
        method <- if (paired) "Paired t-test" else "One Sample t-test"
    } else {
        my <- .htest_mean(y)
        vy <- var(y)
        scale <- max(abs(mx), abs(my))
        difference <- mx - my
        null <- "difference in means"
        estimate <- c("mean of x" = mx, "mean of y" = my)
        if (var.equal) {
            df <- nx + ny - 2
            pooled <- 0
            if (nx > 1) pooled <- pooled + (nx - 1) * vx
            if (ny > 1) pooled <- pooled + (ny - 1) * vy
            stderr <- sqrt(pooled / df * (1/nx + 1/ny))
            method <- " Two Sample t-test"
        } else {
            a <- vx/nx
            b <- vy/ny
            stderr <- sqrt(a+b)
            df <- (a+b)^2 / (a^2/(nx-1) + b^2/(ny-1))
            method <- "Welch Two Sample t-test"
        }
    }
    if (is.finite(stderr) && stderr < 10 * 2.220446049250313e-16 * scale)
        stop("data are essentially constant")
    statistic <- (difference - mu) / stderr
    p <- .htest_tail(alternative, pt(statistic, df), pt(statistic, df, lower.tail = FALSE))
    width <- qt(.htest_alpha(alternative, conf.level), df, lower.tail = FALSE)
    ci <- .htest_interval(alternative, mu + (statistic-width)*stderr,
                          mu + (statistic+width)*stderr, conf.level)
    .htest_make(statistic = c(t = statistic), parameter = c(df = df), p.value = p,
                conf.int = ci, estimate = estimate, null.value = setNames(mu, null), stderr = stderr,
                alternative = alternative, method = method, data.name = label)
}

var.test <- function(x, ...) {
    if (missing(x)) UseMethod("var.test", eval(as.list(match.call())$formula, parent.frame()))
    else UseMethod("var.test")
}
var.test.default <- function(x, y, ratio = 1, alternative = c("two.sided", "less", "greater"),
                             conf.level = 0.95, ...) {
    alternative <- .htest_alt(alternative)
    if (length(ratio) != 1L || !is.finite(ratio) || ratio <= 0) stop("'ratio' must be a single positive number")
    .htest_level(conf.level)
    label <- paste(deparse1(substitute(x)), "and", deparse1(substitute(y)))
    x <- .htest_limit(x)
    y <- .htest_limit(y)
    if (inherits(x, "lm")) {
        dx <- x$df.residual
        vx <- sum(x$residuals^2)/dx
    } else {
        x <- x[is.finite(x)]
        dx <- length(x)-1L
        vx <- var(x)
    }
    if (inherits(y, "lm")) {
        dy <- y$df.residual
        vy <- sum(y$residuals^2)/dy
    } else {
        y <- y[is.finite(y)]
        dy <- length(y)-1L
        vy <- var(y)
    }
    if (dx < 1L) stop("not enough 'x' observations")
    if (dy < 1L) stop("not enough 'y' observations")
    vr <- vx/vy
    f <- vr/ratio
    p <- .htest_tail(alternative, pf(f, dx, dy), pf(f, dx, dy, lower.tail = FALSE))
    a <- .htest_alpha(alternative, conf.level)
    ci <- .htest_interval(alternative, vr/qf(a, dx, dy, lower.tail = FALSE),
                          vr/qf(a, dx, dy), conf.level, minimum = 0)
    .htest_make(statistic = c(F = f), parameter = c("num df" = dx, "denom df" = dy),
                p.value = p, conf.int = ci, estimate = c("ratio of variances" = vr),
                null.value = c("ratio of variances" = ratio), alternative = alternative,
                method = "F test to compare two variances", data.name = label)
}

shapiro.test <- function(x) {
    if (!is.numeric(x)) stop("is.numeric(x) is not TRUE")
    result <- .Internal(C_htest_shapiro(x))
    .htest_make(statistic = c(W = result[[1L]]), p.value = result[[2L]],
                method = "Shapiro-Wilk normality test", data.name = deparse1(substitute(x)))
}

wilcox.test <- function(x, ...) {
    if (missing(x)) UseMethod("wilcox.test", eval(as.list(match.call())$formula, parent.frame()))
    else UseMethod("wilcox.test")
}
wilcox.test.default <- function(x, y = NULL, alternative = c("two.sided", "less", "greater"),
                                mu = 0, paired = FALSE, exact = NULL, correct = TRUE,
                                conf.int = FALSE, conf.level = 0.95, tol.root = 1e-4,
                                digits.rank = Inf, ...) {
    alternative <- .htest_alt(alternative)
    .htest_mu(mu)
    if (conf.int) .htest_level(conf.level, TRUE)
    if (!is.numeric(x)) stop("'x' must be numeric")
    if (!is.null(y) && !is.numeric(y)) stop("'y' must be numeric")
    label <- deparse1(substitute(x))
    if (!is.null(y)) label <- paste(label, "and", deparse1(substitute(y)))
    xy <- .htest_samples(x, y, paired)
    x <- xy[[1L]]
    y <- xy[[2L]]
    if (!length(x)) stop("not enough (non-missing) 'x' observations")
    if (!is.null(y) && !length(y)) stop("not enough (non-missing) 'y' observations")
    w <- .Internal(C_htest_wilcox(x, y, .model_match(alternative, .htest_alternatives), mu,
                                  exact, correct, if (conf.int) conf.level else NULL, tol.root, digits.rank))
    signed <- is.null(y)
    method <- paste("Wilcoxon", if (signed) "signed rank" else "rank sum",
                    if (w[[3L]] == 1) "exact test" else "test")
    if (w[[3L]] == 0 && correct) method <- paste(method, "with continuity correction")
    result <- .htest_make(statistic = setNames(w[[1L]], if (signed) "V" else "W"),
                          parameter = NULL, p.value = w[[2L]],
                          null.value = setNames(mu, if (signed && !paired) "location" else "location shift"),
                          alternative = alternative, method = method, data.name = label)
    if (conf.int) {
        result$conf.int <- structure(w[4L:5L], conf.level = w[[7L]])
        result$estimate <- setNames(w[[6L]], if (!signed) "difference in location" else if (w[[7L]] == 0) "midrange" else "(pseudo)median")
    }
    result
}

cor.test <- function(x, ...) {
    if (missing(x)) UseMethod("cor.test", eval(as.list(match.call())$formula, parent.frame()))
    else UseMethod("cor.test")
}
cor.test.default <- function(x, y, alternative = c("two.sided", "less", "greater"),
                             method = c("pearson", "kendall", "spearman"), exact = NULL,
                             conf.level = 0.95, continuity = FALSE, ...) {
    alternative <- .htest_alt(alternative)
    method <- .htest_choice(method, c("pearson", "kendall", "spearman"))
    if (!is.numeric(x)) stop("'x' must be a numeric vector")
    if (!is.numeric(y)) stop("'y' must be a numeric vector")
    label <- paste(deparse1(substitute(x)), "and", deparse1(substitute(y)))
    x <- .htest_limit(x)
    y <- .htest_limit(y)
    if (length(x) != length(y)) stop("'x' and 'y' must have the same length")
    keep <- complete.cases(x, y)
    x <- x[keep]
    y <- y[keep]
    n <- length(x)
    if (n < if (method == "pearson") 3L else 2L) stop("not enough finite observations")
    ci <- NULL
    df <- NULL
    if (method == "pearson") {
        r <- cor(x, y)
        df <- c(df = n - 2L)
        t <- sqrt(df[[1L]]) * r / sqrt(1-r^2)
        p <- if (is.na(r)) r else .htest_tail(alternative, pt(t, df), pt(t, df, lower.tail = FALSE))
        stat <- c(t = t)
        label.method <- "Pearson's product-moment correlation"
        estimate <- c(cor = r)
        null <- c(correlation = 0)
        if (n > 3L) {
            .htest_level(conf.level)
            z <- .5 * log((1+r)/(1-r))
            width <- qnorm(.htest_alpha(alternative, conf.level), lower.tail = FALSE) / sqrt(n-3L)
            limits <- 2 * plogis(2 * c(z-width, z+width)) - 1
            ci <- .htest_interval(alternative, limits[[1L]], limits[[2L]], conf.level, -1, 1)
        }
    } else {
        k <- method == "kendall"
        a <- .Internal(C_htest_rank_cor(x, y, k, .model_match(alternative, .htest_alternatives), exact, continuity))
        stat <- setNames(a[[1L]], if (!k) "S" else if (a[[4L]] == 1) "T" else "z")
        p <- a[[2L]]
        label.method <- if (k) "Kendall's rank correlation tau" else "Spearman's rank correlation rho"
        estimate <- setNames(a[[3L]], if (k) "tau" else "rho")
        null <- setNames(0, names(estimate))
    }
    result <- .htest_make(statistic = stat, parameter = df, p.value = p, estimate = estimate,
                         null.value = null, alternative = alternative, method = label.method, data.name = label)
    if (!is.null(ci)) result$conf.int <- ci
    result
}

ks.test <- function(x, ...) {
    if (missing(x)) UseMethod("ks.test", eval(as.list(match.call())$formula, parent.frame()))
    else UseMethod("ks.test")
}
ks.test.default <- function(x, y, ..., alternative = c("two.sided", "less", "greater"),
                            exact = NULL, simulate.p.value = FALSE, B = 2000) {
    alternative <- .htest_alt(alternative)
    label <- deparse1(substitute(x))
    x <- .htest_limit(x)
    x <- x[!is.na(x)]
    if (!length(x)) stop("not enough 'x' data")
    two <- is.numeric(y)
    if (two) {
        label <- paste(label, "and", deparse1(substitute(y)))
        y <- .htest_limit(y)
        y <- y[!is.na(y)]
        if (!length(y)) stop("not enough 'y' data")
        values <- y
    } else {
        if (is.character(y)) y <- get(y, mode = "function", envir = parent.frame())
        if (!is.function(y)) stop("'y' must be numeric or a function or a string naming a valid function")
        x <- sort(x)
        values <- y(x, ...)
    }
    b <- if (two && simulate.p.value) B else NULL
    a <- .Internal(C_htest_ks(x, values, two, .model_match(alternative, .htest_alternatives), exact, b))
    mode <- if (a[[3L]] == 1) "Exact" else if (a[[3L]] >= 2) "Monte-Carlo" else "Asymptotic"
    method <- paste(mode, if (two) "two-sample" else "one-sample", "Kolmogorov-Smirnov test")
    comparison <- if (two) "that of y" else "the null hypothesis"
    alt <- if (alternative == "two.sided") "two-sided" else
        paste("the CDF of x lies", if (alternative == "less") "below" else "above", comparison)
    name <- if (alternative == "two.sided") "D" else if (alternative == "less") "D^-" else "D^+"
    structure(list(statistic = setNames(a[[1L]], name), p.value = a[[2L]], alternative = alt,
                   method = method, data.name = label, exact = a[[3L]] == 1 || a[[3L]] == 3), class = c("ks.test", "htest"))
}

p.adjust.methods <- c("holm", "hochberg", "hommel", "bonferroni", "BH", "BY", "fdr", "none")
p.adjust <- function(p, method = p.adjust.methods, n = length(p)) {
    method <- .htest_choice(method, p.adjust.methods)
    labels <- names(p)
    ans <- .Internal(C_htest_adjust(p, .model_match(method, p.adjust.methods), n))
    names(ans) <- labels
    ans
}

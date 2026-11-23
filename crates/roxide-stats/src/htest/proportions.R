binom.test <- function(x, n, p = 0.5, alternative = c("two.sided", "less", "greater"),
                       conf.level = 0.95) {
    alternative <- .htest_alt(alternative)
    .htest_level(conf.level, TRUE)
    if (length(p) != 1L || !is.finite(p) || p < 0 || p > 1)
        stop("'p' must be a single number between 0 and 1")
    label <- deparse1(substitute(x))
    x <- .htest_limit(x)
    if (any(!is.finite(x)) || any(x < 0) || any(abs(x-round(x)) > 1e-7))
        stop("'x' must be nonnegative and integer")
    x <- round(x)
    if (length(x) == 2L) {
        n <- sum(x)
        x <- x[[1L]]
    } else {
        if (length(x) != 1L) stop("incorrect length of 'x'")
        if (length(n) != 1L || !is.finite(n) || n < 1 || abs(n-round(n)) > 1e-7)
            stop("'n' must be a positive integer >= 'x'")
        label <- paste(label, "and", deparse1(substitute(n)))
        n <- round(n)
    }
    if (x > n) stop("'n' must be a positive integer >= 'x'")
    if (alternative == "less") {
        prob <- pbinom(x, n, p)
    } else if (alternative == "greater") {
        prob <- pbinom(x-1, n, p, lower.tail = FALSE)
    } else if (p == 0 || p == 1) {
        prob <- as.double(x == n*p)
    } else if (x == n*p) {
        prob <- 1
    } else {
        cutoff <- dbinom(x, n, p, log = TRUE) + log(1.0000001)
        left <- x < n*p
        low <- if (left) ceiling(n*p) else 0
        high <- if (left) n+1 else floor(n*p)+1
        if (n >= 2^52) stop("binomial test size limit exceeded")
        for (i in seq_len(53L)) {
            if (low >= high) break
            mid <- floor(low + (high-low)/2)
            small <- dbinom(mid, n, p, log = TRUE) <= cutoff
            if (small == left) high <- mid else low <- mid+1
        }
        prob <- if (left) pbinom(x,n,p) + pbinom(low-1,n,p,lower.tail=FALSE) else
            pbinom(low-1,n,p) + pbinom(x-1,n,p,lower.tail=FALSE)
        prob <- min(1, prob)
    }
    a <- .htest_alpha(alternative, conf.level)
    lo <- if (x == 0) 0 else qbeta(a, x, n-x+1)
    hi <- if (x == n) 1 else qbeta(a, x+1, n-x, lower.tail = FALSE)
    .htest_make(statistic = c("number of successes" = x), parameter = c("number of trials" = n),
                p.value = prob, conf.int = .htest_interval(alternative, lo, hi, conf.level, 0, 1),
                estimate = c("probability of success" = x/n), null.value = c("probability of success" = p),
                alternative = alternative, method = "Exact binomial test", data.name = label)
}

.htest_wilson <- function(p, n, z, side) {
    if (is.infinite(z)) return(if (side < 0) 0 else 1)
    a <- z^2/(2*n)
    (p + a + side*z*sqrt((p*(1-p)+a/2)/n))/(1+2*a)
}

prop.test <- function(x, n, p = NULL, alternative = c("two.sided", "less", "greater"),
                      conf.level = 0.95, correct = TRUE) {
    alternative <- .htest_alt(alternative)
    .htest_level(conf.level, TRUE)
    label <- deparse1(substitute(x))
    x <- .htest_limit(x)
    if (is.matrix(x)) {
        if (ncol(x) != 2L) stop("'x' must have 2 columns")
        n <- x[, 1L] + x[, 2L]
        x <- x[, 1L]
    } else {
        label <- paste(label, "out of", deparse1(substitute(n)))
    }
    n <- .htest_limit(n)
    if (length(x) != length(n)) stop("'x' and 'n' must have the same length")
    given <- !is.null(p)
    if (given && length(p) != length(x)) stop("'p' must have the same length as 'x' and 'n'")
    p.label <- deparse1(substitute(p))
    if (given) {
        keep <- complete.cases(x, n, p)
        p <- p[keep]
        if (any(p <= 0 | p >= 1)) stop("elements of 'p' must be in (0,1)")
    } else keep <- complete.cases(x, n)
    x <- x[keep]
    n <- n[keep]
    k <- length(x)
    if (!k) stop("not enough data")
    if (any(n <= 0)) stop("elements of 'n' must be positive")
    if (any(x < 0)) stop("elements of 'x' must be nonnegative")
    if (any(x > n)) stop("elements of 'x' must not be greater than those of 'n'")
    if (k == 1L && !given) p <- 0.5
    if (k == 1L) label <- paste0(label, ", null probability ", p)
    if (given && k > 1L) label <- paste0(label, ", null probabilities ", p.label)
    if (k > 2L || (given && k > 1L)) alternative <- "two.sided"
    estimate <- x/n
    difference <- if (k == 1L) estimate-p else if (k == 2L) estimate[[1L]]-estimate[[2L]] else 0
    correction <- 0
    if (correct && k <= 2L) {
        correction <- 0.5
        if (k == 1L) correction <- min(correction, abs(difference)*n)
        if (k == 2L && !given) correction <- min(correction, abs(difference)/sum(1/n))
    }
    ci <- NULL
    z <- qnorm(.htest_alpha(alternative, conf.level), lower.tail = FALSE)
    if (k == 1L) {
        lo <- .htest_wilson(max(0, estimate-correction/n), n, z, -1)
        hi <- .htest_wilson(min(1, estimate+correction/n), n, z, 1)
        ci <- .htest_interval(alternative, max(0,lo), min(1,hi), conf.level, 0, 1)
    } else if (k == 2L && !given) {
        margin <- z*sqrt(sum(estimate*(1-estimate)/n)) + correction*sum(1/n)
        if (is.infinite(z)) margin <- Inf
        ci <- .htest_interval(alternative, max(-1,difference-margin), min(1,difference+margin), conf.level, -1, 1)
    }
    df <- if (is.null(p)) as.double(k) - 1 else k
    method <- if (k == 1L) "1-sample proportions test" else
        paste0(k, "-sample test for ", if (given) "given proportions" else "equality of proportions")
    method <- paste(method, if (correction > 0) "with" else "without", "continuity correction")
    labels <- if (k == 1L) "p" else paste("prop", seq_along(keep)[keep])
    null <- if (is.null(p)) NULL else setNames(p, labels)
    expected <- n * if (is.null(p)) sum(x)/sum(n) else p
    failures <- (n-expected)
    if (any(c(expected, failures) < 5)) warning("Chi-squared approximation may be incorrect")
    stat <- sum((abs(x-expected)-correction)^2/expected +
                (abs(n-x-failures)-correction)^2/failures)
    prob <- pchisq(stat, df, lower.tail = FALSE)
    if (alternative != "two.sided") {
        signed <- sign(difference)*sqrt(stat)
        prob <- pnorm(signed, lower.tail = alternative == "less")
    }
    .htest_make(statistic = c("X-squared" = stat), parameter = c(df = df), p.value = prob,
                estimate = setNames(estimate, labels), null.value = null, conf.int = ci,
                alternative = alternative, method = method, data.name = label)
}

chisq.test <- function(x, y = NULL, correct = TRUE, p = rep(1/length(x), length(x)),
                       rescale.p = FALSE, simulate.p.value = FALSE, B = 2000) {
    label <- deparse1(substitute(x))
    if (is.data.frame(x)) x <- do.call(cbind, as.list(x))
    x <- .htest_limit(x)
    if (is.matrix(x) && min(dim(x)) == 1L) x <- c(x)
    if (!is.matrix(x) && !is.null(y)) {
        if (length(x) != length(y)) stop("'x' and 'y' must have the same length")
        yn <- deparse1(substitute(y))
        keep <- complete.cases(x, y)
        x <- .Internal(C_htest_table(factor(x[keep]), factor(y[keep])))
        dn <- dimnames(x)
        names(dn) <- c(label, yn)
        dimnames(x) <- dn
        label <- paste(label, "and", yn)
    }
    if (any(is.na(x)) || any(x < 0)) stop("all entries of 'x' must be nonnegative and finite")
    if (sum(x) == 0) stop("at least one entry of 'x' must be positive")
    matrix <- is.matrix(x)
    if (!matrix) {
        if (length(x) < 2L) stop("'x' must at least have 2 elements")
        p <- .htest_limit(p)
        if (length(p) != length(x)) stop("'x' and 'p' must have the same number of elements")
        if (any(p < 0)) stop("probabilities must be non-negative.")
        if (abs(sum(p)-1) > sqrt(2.220446049250313e-16)) {
            if (!rescale.p) stop("probabilities must sum to 1.")
            p <- p/sum(p)
        }
    }
    b <- if (simulate.p.value) B else NULL
    h <- .Internal(C_htest_chisq(x, if (matrix) NULL else p, correct, b))
    method <- if (matrix) "Pearson's Chi-squared test" else "Chi-squared test for given probabilities"
    if (h[[4L]] == 1) method <- paste(method, "with Yates' continuity correction")
    if (h[[4L]] == 2) method <- paste0(method, " with simulated p-value\n\t (based on ", B, " replicates)")
    expected <- h[[5L]]
    resid <- (x-expected)/sqrt(expected)
    std <- (x-expected)/sqrt(h[[6L]])
    .htest_make(statistic = c("X-squared" = h[[1L]]), parameter = c(df = h[[2L]]),
                p.value = h[[3L]], method = method, data.name = label, observed = x,
                expected = expected, residuals = resid, stdres = std)
}

fisher.test <- function(x, y = NULL, workspace = 2e5, hybrid = FALSE,
                        hybridPars = c(expect = 5, percent = 80, Emin = 1), control = list(),
                        or = 1, alternative = "two.sided", conf.int = TRUE, conf.level = 0.95,
                        simulate.p.value = FALSE, B = 2000) {
    alternative <- .htest_alt(alternative)
    label <- deparse1(substitute(x))
    if (is.data.frame(x)) x <- do.call(cbind, as.list(x))
    if (is.matrix(x)) {
        if (any(dim(x) < 2L)) stop("'x' must have at least 2 rows and columns")
        x <- .htest_limit(x)
        if (!is.numeric(x) || any(!is.finite(x)) || any(x < 0))
            stop("all entries of 'x' must be nonnegative and finite")
        if (any(x > 2147483647)) stop("'x' has entries too large to be integer")
        rounded <- round(x)
        changed <- x != rounded
        if (any(changed))
            warning(paste0("'x' has been rounded to integer: Mean relative difference: ",
                           .Internal(C_htest_format(sum(abs(x[changed]-rounded[changed]))/sum(abs(x[changed])), 7, 0))))
        x <- rounded
    } else {
        if (is.null(y)) stop("if 'x' is not a matrix, 'y' must be given")
        if (length(x) != length(y)) stop("'x' and 'y' must have the same length")
        keep <- complete.cases(x, y)
        x <- .Internal(C_htest_table(factor(x[keep]), factor(y[keep])))
        label <- paste(label, "and", deparse1(substitute(y)))
    }
    two <- all(dim(x) == 2L)
    if (two) {
        if (length(or) != 1L || is.na(or) || or < 0) stop("'or' must be a single number between 0 and Inf")
        if (conf.int) .htest_level(conf.level, TRUE)
    }
    b <- if (!two && simulate.p.value) B else NULL
    hypp <- NULL
    if (two && hybrid) warning("'hybrid' is ignored for a 2 x 2 table")
    if (!two && is.null(b) && hybrid) {
        hypp <- as.double(hybridPars)
        stopifnot(length(hypp) == 3L, hypp[1] > 0, 0 <= hypp[2],
                  hypp[2] <= 100, hypp[3] >= 0)
    }
    h <- .Internal(C_htest_fisher(x, or, .model_match(alternative, .htest_alternatives),
                                  if (two && conf.int) conf.level else NULL, b, workspace, hypp))
    method <- "Fisher's Exact Test for Count Data"
    if (!is.null(b)) method <- paste0(method, " with simulated p-value\n\t (based on ", B, " replicates)")
    if (!is.null(hypp)) {
        method <- paste0(method, " hybrid using asym.chisq. iff (exp=", hypp[[1L]],
                         ", perc=", hypp[[2L]], ", Emin=", hypp[[3L]], ")")
    }
    if (two) {
        result <- .htest_make(p.value = h[[1L]], conf.int = if (conf.int) structure(h[3L:4L], conf.level = conf.level) else NULL,
                              estimate = c("odds ratio" = h[[2L]]), null.value = c("odds ratio" = or),
                              alternative = alternative, method = method, data.name = label)
        return(result)
    }
    .htest_make(p.value = h[[1L]], alternative = alternative, method = method, data.name = label)
}

.smooth_pi <- 3.141592653589793

bw.nrd0 <- function(x) {
    if (length(x) < 2L) stop("need at least 2 data points")
    hi <- sd(x)
    lo <- min(hi, diff(quantile(x, c(.25, .75), names = FALSE)) / 1.34)
    if (lo == 0) lo <- hi
    if (lo == 0) lo <- abs(x[1L])
    if (lo == 0) lo <- 1
    .9 * lo * length(x)^(-.2)
}

bw.nrd <- function(x) {
    if (length(x) < 2L) stop("need at least 2 data points")
    1.06 * min(sd(x), diff(quantile(x, c(.25, .75), names = FALSE)) / 1.34) *
        length(x)^(-.2)
}

.smooth_bw_data <- function(x, nb) {
    if (!is.numeric(x) || any(!is.finite(x))) stop("invalid bandwidth data")
    if (length(x) < 2L) stop("need at least 2 data points")
    if (length(nb) != 1L || !is.finite(nb) || nb <= 0) stop("invalid 'nb'")
    bin <- .Internal(C_smooth_pair_counts(x, nb))
    list(n = length(x), width = bin[[1L]], count = bin[[2L]])
}

.smooth_bw_cv <- function(x, nb, lower, upper, tol, biased) {
    bin <- .smooth_bw_data(x, nb)
    n <- bin$n
    kind <- if (biased) 2L else 1L
    criterion <- function(h) .Internal(C_smooth_bw_score(h, n, bin$width, bin$count, kind))
    h <- optimize(criterion, c(lower, upper), tol = tol)$minimum
    if (min(h - lower, upper - h) < tol)
        warning("minimum occurred at one end of the range")
    h
}

bw.ucv <- function(x, nb = 1000, lower = .1 * hmax, upper = hmax, tol = .1 * lower) {
    hmax <- 1.144 * sd(x) * length(x)^(-.2)
    .smooth_bw_cv(x, nb, lower, upper, tol, FALSE)
}

bw.bcv <- function(x, nb = 1000, lower = .1 * hmax, upper = hmax, tol = .1 * lower) {
    hmax <- 1.144 * sd(x) * length(x)^(-.2)
    .smooth_bw_cv(x, nb, lower, upper, tol, TRUE)
}

bw.SJ <- function(x, nb = 1000, lower = .1 * hmax, upper = hmax,
                  method = c("ste", "dpi"), tol = .1 * lower) {
    method <- .smooth_match(method, c("ste", "dpi"))
    bin <- .smooth_bw_data(x, nb)
    n <- bin$n
    scale <- min(sd(x), diff(quantile(x, c(.25, .75), names = FALSE)) / 1.349)
    hmax <- 1.144 * scale * n^(-.2)
    phi <- function(h, fourth) {
        kind <- if (fourth) 3L else 4L
        .Internal(C_smooth_bw_score(h, n, bin$width, bin$count, kind))
    }
    td <- -phi(1.23 * scale * n^(-1 / 9), FALSE)
    if (!is.finite(td) || td <= 0) stop("sample is too sparse to find TD")
    c1 <- 1 / (2 * sqrt(.smooth_pi) * n)
    if (method == "dpi") return((c1 / phi((2.394 / (n * td))^(1 / 7), TRUE))^.2)
    alpha <- 1.357 * (phi(1.24 * scale * n^(-1 / 7), TRUE) / td)^(1 / 7)
    eq <- function(h) (c1 / phi(alpha * h^(5 / 7), TRUE))^.2 - h
    expand <- missing(lower) && missing(upper)
    a <- lower
    b <- upper
    for (i in seq_len(100L)) {
        if (eq(a) * eq(b) <= 0) break
        if (!expand) break
        if (i %% 2L == 1L) b <- b * 1.2 else a <- a / 1.2
    }
    uniroot(eq, c(a, b), tol = tol)$root
}

density <- function(x, ...) UseMethod("density")

density.default <- function(x, bw = "nrd0", adjust = 1,
                            kernel = c("gaussian", "epanechnikov", "rectangular",
                                       "triangular", "biweight", "cosine", "optcosine"),
                            weights = NULL, window = kernel, width, give.Rkern = FALSE,
                            subdensity = FALSE, warnWbw = var(weights) > 0,
                            n = 512, from, to, cut = 3, ext = 4,
                            old.coords = FALSE, na.rm = FALSE, ...) {
    cl <- match.call()
    kernels <- c("gaussian", "epanechnikov", "rectangular", "triangular",
                 "biweight", "cosine", "optcosine")
    if (missing(kernel) && !missing(window)) kernel <- window
    kind <- .model_match(.smooth_match(kernel, kernels), kernels)
    if (give.Rkern) return(c(1 / (2 * sqrt(.smooth_pi)), 3 / (5 * sqrt(5)), 1 / (2 * sqrt(3)),
        2 / (3 * sqrt(6)), 5 / (7 * sqrt(7)), .75 * sqrt(1 / 3 - 2 / .smooth_pi^2),
        .smooth_pi^2 / 16 * sqrt(1 - 8 / .smooth_pi^2))[kind])
    if (!is.numeric(x)) stop("argument 'x' must be numeric")
    data.name <- paste(deparse(substitute(x)), collapse = " ")
    w <- !is.null(weights)
    if (w && length(weights) != length(x)) stop("'x' and 'weights' have unequal length")
    missing.x <- is.na(x)
    if (any(missing.x)) {
        if (!na.rm) stop("'x' contains missing values")
        if (w) {
            total <- sum(weights)
            weights <- weights[!missing.x]
            if (is.finite(total) && abs(total - 1) < 1.49e-8) weights <- weights / sum(weights)
        }
        x <- x[!missing.x]
    }
    nx <- length(x)
    finite <- is.finite(x)
    x <- x[finite]
    if (w) {
        if (any(!is.finite(weights))) stop("'weights' must all be finite")
        if (any(weights < 0)) stop("'weights' must not be negative")
        if (!subdensity && abs(sum(weights) - 1) >= 1.49e-8)
            warning("sum(weights) != 1  -- will not get true density")
        weights <- weights[finite]
    } else {
        weights <- rep(1 / nx, length(x))
    }
    if (!missing(width) && missing(bw)) {
        if (is.character(width)) bw <- width else
            bw <- width / c(4, 2 * sqrt(5), 2 * sqrt(3), 2 * sqrt(6),
                            2 * sqrt(7), 2 / sqrt(1 / 3 - 2 / .smooth_pi^2),
                            2 / sqrt(1 - 8 / .smooth_pi^2))[kind]
    }
    if (is.character(bw)) {
        if (length(x) < 2L)
            stop("need at least 2 points to select a bandwidth automatically")
        if (w && length(weights) > 1L && warnWbw)
            warning("Selecting bandwidth *not* using 'weights'")
        rule <- tolower(bw)
        selectors <- list(nrd0 = bw.nrd0, nrd = bw.nrd, ucv = bw.ucv, bcv = bw.bcv,
                          sj = bw.SJ, "sj-ste" = bw.SJ,
                          "sj-dpi" = function(x) bw.SJ(x, method = "dpi"))
        select <- selectors[[rule]]
        if (is.null(select)) stop("unknown bandwidth rule")
        bw <- select(x)
    }
    if (length(bw) != 1L || !is.finite(bw)) stop("non-finite 'bw'")
    bw <- adjust * bw
    if (!is.finite(bw) || bw <= 0) stop("'bw' is not positive.")
    if (missing(from)) from <- min(x) - cut * bw
    if (missing(to)) to <- max(x) + cut * bw
    if (!is.finite(from)) stop("non-finite 'from'")
    if (!is.finite(to)) stop("non-finite 'to'")
    if (!is.finite(n) || n < 1) stop("invalid 'n'")
    range <- c(from - ext * bw, to + ext * bw)
    grid <- seq.int(from, to, length.out = n)
    y <- .Internal(C_smooth_density(x, weights, bw, kind, n, range, grid, old.coords))
    structure(list(x = grid, y = y, bw = bw, n = as.integer(nx),
                   old.coords = old.coords, call = cl, data.name = data.name,
                   has.na = FALSE), class = "density")
}

print.density <- function(x, digits = NULL, ...) {
    cat("\nCall:\n\t", paste(deparse(x$call), collapse = " "), "\n\n", sep = "")
    cat("Data: ", x$data.name, " (", x$n, " obs.);\tBandwidth 'bw' = ",
        .smooth_format(x$bw, if (is.null(digits)) 4L else digits), "\n\n", sep = "")
    stats <- .smooth_summary(x$x)
    labels <- sprintf("%-7s:", names(stats))
    column <- function(values, label) {
        formatted <- .smooth_format(values, max(3L, getOption("digits") - 3L))
        width <- max(nchar(formatted))
        span <- 8L + width
        title <- paste0("       ", label, paste(rep(" ", span - 6L), collapse = ""))
        c(title, paste0(" ", labels, sprintf("%*s", width, formatted), " "))
    }
    cat(paste0(column(stats, "x"), " ", column(.smooth_summary(x$y), "y"), " "), sep = "\n")
    invisible(x)
}

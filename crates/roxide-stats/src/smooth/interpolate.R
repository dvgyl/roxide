.smooth_mean <- function(x) sum(x) / length(x)

.smooth_match <- function(value, choices) {
    .Internal(C_optim_match_arg(value, choices))
}

.smooth_xy <- function(x, y) {
    if (!is.null(y))
        return(list(x = as.double(x), y = as.double(y)))
    if (inherits(x, "formula")) {
        env <- environment(x)
        return(list(x = as.double(eval(x[[3L]], env)),
                    y = as.double(eval(x[[2L]], env))))
    }
    if (is.list(x)) {
        if (is.null(x$x) || is.null(x$y))
            stop("'x' is a list, but does not have components 'x' and 'y'")
        return(list(x = as.double(x$x), y = as.double(x$y)))
    }
    if (!is.null(dim(x)) && length(dim(x)) == 2L && ncol(x) >= 2L)
        return(list(x = as.double(x[, 1L]), y = as.double(x[, 2L])))
    if (is.complex(x))
        return(list(x = Re(x), y = Im(x)))
    list(x = as.double(seq_along(x)), y = as.double(x))
}

.smooth_knots <- function(x, y, ties, na.rm, warn) {
    xy <- .smooth_xy(x, y)
    x <- xy$x
    y <- xy$y
    if (length(x) != length(y)) stop("'x' and 'y' lengths differ")
    if (na.rm) {
        keep <- !is.na(x) & !is.na(y)
        x <- x[keep]
        y <- y[keep]
    } else if (any(is.na(x))) {
        stop("approx(x,y, .., na.rm=FALSE): NA values in x are not allowed")
    }
    if (identical(ties, "ordered")) return(list(x = x, y = y))
    if (is.list(ties)) {
        if (length(ties) != 2L || !identical(ties[[1L]], "ordered"))
            stop("invalid 'ties' argument")
        ties <- ties[[2L]]
    } else {
        ix <- order(x)
        x <- x[ix]
        y <- y[ix]
    }
    if (length(x) > 1L) {
        runs <- rle(x)$lengths
        if (length(runs) < length(x)) {
            if (warn) warning("collapsing to unique 'x' values")
            tie <- if (identical(ties, "mean")) .smooth_mean else .stats_optimizer_fun(ties, parent.frame())
            ends <- cumsum(runs)
            starts <- ends - runs + 1L
            z <- y[starts]
            for (i in seq_along(runs)[runs > 1L])
                z[i] <- tie(y[seq.int(starts[i], ends[i])])
            x <- x[starts]
            y <- z
        }
    }
    list(x = x, y = y)
}

.smooth_approx <- function(xy, method, yleft, yright, rule, f) {
    method <- .smooth_match(method, c("linear", "constant"))
    count <- length(xy$x)
    if (count < 2L && method == "linear")
        stop("need at least two non-NA values to interpolate")
    if (!count) stop("zero non-NA points")
    if (length(rule) == 1L) rule <- rep(rule, 2L)
    if (length(rule) != 2L || any(is.na(rule)))
        stop("'rule' must be a vector of length 1 or 2")
    if (is.null(yleft)) yleft <- if (rule[1L] == 1) NA_real_ else xy$y[1L]
    if (is.null(yright)) yright <- if (rule[2L] == 1) NA_real_ else xy$y[count]
    if (method == "linear") {
        f <- -1
    } else if (length(f) != 1L || is.na(f) || f < 0 || f > 1) {
        stop("invalid f value")
    }
    ends <- c(as.double(yleft)[1L], as.double(yright)[1L])
    force(xy)
    force(f)
    function(v) .Internal(C_smooth_approx(xy$x, xy$y, v, ends, f))
}

approx <- function(x, y = NULL, xout, method = "linear", n = 50,
                   yleft, yright, rule = 1, f = 0, ties = .smooth_mean, na.rm = TRUE) {
    xy <- .smooth_knots(x, y, ties, na.rm, missing(ties))
    fun <- .smooth_approx(xy, method, if (missing(yleft)) NULL else yleft,
                          if (missing(yright)) NULL else yright, rule, f)
    if (missing(xout)) {
        if (!is.finite(n) || n <= 0) stop("'n' must be at least one")
        xout <- seq.int(xy$x[1L], xy$x[length(xy$x)], length.out = n)
    }
    list(x = xout, y = fun(xout))
}

approxfun <- function(x, y = NULL, method = "linear", yleft, yright,
                      rule = 1, f = 0, ties = .smooth_mean, na.rm = TRUE) {
    xy <- .smooth_knots(x, y, ties, na.rm, missing(ties))
    .smooth_approx(xy, method, if (missing(yleft)) NULL else yleft,
                    if (missing(yright)) NULL else yright, rule, f)
}

.smooth_spline <- function(xy, method) {
    methods <- c("fmm", "periodic", "natural", "monoH.FC", "hyman")
    method <- .model_match(.smooth_match(method, methods), methods)
    x <- xy$x
    y <- xy$y
    n <- length(x)
    if (!n) stop("zero non-NA points")
    if (method == 2L && y[1L] != y[n]) {
        warning("spline: first and last y values differ - using y[1L] for both")
        y[n] <- y[1L]
    }
    if (method == 4L && n < 2L) stop("length(m) must be at least two")
    if (method == 5L && any(diff(y) > 0) && any(diff(y) < 0))
        stop("'y' must be increasing or decreasing")
    coef <- .Internal(C_smooth_cubic(x, y, method))
    function(x, deriv = 0L) {
        deriv <- as.integer(deriv)
        if (length(deriv) != 1L || is.na(deriv) || deriv < 0L || deriv > 3L)
            stop("'deriv' must be between 0 and 3")
        .Internal(C_smooth_spline(xy$x, y, coef[[1L]], coef[[2L]], coef[[3L]],
                                   c(method, deriv, as.double(x))))
    }
}

spline <- function(x, y = NULL, n = 3 * length(x), method = "fmm",
                   xmin = min(x), xmax = max(x), xout, ties = .smooth_mean) {
    xy <- .smooth_knots(x, y, ties, TRUE, FALSE)
    f <- .smooth_spline(xy, method)
    if (missing(xout)) {
        left <- if (missing(xmin)) min(xy$x) else xmin
        right <- if (missing(xmax)) max(xy$x) else xmax
        xout <- seq.int(left, right, length.out = n)
    }
    xout <- as.double(xout)
    list(x = xout, y = f(xout))
}

splinefun <- function(x, y = NULL, method = c("fmm", "periodic", "natural",
                                             "monoH.FC", "hyman"), ties = .smooth_mean) {
    xy <- .smooth_knots(x, y, ties, TRUE, FALSE)
    .smooth_spline(xy, method)
}

ecdf <- function(x) {
    sample <- sort(as.double(x[!is.na(x)]))
    n <- length(sample)
    if (!n) stop("'x' must have 1 or more non-missing values")
    counts <- rle(sample)$lengths
    index <- cumsum(counts)
    knots <- sample[index]
    Fn <- .smooth_approx(list(x = knots, y = index / n),
                         "constant", 0, 1, 2, 0)
    result <- function(v) Fn(v)
    structure(result, class = c("ecdf", "stepfun", "function"), call = sys.call())
}

knots <- function(Fn, ...) UseMethod("knots")
knots.stepfun <- function(Fn, ...) get("knots", envir = environment(Fn), inherits = FALSE)
knots.ecdf <- knots.stepfun

quantile.ecdf <- function(x, ...) {
    quantile(get("sample", envir = environment(x), inherits = FALSE), ...)
}

.smooth_summary <- function(x) {
    result <- c(quantile(x, c(0, .25, .5)), .smooth_mean(x), quantile(x, c(.75, 1)))
    setNames(result, c("Min.", "1st Qu.", "Median", "Mean", "3rd Qu.", "Max."))
}

.smooth_format <- function(x, digits) {
    .Internal(C_htest_format(x, digits, getOption("scipen", 0L)))
}

summary.ecdf <- function(object, ...) {
    x <- knots(object)
    structure(.smooth_summary(x), class = "summary.ecdf", header =
        sprintf("Empirical CDF:\t%3d unique values with summary\n", length(x)))
}

print.summary.ecdf <- function(x, digits = max(3L, getOption("digits") - 3L), ...) {
    cat(attr(x, "header"))
    formatted <- .smooth_format(x, digits)
    width <- max(nchar(formatted), nchar(names(x)))
    cat(paste(sprintf("%*s", width, names(x)), collapse = " "), " \n", sep = "")
    cat(paste(sprintf("%*s", width, formatted), collapse = " "), " \n", sep = "")
    invisible(x)
}

print.ecdf <- function(x, digits = getOption("digits") - 2L, ...) {
    values <- knots(x)
    n <- length(values)
    cat("Empirical CDF \nCall: ", paste(deparse(attr(x, "call")), collapse = " "), "\n", sep = "")
    format.values <- function(x) sprintf("%*s", max(6L, digits), .smooth_format(x, digits))
    prefix <- sprintf(" x[1:%d] = ", n)
    if (n <= 5L) {
        cat(prefix, paste(format.values(values), collapse = ", "), "\n", sep = "")
    } else {
        cat(prefix, paste(format.values(values[1:3]), collapse = ", "), ",  ..., ",
            paste(format.values(values[c(n - 1L, n)]), collapse = ", "), "\n", sep = "")
    }
    invisible(x)
}

lowess <- function(x, y = NULL, f = 2 / 3, iter = 3L, delta = .01 * diff(range(x))) {
    xy <- .smooth_xy(x, y)
    if (length(xy$x) != length(xy$y)) stop("'x' and 'y' lengths differ")
    keep <- !is.na(xy$x) & !is.na(xy$y)
    x <- xy$x[keep]
    y <- xy$y[keep]
    ix <- order(x)
    result <- .Internal(C_smooth_lowess(x[ix], y[ix], f, iter, delta))
    names(result) <- c("x", "y")
    result
}

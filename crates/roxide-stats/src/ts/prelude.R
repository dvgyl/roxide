fft <- function(z, inverse = FALSE) .Internal(C_ts_fft(z, inverse, FALSE))
mvfft <- function(z, inverse = FALSE) .Internal(C_ts_fft(z, inverse, TRUE))
nextn <- function(n, factors = c(2, 3, 5)) .Internal(C_ts_nextn(n, factors))
convolve <- function(x, y, conj = TRUE, type = c("circular", "open", "filter")) {
    mode <- .ts_choice(type, c("circular", "open", "filter"))
    if (mode == "circular" && length(x) == length(y)) {
        px <- tsp(x)
        py <- tsp(y)
        if (!is.null(px) && !is.null(py) && !identical(px, py)) {
            f <- px[3L]
            if (abs(f - py[3L]) > .ts_eps()) stop("not all series have the same frequency")
            offset <- (px[1L] - py[1L]) * f
            if (abs(offset - round(offset)) > .ts_eps()) stop("not all series have the same phase")
            first <- max(px[1L], py[1L])
            last <- min(px[2L], py[2L])
            if (first > last) {
                warning("non-intersecting series")
                return(if (is.complex(x) || is.complex(y)) complex() else numeric())
            }
            a <- window(fft(x), first, last)
            b <- window(fft(y), first, last)
            z <- fft(a * if (conj) Conj(b) else b, inverse = TRUE) / length(x)
            return(if (is.complex(x) || is.complex(y)) z else Re(z))
        }
    }
    .Internal(C_ts_convolve(x, y, conj, mode))
}

.ts_choice <- function(arg, choices) .Internal(C_optim_match_arg(arg, choices))
.ts_rows <- function(x) if (is.null(dim(x))) length(x) else dim(x)[1L]
.ts_cols <- function(x) if (is.matrix(x)) ncol(x) else 1L
.ts_eps <- function() {
    eps <- getOption("ts.eps")
    if (is.null(eps)) 1e-5 else eps
}
.ts_extent <- function(rows, cols = 1L, width = 8L) .Internal(C_ts_extent(rows, cols, width))
.ts_date <- function(x, f) {
    if (length(x) > 1L) x[1L] + (x[2L] - 1) / f else x
}
.ts_params <- function(x) {
    p <- tsp(x)
    if (is.null(p)) c(1, .ts_rows(x), 1) else p
}
.ts_slice <- function(x, i) if (is.matrix(x)) x[i, , drop = FALSE] else x[i]

tsp <- function(x) attr(x, "tsp", exact = TRUE)
`tsp<-` <- function(x, value) .Internal(C_ts_set_tsp(x, value))
is.ts <- function(x) inherits(x, "ts")
is.mts <- function(x) is.ts(x) && is.matrix(x) && inherits(x, "mts")
as.ts <- function(x, ...) UseMethod("as.ts")
as.ts.ts <- function(x, ...) x
as.ts.default <- function(x, ...) {
    p <- tsp(x)
    if (is.null(p)) ts(x) else ts(x, start = p[1L], end = p[2L], frequency = p[3L])
}

ts <- function(data = NA, start = 1, end = numeric(), frequency = 1,
               deltat = 1, ts.eps = .ts_eps(), class = if (nseries > 1)
                   c("mts", "ts", "matrix", "array") else "ts", names = NULL) {
    if (is.data.frame(data)) {
        labels <- names(data)
        rows <- nrow(data)
        cols <- ncol(data)
        .ts_extent(rows, cols)
        fields <- lapply(data, function(column) {
            if (is.character(column)) as.integer(factor(column))
            else if (is.logical(column) || is.factor(column)) as.integer(column)
            else column
        })
        data <- matrix(do.call(c, unname(fields)), rows, cols, dimnames = list(NULL, labels))
    }
    n <- .ts_rows(data)
    nseries <- .ts_cols(data)
    if (n == 0L) stop("'ts' object must have one or more observations")
    if (missing(frequency)) frequency <- 1 / deltat
    if (abs(frequency - round(frequency)) < ts.eps) frequency <- round(frequency)
    if (is.null(end)) stop("argument is of length zero")
    if (length(end) == 0L) {
        start <- .ts_date(start, frequency)
        end <- start + (n - 1) / frequency
    } else {
        end <- .ts_date(end, frequency)
        start <- if (missing(start)) end - (n - 1) / frequency else .ts_date(start, frequency)
    }
    if (length(start) != 1L || length(end) != 1L || length(frequency) != 1L ||
        !is.finite(start) || !is.finite(end) || !is.finite(frequency))
        stop("missing value where TRUE/FALSE needed")
    if (start > end) stop("'start' cannot be after 'end'")
    steps <- (end - start) * frequency
    if (abs(steps - round(steps)) > ts.eps)
        stop("'end' must be a whole number of cycles after 'start'")
    size <- .ts_extent(round(steps) + 1, nseries, if (is.complex(data)) 16L else 8L)
    if (size != n) data <- .ts_slice(data, (seq_len(size) - 1L) %% n + 1L)
    if (is.matrix(data)) {
        if (is.null(names)) names <- colnames(data)
        if (is.null(names) && !is.null(class)) names <- paste("Series", seq_len(nseries))
        dimnames(data) <- list(NULL, names)
    }
    if (frequency <= 0) stop("invalid time series parameters specified (0)")
    attr(data, "tsp") <- c(start, end, frequency)
    class(data) <- class
    data
}

start <- function(x, ...) UseMethod("start")
end <- function(x, ...) UseMethod("end")
frequency <- function(x, ...) UseMethod("frequency")
deltat <- function(x, ...) UseMethod("deltat")
time <- function(x, ...) UseMethod("time")
cycle <- function(x, ...) UseMethod("cycle")
.ts_parts <- function(t, f) {
    ticks <- t * f
    if (abs(f - round(f)) < .ts_eps() && abs(ticks - round(ticks)) < .ts_eps()) {
        ticks <- round(ticks)
        c(floor(ticks / f), ticks %% f + 1)
    } else t
}
start.default <- function(x, ...) {
    p <- .ts_params(x)
    .ts_parts(p[1L], p[3L])
}
end.default <- function(x, ...) {
    p <- .ts_params(x)
    .ts_parts(p[2L], p[3L])
}
frequency.default <- function(x, ...) .ts_params(x)[3L]
deltat.default <- function(x, ...) 1 / frequency(x)
.ts_clock <- function(data, x, p) {
    tsp(data) <- p
    if (is.ts(x)) class(data) <- "ts"
    data
}
time.default <- function(x, offset = 0, ts.eps = .ts_eps(), ...) {
    p <- .ts_params(x)
    n <- .ts_extent(.ts_rows(x))
    values <- p[1L] + (seq_len(n) - 1 + offset) / p[3L]
    .ts_clock(values, x, p)
}
cycle.default <- function(x, ...) {
    p <- .ts_params(x)
    n <- .ts_extent(.ts_rows(x))
    values <- (round(p[1L] * p[3L]) + seq_len(n) - 1) %% p[3L] + 1
    .ts_clock(values, x, p)
}

lag <- function(x, ...) UseMethod("lag")
lag.default <- function(x, k = 1, ...) {
    if (abs(k - round(k)) > .ts_eps()) warning("'k' is not an integer")
    p <- .ts_params(x)
    p[1:2] <- p[1:2] - round(k) / p[3L]
    tsp(x) <- p
    x
}

diff.ts <- function(x, lag = 1, differences = 1, ...) {
    if (length(lag) != 1L || length(differences) != 1L || lag < 1 || differences < 1)
        stop("bad value for 'lag' or 'differences'")
    k <- round(lag)
    d <- as.integer(differences)
    if (k * d >= .ts_rows(x)) return(.ts_slice(x, integer()))
    p <- .ts_params(x)
    cl <- class(x)
    for (i in seq_len(d)) {
        if (abs(lag - k) > .ts_eps()) warning("'k' is not an integer")
        n <- .ts_rows(x)
        x <- .ts_slice(x, seq.int(k + 1L, n)) - .ts_slice(x, seq_len(n - k))
    }
    if (is.matrix(x)) dimnames(x) <- list(NULL, colnames(x)) else names(x) <- NULL
    tsp(x) <- c(p[1L] + k * d / p[3L], p[2L], p[3L])
    class(x) <- cl
    x
}

window <- function(x, ...) UseMethod("window")
window.ts <- function(x, ...) window.default(x, ...)
window.default <- function(x, start = NULL, end = NULL, frequency = NULL,
                           deltat = NULL, extend = FALSE, ts.eps = .ts_eps(), ...) {
    p <- .ts_params(x)
    f <- p[3L]
    if (!is.null(deltat)) {
        if (!is.null(frequency) && abs(frequency * deltat - 1) > ts.eps)
            stop("'frequency' and 'deltat' are both not NULL and are inconsistent")
        if (is.null(frequency)) frequency <- 1 / deltat
    }
    step <- 1
    if (!is.null(frequency)) {
        ratio <- f / frequency
        if (!is.finite(ratio) || ratio < 1 - ts.eps || abs(ratio - round(ratio)) > ts.eps) {
            warning("'frequency' not changed")
        } else step <- round(ratio)
    }
    left <- if (is.null(start)) p[1L] else .ts_date(start, f)
    right <- if (is.null(end)) p[2L] else .ts_date(end, f)
    if (!extend) {
        if (left < p[1L] - ts.eps) {
            warning("'start' value not changed")
            left <- p[1L]
        }
        if (right > p[2L] + ts.eps) {
            warning("'end' value not changed")
            right <- p[2L]
        }
    }
    if (left > right + ts.eps) stop("'start' cannot be after 'end'")
    first <- ceiling((left - p[1L]) * f - ts.eps)
    last <- floor((right - p[1L]) * f + ts.eps)
    if (first > last) stop("wrong sign in 'by' argument")
    n <- .ts_extent(floor((last - first) / step) + 1, .ts_cols(x), if (is.complex(x)) 16L else 8L)
    if (first == 0 && n == .ts_rows(x) && step == 1) {
        ans <- x
    } else {
        i <- first + (seq_len(n) - 1L) * step + 1L
        i[i < 1 | i > .ts_rows(x)] <- NA
        ans <- .ts_slice(x, i)
        if (is.matrix(ans) && any(is.na(i))) dimnames(ans) <- list(NULL, colnames(x))
        if (!is.matrix(ans) && !is.null(names(ans)) && any(is.na(i))) {
            labels <- names(ans)
            labels[is.na(i)] <- ""
            names(ans) <- labels
        }
    }
    tsp(ans) <- c(p[1L] + first / f, p[1L] + (first + (n - 1L) * step) / f, f / step)
    if (is.ts(x)) class(ans) <- class(x)
    ans
}

print.ts <- function(x, calendar, ...) {
    p <- .ts_params(x)
    f <- p[3L]
    series <- x
    tsp(series) <- NULL
    first <- .ts_parts(p[1L], f)
    last <- .ts_parts(p[2L], f)
    if (missing(calendar)) calendar <- f == 4 || f == 12
    if (calendar && length(first) == 2L && f > 1 && f == round(f)) {
        months <- c("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")
        if (is.matrix(x)) {
            index <- first[2L] - 1L + seq_len(nrow(x)) - 1L
            year <- first[1L] + floor(index / f)
            period <- index %% f + 1L
            labels <- if (f == 4) paste(year, paste0("Q", period)) else if (f == 12)
                paste(months[period], year) else paste(year, period)
            dimnames(series) <- list(labels, colnames(x))
            print.default(series, ...)
        } else {
            prefix <- first[2L] - 1
            n <- ceiling((prefix + length(series)) / f)
            .ts_extent(n, f)
            cells <- rep("", n * f)
            cells[prefix + seq_along(series)] <- .ts_format(series, ...)
            labels <- if (f == 4) paste0("Qtr", 1:4) else if (f == 12) months else paste0("p", seq_len(f))
            cells <- matrix(cells, nrow = n, byrow = TRUE)
            years <- as.character(first[1L] + seq_len(n) - 1L)
            rows <- c("", years)
            lines <- sprintf("%*s", max(nchar(years)), rows)
            for (j in seq_len(f)) {
                col <- c(labels[j], cells[, j])
                lines <- paste(lines, sprintf("%*s", max(nchar(col)), col))
            }
            cat(lines, sep = "\n")
        }
        return(invisible(x))
    }
    if (f == 1) {
        first <- p[1L]
        last <- p[2L]
    }
    cat("Time Series:\nStart =", paste(deparse(first), collapse = " "), "\nEnd =",
        paste(deparse(last), collapse = " "), "\nFrequency =", f, "\n")
    if (is.matrix(series)) dimnames(series) <- list(.ts_format(as.double(time(x))), colnames(series))
    print.default(series, ...)
    invisible(x)
}
.ts_format <- function(x, digits = getOption("digits"), ...) .Internal(C_ts_format(x, digits))

filter <- function(x, filter, method = c("convolution", "recursive"),
                   sides = 2L, circular = FALSE, init = NULL) {
    x <- as.ts(x)
    method <- .ts_choice(method, c("convolution", "recursive"))
    ans <- .Internal(C_ts_filter(x, as.double(filter), method == "recursive", sides, circular, init))
    tsp(ans) <- tsp(x)
    class(ans) <- if (.ts_cols(x) > 1L) c("mts", "ts") else "ts"
    ans
}

na.omit.ts <- function(object, ...) {
    keep <- complete.cases(object)
    if (!any(keep)) stop("all times contain an NA")
    selected <- seq_along(keep)[keep]
    first <- selected[1L]
    last <- selected[length(selected)]
    if (last - first + 1L != length(selected)) stop("time series contains internal NAs")
    if (all(keep)) return(object)
    p <- .ts_params(object)
    ans <- .ts_slice(object, selected)
    tsp(ans) <- p + c((first - 1L) / p[3L], (last - length(keep)) / p[3L], 0)
    class(ans) <- class(object)
    ans
}

.ts_acf <- function(x, lag.max, type, demean, label) {
    if (!is.numeric(x)) stop("'x' must be numeric")
    n <- .ts_rows(x)
    cols <- .ts_cols(x)
    if (is.null(lag.max)) lag.max <- floor(10 * log(n / cols, 10))
    k <- min(as.integer(lag.max), n - 1L)
    if (is.na(k) || k < 0L) stop("'lag.max' must be at least 0")
    if (type == "partial" && k < 1L) stop("'lag.max' must be at least 1")
    ans <- .Internal(C_ts_acf(x, k, type == "correlation", demean, frequency(x)))
    val <- ans[[1L]]
    lag <- ans[[2L]]
    labels <- if (is.matrix(x)) colnames(x) else NULL
    if (type == "partial") {
        val <- .Internal(C_ts_pacf(val, k, cols))
        lag <- lag[-1L, , , drop = FALSE]
        if (cols > 1L) dimnames(val) <- list(as.character(seq_len(k)), labels, labels)
        else labels <- NULL
    }
    structure(list(acf = val, type = type, n.used = as.integer(n),
                   lag = lag, series = label, snames = labels), class = "acf")
}
acf <- function(x, lag.max = NULL, type = c("correlation", "covariance", "partial"),
                plot = TRUE, na.action = na.fail, demean = TRUE, ...) {
    label <- paste(deparse(substitute(x)), collapse = " ")
    type <- .ts_choice(type, c("correlation", "covariance", "partial"))
    ans <- .ts_acf(na.action(as.ts(x)), lag.max, type, demean, label)
    if (plot) invisible(ans) else ans
}
pacf <- function(x, lag.max, plot, ...) UseMethod("pacf")
pacf.default <- function(x, lag.max = NULL, plot = TRUE, na.action = na.fail, ...) {
    label <- paste(deparse(substitute(x)), collapse = " ")
    ans <- .ts_acf(na.action(as.ts(x)), lag.max, "partial", TRUE, label)
    if (plot) invisible(ans) else ans
}
ccf <- function(x, y, lag.max = NULL, type = c("correlation", "covariance"),
                plot = TRUE, na.action = na.fail, ...) {
    label <- paste(paste(deparse(substitute(x)), collapse = " "),
                   paste(deparse(substitute(y)), collapse = " "), sep = " & ")
    type <- .ts_choice(type, c("correlation", "covariance"))
    x <- as.ts(x)
    y <- as.ts(y)
    if (.ts_cols(x) != 1L || .ts_cols(y) != 1L) stop("univariate time series only")
    px <- tsp(x)
    py <- tsp(y)
    if (abs(px[3L] - py[3L]) > .ts_eps()) stop("not all series have the same frequency")
    phase <- (px[1L] - py[1L]) * px[3L]
    if (abs(phase - round(phase)) > .ts_eps()) stop("not all series have the same phase")
    first <- max(px[1L], py[1L])
    last <- min(px[2L], py[2L])
    if (last < first) stop("no time series intersection")
    X <- cbind(as.double(window(x, first, last)), as.double(window(y, first, last)))
    X <- na.action(ts(X, start = first, frequency = px[3L]))
    ans <- .ts_acf(X, lag.max, type, TRUE, "X")
    values <- c(rev(ans$acf[-1L, 2L, 1L]), ans$acf[, 1L, 2L])
    lags <- c(rev(ans$lag[-1L, 2L, 1L]), ans$lag[, 1L, 2L])
    dim(values) <- dim(lags) <- c(length(values), 1L, 1L)
    ans$acf <- values
    ans$lag <- lags
    ans$snames <- label
    if (plot) invisible(ans) else ans
}

decompose <- function(x, type = c("additive", "multiplicative"), filter = NULL) {
    type <- .ts_choice(type, c("additive", "multiplicative"))
    x <- as.ts(x)
    p <- tsp(x)
    f <- p[3L]
    n <- length(x)
    if (f <= 1 || sum(!is.na(x)) < 2 * f)
        stop("time series has no or less than 2 periods")
    if (is.null(filter)) {
        filter <- if (f %% 2 == 0) c(0.5, rep(1, f - 1L), 0.5) / f else rep(1 / f, f)
    }
    trend <- stats::filter(x, filter)
    remove <- if (type == "additive") as.double(x) - as.double(trend) else as.double(x) / as.double(trend)
    figure <- numeric(f)
    for (i in seq_len(f)) {
        values <- remove[seq.int(i, n, by = f)]
        figure[i] <- sum(values, na.rm = TRUE) / sum(!is.na(values))
    }
    center <- sum(figure) / length(figure)
    figure <- if (type == "additive") figure - center else figure / center
    seasonal <- ts(figure[(seq_len(n) - 1L) %% length(figure) + 1L], start = p[1L], frequency = f)
    adjusted <- if (type == "additive") remove - as.double(seasonal) else remove / as.double(seasonal)
    if (is.matrix(x)) {
        prefix <- if (type == "additive") "x - seasonal.x." else "x/seasonal.x."
        adjusted <- matrix(adjusted, nrow(x), ncol(x), dimnames = list(NULL, paste0(prefix, colnames(x))))
    }
    tsp(adjusted) <- p
    class(adjusted) <- class(x)
    structure(list(x = x, seasonal = seasonal, trend = trend, random = adjusted,
                   figure = figure, type = type), class = "decomposed.ts")
}

Box.test <- function(x, lag = 1, type = c("Box-Pierce", "Ljung-Box"), fitdf = 0) {
    if (is.matrix(x)) stop("x is not a vector or univariate time series")
    label <- paste(deparse(substitute(x)), collapse = " ")
    type <- .ts_choice(type, c("Box-Pierce", "Ljung-Box"))
    r <- acf(x, lag.max = lag, plot = FALSE, na.action = na.pass)
    n <- sum(!is.na(x))
    k <- seq_len(.ts_extent(as.integer(lag)))
    value <- if (length(k) == 0L || lag >= dim(r$acf)[1L]) NA_real_ else {
        a <- r$acf[k + 1L, 1L, 1L]^2
        if (type == "Ljung-Box") n * (n + 2) * sum(a / (n - k)) else n * sum(a)
    }
    df <- lag - fitdf
    structure(list(statistic = c("X-squared" = value), parameter = c(df = df),
                   p.value = pchisq(value, df, lower.tail = FALSE),
                   method = if (type == "Ljung-Box") "Box-Ljung test" else "Box-Pierce test",
                   data.name = label), class = "htest")
}

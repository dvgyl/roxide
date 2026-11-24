.gd_indices <- function(x) seq_along(x)[!is.na(x) & x]

.gd_choice <- function(arg, choices) {
    if (identical(arg, choices) || is.null(arg)) return(choices[1L])
    if (!is.character(arg)) stop("'arg' must be NULL or a character vector")
    if (length(arg) != 1L) stop("'arg' must be of length 1")
    idx <- .gd_indices(choices == arg)
    if (!length(idx)) {
        hits <- startsWith(choices, arg)
        if (sum(hits, na.rm = TRUE) == 1L) return(choices[hits])
        stop(paste0("'arg' should be one of ", paste(paste0("“", choices, "”"), collapse = ", ")))
    }
    choices[idx]
}

colors <- function(distinct = FALSE) .Internal(C_gd_colors(distinct))
colours <- colors

col2rgb <- function(col, alpha = FALSE) {
    if (is.factor(col)) col <- as.character(col)
    .Internal(C_gd_col2rgb(col, alpha))
}

rgb <- function(red, green, blue, alpha, names = NULL, maxColorValue = 1) {
    if (missing(green) && (is.matrix(red) || is.data.frame(red))) {
        values <- as.matrix(red)
        red <- values[, 1L]
        green <- values[, 2L]
        blue <- values[, 3L]
    }
    if (missing(alpha)) alpha <- NULL
    if (!is.null(names)) names <- as.character(names)
    .Internal(C_gd_rgb(red, green, blue, alpha, names, maxColorValue))
}

hsv <- function(h = 1, s = 1, v = 1, alpha) {
    if (missing(alpha)) alpha <- NULL
    if (!is.null(alpha) && length(alpha) == 0L) return(character())
    .Internal(C_gd_hsv(h, s, v, alpha))
}

hcl <- function(h = 0, c = 35, l = 85, alpha, fixup = TRUE) {
    if (missing(alpha)) alpha <- NULL
    if (!is.null(alpha) && length(alpha) == 0L) return(character())
    .Internal(C_gd_hcl(h, c, l, alpha, fixup))
}

gray <- function(level, alpha) {
    if (missing(alpha)) alpha <- NULL
    if (length(level) > 0L && length(alpha) > length(level))
        stop(sprintf("attempt to set index %d/%d in SET_STRING_ELT", length(level), length(level)))
    .Internal(C_gd_gray(level, alpha))
}
grey <- gray

gray.colors <- function(n, start = 0.3, end = 0.9, gamma = 2.2, alpha, rev = FALSE) {
    if (missing(alpha)) alpha <- NULL
    values <- gray(seq.int(start^gamma, end^gamma, length.out = n)^(1/gamma),
                   alpha = if (length(alpha)) .gd_recycle(alpha, n) else alpha)
    if (rev) rev(values) else values
}
grey.colors <- gray.colors

adjustcolor <- function(col, alpha.f = 1, red.f = 1, green.f = 1, blue.f = 1,
                        offset = c(0, 0, 0, 0),
                        transform = diag(c(red.f, green.f, blue.f, alpha.f))) {
    if (!identical(dim(transform), c(4L, 4L))) stop("d == c(4L, 4L) are not all TRUE")
    if (length(offset) %% 4L != 0L) stop("length(offset)%%4L == 0L is not TRUE")
    channels <- col2rgb(col, alpha = TRUE)/255
    values <- transform %*% channels + matrix(offset, nrow = 4L, ncol = ncol(channels))
    values[.gd_indices(values < 0)] <- 0
    values[.gd_indices(values > 1)] <- 1
    dim(values) <- c(4L, length(col))
    rgb(values[1L, ], values[2L, ], values[3L, ], values[4L, ])
}

colorRamp <- function(colors, bias = 1, space = c("rgb", "Lab"),
                      interpolate = c("linear", "spline"), alpha = FALSE) {
    if (alpha && !missing(space)) stop("'alpha' must be false if 'space' is specified")
    space <- .gd_choice(space, c("rgb", "Lab"))
    interpolate <- .gd_choice(interpolate, c("linear", "spline"))
    if (bias <= 0) stop("'bias' must be positive")
    values <- t(col2rgb(colors, alpha = alpha))/255
    n <- nrow(values)
    if (n == 0L) stop("need at least two non-NA values to interpolate")
    if (n == 1L) values <- rbind(values, values)
    if (space == "Lab") values <- .Internal(C_gd_lab(values, TRUE))
    positions <- seq.int(0, 1, length.out = nrow(values))^bias
    make_curve <- if (interpolate == "linear") stats::approxfun else stats::splinefun
    red <- make_curve(positions, values[, 1L])
    green <- make_curve(positions, values[, 2L])
    blue <- make_curve(positions, values[, 3L])
    if (alpha) opacity <- make_curve(positions, values[, 4L])
    function(x) {
        if (!length(x)) return(matrix(numeric(), 0L, 4L, dimnames = list(NULL, NULL)))
        value <- if (alpha) cbind(red(x), green(x), blue(x), opacity(x))
                 else cbind(red(x), green(x), blue(x))
        if (space == "Lab") value <- .Internal(C_gd_lab(value, FALSE))
        value[.gd_indices(value < 0)] <- 0
        value[.gd_indices(value > 1)] <- 1
        dimnames(value) <- NULL
        value * 255
    }
}

colorRampPalette <- function(colors, ...) {
    ramp <- colorRamp(colors, ...)
    function(n) {
        if (n == 0L) return(character())
        values <- ramp(seq.int(0, 1, length.out = n))
        if (ncol(values) == 4L)
            rgb(values[, 1L], values[, 2L], values[, 3L], values[, 4L], maxColorValue = 255)
        else rgb(values, maxColorValue = 255)
    }
}

.gd_series <- function(start, end, n) .Internal(C_gd_series(start, end, n))
.gd_recycle <- function(x, n) rep(x, length.out = n)

rainbow <- function(n, s = 1, v = 1, start = 0, end = max(1, n - 1)/n,
                    alpha, rev = FALSE) {
    if (n <= 0L) return(character())
    if (any(c(start, end) < 0 | c(start, end) > 1) || start == end)
        stop("'start' and 'end' must be distinct and in [0, 1].")
    values <- if (missing(alpha) || is.null(alpha)) hsv(.gd_series(start, end, n), s, v)
              else hsv(.gd_series(start, end, n), s, v, .gd_recycle(alpha, n))
    if (rev) rev(values) else values
}

heat.colors <- function(n, alpha, rev = FALSE) {
    if (n <= 0L) return(character())
    tail <- n %/% 4L
    head <- n - tail
    h <- c(.gd_series(0, 1/6, head), rep(1/6, tail))
    s <- c(rep(1, head), if (tail) .gd_series(1 - .5/tail, .5/tail, tail))
    values <- if (missing(alpha) || is.null(alpha)) hsv(h, s) else hsv(h, s, alpha = .gd_recycle(alpha, n))
    if (rev) rev(values) else values
}

terrain.colors <- function(n, alpha, rev = FALSE) {
    if (n <= 0L) return(character())
    head <- n %/% 2L
    tail <- n - head
    h <- c(.gd_series(1/3, 1/6, head), .gd_series(1/6, 0, tail + 1L)[-1L])
    s <- c(rep(1, head), .gd_series(1, 0, tail + 1L)[-1L])
    v <- c(.gd_series(.65, .9, head), .gd_series(.9, .95, tail + 1L)[-1L])
    values <- if (missing(alpha) || is.null(alpha)) hsv(h, s, v) else hsv(h, s, v, .gd_recycle(alpha, n))
    if (rev) rev(values) else values
}

topo.colors <- function(n, alpha, rev = FALSE) {
    if (n <= 0L) return(character())
    mid <- n %/% 3L
    tail <- n %/% 3L
    head <- n - mid - tail
    h <- c(.gd_series(43/60, 31/60, head),
           .gd_series(23/60, 11/60, mid),
           .gd_series(1/6, .1, tail))
    s <- c(rep(1, head + mid), .gd_series(1, .3, tail))
    values <- if (missing(alpha) || is.null(alpha)) hsv(h, s) else hsv(h, s, alpha = .gd_recycle(alpha, n))
    if (rev) rev(values) else values
}

cm.colors <- function(n, alpha, rev = FALSE) {
    if (n <= 0L) return(character())
    if (n == 1L) {
        h <- .5
        s <- .5
    } else {
        k <- n %/% 2L
        h <- c(rep(.5, k), rep(5/6, n - k))
        s <- c(if (n %% 2L) .gd_series(.5, 0, k + 1L)[-(k + 1L)]
               else .gd_series(.5, .5/k, k),
               if (n %% 2L) .gd_series(0, .5, k + 1L)
               else .gd_series(0, .5, k + 1L)[-1L])
    }
    values <- if (missing(alpha) || is.null(alpha)) hsv(h, s) else hsv(h, s, alpha = .gd_recycle(alpha, n))
    if (rev) rev(values) else values
}

palette <- function(value) {
    if (missing(value)) return(.Internal(C_gd_palette(character())))
    old <- .Internal(C_gd_palette(value))
    if (length(value)) invisible(old) else old
}

palette.pals <- function() .Internal(C_gd_palette_names("palette"))

palette.colors <- function(n = NULL, palette = "Okabe-Ito", alpha, recycle = FALSE, names = FALSE) {
    values <- .Internal(C_gd_palette_colors(n, palette, recycle, names))
    if (!missing(alpha) && !is.null(alpha) && length(values)) {
        labels <- names(values)
        suffix <- sprintf("%02X", as.integer(floor(255 * alpha + .5)))
        values <- paste0(values, .gd_recycle(suffix, length(values)))
        names(values) <- labels
    }
    values
}

hcl.pals <- function(type = NULL) {
    type <- .gd_choice(type, c("all", "qualitative", "sequential", "diverging", "divergingx"))
    .Internal(C_gd_palette_names(type))
}

hcl.colors <- function(n, palette = "viridis", alpha = NULL, rev = FALSE, fixup = TRUE)
    .Internal(C_gd_hcl_colors(n, palette, alpha, rev, fixup))

dev.cur <- function() .Internal(C_gd_dev_cur())
dev.list <- function() .Internal(C_gd_dev_list())
dev.next <- function(which = dev.cur()) .Internal(C_gd_dev_step(which, TRUE))
dev.prev <- function(which = dev.cur()) .Internal(C_gd_dev_step(which, FALSE))
dev.set <- function(which = dev.next()) .Internal(C_gd_dev_set(which))
dev.off <- function(which = dev.cur()) {
    if (which == 1L) stop("cannot shut down device 1 (the null device)")
    .Internal(C_gd_dev_off(which))
}
dev.new <- function(..., noRStudioGD = FALSE) {
    device <- getOption("device")
    if (is.null(device)) device <- pdf
    if (is.character(device) && length(device) == 1L)
        device <- get(device, mode = "function", envir = parent.frame())
    if (!is.function(device)) stop("invalid 'device' option")
    invisible(device(...))
}
dev.size <- function(units = c("in", "cm", "px")) {
    units <- .gd_choice(units, c("in", "cm", "px"))
    scale <- if (units == "in") 1 else if (units == "cm") 2.54 else 0
    .Internal(C_gd_dev_size(scale))
}
graphics.off <- function() invisible(.Internal(C_gd_graphics_off()))

pretty <- function(x, ...) UseMethod("pretty")
pretty.default <- function(x, n = 5L, min.n = n %/% 3L, shrink.sml = 0.75,
                           high.u.bias = 1.5, u5.bias = .5 + 1.5 * high.u.bias,
                           eps.correct = 0L, f.min = 2^-20, bounds = TRUE, ...)
    .Internal(C_gd_pretty(x, n, min.n, shrink.sml, high.u.bias, u5.bias, eps.correct, f.min, bounds))

extendrange <- function(x, r = range(x, na.rm = TRUE), f = .05) {
    if (length(r) != 2L) stop("'r' must be a \"range\", hence of length 2")
    r + c(-1, 1) * .gd_recycle(f, 2L) * diff(r)
}

n2mfrow <- function(nr.plots, asp = 1) {
    if (asp == 1 && nr.plots <= 12L) {
        if (nr.plots <= 3L) return(c(as.integer(nr.plots), 1L))
        if (nr.plots <= 6L) return(c(as.integer((nr.plots + 1L) %/% 2L), 2L))
        if (nr.plots <= 9L) return(c(3L, 3L))
        return(c(4L, 3L))
    }
    rows <- ceiling(sqrt(nr.plots/asp))
    cols <- ceiling(nr.plots/rows)
    rows <- ceiling(nr.plots/cols)
    if (!is.finite(rows) || !is.finite(cols)) stop("missing value where TRUE/FALSE needed")
    c(rows, cols)
}

nclass.Sturges <- function(x) ceiling(1 + log(length(x), base = 2))

nclass.scott <- function(x) {
    width <- 3.5 * stats::sd(x)
    if (width > 0) ceiling(diff(range(x)) * length(x)^(1/3) / width) else 1L
}

nclass.FD <- function(x, digits = 5) {
    rounded <- signif(x, digits)
    width <- 2 * stats::IQR(rounded)
    if (width == 0) {
        tails <- 2^-(3:9)
        indices <- seq_along(tails)
        q <- stats::quantile(rounded, c(tails, 1 - tails), names = FALSE)
        widths <- (q[length(tails) + indices] - q[indices]) / (1 - 2 * tails)
        candidates <- .gd_indices(widths > 0)
        if (!length(candidates)) return(nclass.scott(x))
        width <- widths[candidates[1L]]
    }
    ceiling(diff(range(x)) * length(x)^(1/3) / width)
}

boxplot.stats <- function(x, coef = 1.5, do.conf = TRUE, do.out = TRUE) {
    if (coef < 0) stop("'coef' must not be negative")
    present <- !is.na(x)
    n <- sum(present)
    hinges <- stats::fivenum(x, na.rm = TRUE)
    if (n == 0L) hinges <- rep(NA, 5L)
    spread <- hinges[4L] - hinges[2L]
    if (coef == 0) do.out <- FALSE
    if (coef > 0) {
        outside <- if (is.na(spread)) !is.finite(x)
                   else x < hinges[2L] - coef * spread | x > hinges[4L] + coef * spread
        outside <- outside & present
        if (any(outside)) hinges[c(1L, 5L)] <- range(x[present & !outside])
    }
    conf <- if (do.conf) hinges[3L] + c(-1, 1) * 1.58 * spread / sqrt(n)
    out <- if (do.out) x[outside] else numeric()
    list(stats = hinges, n = n, conf = conf, out = out)
}

.gd_log_coords <- function(values, axis, log) {
    values <- as.double(values)
    if (length(log) && grepl(axis, log, fixed = TRUE)) {
        omit <- .gd_indices(values <= 0)
        n <- length(omit)
        if (n > 0L) {
            noun <- if (n == 1L) "value" else "values"
            warning(paste(n, axis, noun, "<= 0 omitted from logarithmic plot"))
            values[omit] <- NA_real_
        }
    }
    values
}

xy.coords <- function(x, y = NULL, xlab = NULL, ylab = NULL, log = NULL,
                      recycle = FALSE, setLab = TRUE) {
    if (is.null(y)) {
        if (inherits(x, "formula")) {
            if (length(x) != 3L) stop("invalid first argument")
            formula_env <- environment(x)
            if (is.null(formula_env)) formula_env <- parent.frame()
            if (setLab) {
                xlab <- deparse(x[[3L]])[1L]
                ylab <- deparse(x[[2L]])[1L]
            }
            y <- eval(x[[2L]], formula_env)
            x <- eval(x[[3L]], formula_env)
        } else if (is.complex(x)) {
            if (setLab) {
                label <- if (is.null(ylab)) "" else ylab
                xlab <- paste0("Re(", label, ")")
                ylab <- paste0("Im(", label, ")")
            }
            y <- Im(x)
            x <- Re(x)
        } else if ((is.matrix(x) || is.data.frame(x)) && ncol(x) >= 2L) {
            labels <- colnames(x)
            if (setLab) {
                if (is.null(labels)) labels <- paste0(if (is.null(ylab)) "" else ylab, c("[,1]", "[,2]"))
                xlab <- labels[1L]
                ylab <- labels[2L]
            }
            y <- x[, 2L]
            x <- x[, 1L]
        } else if (is.list(x) && !is.data.frame(x)) {
            if (!length(.gd_indices(names(x) == "x")) || !length(.gd_indices(names(x) == "y")))
                stop("'x' is a list, but does not have components 'x' and 'y'")
            if (setLab) {
                label <- if (is.null(ylab)) "" else ylab
                xlab <- paste0(label, "$x")
                ylab <- paste0(label, "$y")
            }
            y <- x$y
            x <- x$x
        } else {
            y <- if (is.data.frame(x)) x[[1L]] else x
            if (setLab && is.null(ylab)) ylab <- xlab
            if (inherits(x, "ts")) {
                x <- stats::time(x)
                if (setLab) xlab <- "Time"
            } else {
                x <- seq_along(y)
                if (setLab) xlab <- "Index"
            }
        }
    }
    if (length(x) != length(y)) {
        if (!recycle) stop("'x' and 'y' lengths differ")
        n <- max(length(x), length(y))
        x <- .gd_recycle(as.double(x), n)
        y <- .gd_recycle(as.double(y), n)
    }
    list(x = .gd_log_coords(x, "x", log), y = .gd_log_coords(y, "y", log),
         xlab = xlab, ylab = ylab)
}

xyz.coords <- function(x, y = NULL, z = NULL, xlab = NULL, ylab = NULL, zlab = NULL,
                       log = NULL, recycle = FALSE, setLab = TRUE) {
    if (is.null(z)) {
        if (!is.null(y)) {
            z <- y
            y <- x
            x <- seq_along(y)
            if (setLab) xlab <- "Index"
        } else if ((is.matrix(x) || is.data.frame(x)) && ncol(x) >= 2L) {
            labels <- colnames(x)
            if (ncol(x) >= 3L) {
                if (setLab) {
                    if (is.null(labels)) labels <- c("[,1]", "[,2]", "[,3]")
                    if (is.null(xlab)) xlab <- labels[1L]
                    if (is.null(ylab)) ylab <- labels[2L]
                    if (is.null(zlab)) zlab <- labels[3L]
                }
                z <- x[, 3L]
                y <- x[, 2L]
                x <- x[, 1L]
            } else {
                z <- x[, 2L]
                y <- x[, 1L]
                x <- seq_along(y)
                if (setLab) xlab <- "Index"
            }
        } else if (is.list(x) && !is.data.frame(x)) {
            if (!length(.gd_indices(names(x) == "x")) || !length(.gd_indices(names(x) == "y")) ||
                !length(.gd_indices(names(x) == "z")))
                stop("'x' is a list, but does not have components 'x', 'y'  and 'z'")
            if (setLab) {
                label <- if (is.null(xlab)) "" else xlab
                xlab <- paste0(label, "$x")
                ylab <- paste0(label, "$y")
                zlab <- paste0(label, "$z")
            }
            z <- x$z
            y <- x$y
            x <- x$x
        }
    }
    lengths <- c(length(x), length(y), length(z))
    n <- max(lengths)
    if (any(lengths != n)) {
        if (!recycle) stop("'x', 'y' and 'z' lengths differ")
        if (length(x)) x <- .gd_recycle(as.double(x), n)
        if (length(y)) y <- .gd_recycle(as.double(y), n)
        if (length(z)) z <- .gd_recycle(as.double(z), n)
    }
    list(x = .gd_log_coords(x, "x", log), y = .gd_log_coords(y, "y", log),
         z = .gd_log_coords(z, "z", log), xlab = xlab, ylab = ylab, zlab = zlab)
}

dev.control <- function(displaylist = c("inhibit","enable")) {
    invisible(.Internal(C_rx_dev_control(.gd_choice(displaylist, c("inhibit","enable")))))
}

recordPlot <- function(load = NULL, attach = NULL) {
    .Internal(C_rx_record_plot(as.character(load), as.character(attach)))
}

replayPlot <- function(x, reloadPkgs = FALSE) {
    if (reloadPkgs && inherits(x, "recordedplot")) {
        for (pkg in attr(x, "load")) loadNamespace(pkg)
        for (pkg in attr(x, "attach")) library(pkg, character.only = TRUE)
    }
    invisible(.Internal(C_rx_replay_plot(x)))
}

dev.copy <- function(device, ..., which = dev.next()) {
    source <- dev.cur()
    if (source == 1L) stop("no device to copy from")
    if (missing(device) && which == source) stop("cannot copy device to itself")
    plot <- recordPlot()
    if (missing(device)) {
        if (is.null(dev.list()) || !any(which == dev.list())) stop("cannot copy to the null device")
        dev.set(which)
    } else {
        if (!missing(which)) stop("cannot supply 'which' and 'device' at the same time")
        if (!is.function(device)) stop("'device' should be a function")
        device(...)
    }
    replayPlot(plot)
    dev.cur()
}

dev.print <- function(device = pdf, ..., width = dev.size()[1L], height = dev.size()[2L]) {
    source <- dev.cur()
    if (source == 1L) stop("no device to print from")
    if (!.Internal(C_rx_is_recording())) stop("can only print from a screen device")
    width <- width
    height <- height
    on.exit(dev.set(source))
    copy <- dev.copy(device = device, ..., width = width, height = height)
    dev.off(copy)
}

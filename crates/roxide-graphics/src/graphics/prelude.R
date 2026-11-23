par <- function(..., no.readonly = FALSE) {
    args <- list(...)
    all.params <- length(args) == 0L
    scalar <- FALSE
    if (length(args) > 1L && is.null(names(args)) && all(vapply(args, is.character, logical(1))))
        args <- as.list(do.call(c, args))
    if (length(args) == 1L && (is.null(names(args)) || names(args)[1L] == "")) {
        if (is.null(args[[1L]]) || is.list(args[[1L]])) {
            args <- as.list(args[[1L]])
        } else if (is.character(args[[1L]])) {
            scalar <- length(args[[1L]]) == 1L
            args <- as.list(args[[1L]])
        }
    }
    if (all.params) {
        if (no.readonly) return(.Internal(C_rx_par(NULL, TRUE, FALSE)))
        return(.Internal(C_rx_par(NULL, FALSE, FALSE)))
    }
    result <- .Internal(C_rx_par(args, FALSE, scalar))
    if (is.null(names(args))) result else invisible(result)
}

lcm <- function(x) paste(x, "cm")

layout <- function(mat, widths = rep(1, ncol(mat)),
                   heights = rep(1, nrow(mat)), respect = FALSE) {
    invisible(.Internal(C_rx_layout(mat, widths, heights, respect)))
}

layout.show <- function(n = 1L) {
    old <- par(mar = rep(0, 4L))
    on.exit(par(old))
    for (i in seq_len(n)) {
        plot.new()
        box()
        text(0.5, 0.5, i)
    }
    invisible(NULL)
}

plot.new <- function() invisible(.Internal(C_rx_plot_new()))
frame <- plot.new

plot.window <- function(xlim, ylim, log = "", asp = NA, ...) {
    invisible(.Internal(C_rx_plot_window(xlim, ylim, log, asp, list(...))))
}

grconvertX <- function(x, from = "user", to = "user") {
    .Internal(C_rx_grconvert(x, from, to, FALSE))
}

grconvertY <- function(y, from = "user", to = "user") {
    .Internal(C_rx_grconvert(y, from, to, TRUE))
}

strwidth <- function(s, units = "user", cex = NULL, font = NULL, vfont = NULL, ...) {
    .Internal(C_rx_strmetric(s, units, cex, font, FALSE))
}

strheight <- function(s, units = "user", cex = NULL, font = NULL, vfont = NULL, ...) {
    .Internal(C_rx_strmetric(s, units, cex, font, TRUE))
}

axTicks <- function(side, axp = NULL, usr = NULL, log = NULL, nintLog = NULL) {
    .Internal(C_rx_ax_ticks(side, axp, usr, log, nintLog))
}

.g_draw <- function(op, data, style) invisible(.Internal(C_rx_draw(op, data, style)))

plot.xy <- function(xy, type, pch = par("pch"), lty = par("lty"),
                    col = par("col"), bg = NA, cex = 1, lwd = par("lwd"), ...) {
    .g_draw("plot_xy", list(xy$x, xy$y, type),
            list(pch = pch, lty = lty, col = col, bg = bg, cex = cex, lwd = lwd, ...))
}

points <- function(x, ...) UseMethod("points")
points.default <- function(x, y = NULL, type = "p", ...) {
    plot.xy(xy.coords(x, y), type = type, ...)
}

lines <- function(x, ...) UseMethod("lines")
lines.default <- function(x, y = NULL, type = "l", ...) {
    plot.xy(xy.coords(x, y), type = type, ...)
}

segments <- function(x0, y0, x1 = x0, y1 = y0,
                     col = par("fg"), lty = par("lty"), lwd = par("lwd"), ...) {
    .g_draw("segments", list(x0, y0, x1, y1), list(col = col, lty = lty, lwd = lwd, ...))
}

arrows <- function(x0, y0, x1 = x0, y1 = y0, length = 0.25, angle = 30,
                   code = 2L, col = par("fg"), lty = par("lty"), lwd = par("lwd"), ...) {
    .g_draw("arrows", list(x0, y0, x1, y1, length, angle, code),
            list(col = col, lty = lty, lwd = lwd, ...))
}

rect <- function(xleft, ybottom, xright, ytop, density = NULL, angle = 45,
                 col = NA, border = NULL, lty = par("lty"), lwd = par("lwd"), ...) {
    .g_draw("rect", list(xleft, ybottom, xright, ytop, density, angle),
            list(col = col, border = if (identical(border, FALSE)) NA else border,
                 lty = lty, lwd = lwd, ...))
}

polygon <- function(x, y = NULL, density = NULL, angle = 45, border = NULL,
                    col = NA, lty = par("lty"), ..., fillOddEven = FALSE) {
    xy <- xy.coords(x, y, setLab = FALSE)
    .g_draw("polygon", list(xy$x, xy$y, density, angle, fillOddEven),
            list(col = col, border = if (identical(border, FALSE)) NA else border, lty = lty, ...))
}

.g_choice <- function(value, choices, arg) {
    if (length(value) == 1L && is.character(value) && !is.na(value)) {
        if (any(value == choices)) return(value)
        matches <- choices[substr(choices, 1L, nchar(value)) == value]
        if (length(matches) == 1L) return(matches)
    }
    stop(paste0("invalid '", arg, "' argument"))
}

polypath <- function(x, y = NULL, border = NULL, col = NA, lty = par("lty"), ...,
                     rule = c("winding", "evenodd")) {
    xy <- xy.coords(x, y, setLab = FALSE)
    if (missing(rule)) rule <- "winding"
    .g_draw("polypath", list(xy$x, xy$y, .g_choice(rule, c("winding", "evenodd"), "rule")),
            list(col = col, border = if (identical(border, FALSE)) NA else border, lty = lty, ...))
}

.g_limits <- function() {
    u <- par("usr")
    if (par("xlog")) u[1:2] <- 10^u[1:2]
    if (par("ylog")) u[3:4] <- 10^u[3:4]
    u
}

abline <- function(a = NULL, b = NULL, h = NULL, v = NULL, reg = NULL,
                    coef = NULL, untf = FALSE, ...) {
    if (!is.null(reg)) coef <- stats::coef(reg)
    if (is.null(coef) && !is.null(a) && !is.null(attr(a, "class"))) coef <- stats::coef(a)
    if (!is.null(coef)) { a <- coef[1L]; b <- coef[2L] }
    if (is.null(b) && length(a) == 2L) { b <- a[2L]; a <- a[1L] }
    u <- .g_limits()
    if (!is.null(a) && !is.null(b)) {
        x <- u[1:2]
        if (untf) {
            if (par("xlog")) x <- 10^seq(log10(x[1L]), log10(x[2L]), length.out = 101L)
            else x <- seq(x[1L], x[2L], length.out = 101L)
            y <- a + b * x
        } else {
            y <- a + b * par("usr")[1:2]
            if (par("ylog")) y <- 10^y
        }
        lines(x, y, ...)
    }
    if (length(h)) segments(u[1L], h, u[2L], h, ...)
    if (length(v)) segments(v, u[3L], v, u[4L], ...)
    invisible(NULL)
}

text <- function(x, ...) UseMethod("text")
text.default <- function(x, y = NULL, labels = seq_along(x), adj = NULL,
                         pos = NULL, offset = 0.5, vfont = NULL,
                         cex = 1, col = NULL, font = NULL, ...) {
    xy <- xy.coords(x, y, recycle = TRUE, setLab = FALSE)
    if (missing(labels)) labels <- seq_along(xy$x)
    .g_draw("text", list(xy$x, xy$y, labels, adj, pos, offset),
            list(cex = cex, col = col, font = font, ...))
}

mtext <- function(text, side = 3, line = 0, outer = FALSE, at = NA,
                   adj = NA, padj = NA, cex = NA, col = NA, font = NA, ...) {
    .g_draw("margin", list(side, line, at, adj, par("las"), text, outer),
            list(padj = padj, cex = if (all(is.na(cex))) NULL else cex,
                 col = if (all(is.na(col))) NULL else col,
                 font = if (all(is.na(font))) NULL else font, ...))
}

title <- function(main = NULL, sub = NULL, xlab = NULL, ylab = NULL,
                   line = NA, outer = FALSE, ...) {
    labels <- list(main, sub, xlab, ylab)
    cex <- c("cex.main", "cex.sub", "cex.lab", "cex.lab")
    col <- c("col.main", "col.sub", "col.lab", "col.lab")
    font <- c("font.main", "font.sub", "font.lab", "font.lab")
    lines <- c(par("mar")[3L] * 0.5, par("mar")[1L] - 1, rep(par("mgp")[1L], 2L))
    for (i in seq_along(labels)) {
        label <- labels[[i]]
        if (!is.null(label)) {
            style <- list(cex = par(cex[i]), col = par(col[i]), font = par(font[i]), ...)
            if (is.list(label)) {
                style <- c(style, label[-1L])
                label <- label[[1L]]
            }
            .g_draw("margin", list(c(3L,1L,1L,2L)[i], if (is.na(line)) lines[i] else line,
                                   NA, 0.5, 0, label, outer), style)
        }
    }
    invisible(NULL)
}

box <- function(which = "plot", lty = par("lty"), ...) {
    which <- .g_choice(which, c("plot", "figure", "inner", "outer"), "which")
    options <- list(lty = lty, ...)
    type <- options$bty
    options$bty <- NULL
    .g_draw("box", list(which, type), options)
}

clip <- function(x1, x2, y1, y2) .g_draw("clip", list(x1, x2, y1, y2), list())

.g_grid_ticks <- function(n, limits, side, log, equilogs) {
    if (is.null(n)) {
        at <- axTicks(side)
        if (log && equilogs) {
            powers <- log10(at)
            at <- at[abs(powers - round(powers)) < 1e-8]
        }
        return(at)
    }
    if (is.na(n) || n <= 0L) return(NULL)
    at <- seq(limits[1L], limits[2L], length.out = n + 1L)
    at <- at[-c(1L, length(at))]
    if (log) 10^at else at
}

grid <- function(nx = NULL, ny = nx, col = "lightgray", lty = "dotted",
                  lwd = par("lwd"), equilogs = TRUE) {
    usr <- par("usr")
    x <- .g_grid_ticks(nx, usr[1:2], 1L, par("xlog"), equilogs)
    y <- .g_grid_ticks(ny, usr[3:4], 2L, par("ylog"), equilogs)
    u <- .g_limits()
    if (length(x)) segments(x, u[3L], x, u[4L], col = col, lty = lty, lwd = lwd)
    if (length(y)) segments(u[1L], y, u[2L], y, col = col, lty = lty, lwd = lwd)
    invisible(list(atx = x, aty = y))
}

rug <- function(x, ticksize = 0.03, side = 1, lwd = 0.5, col = par("fg"), ...) {
    if (length(side) != 1L || !any(side == 1:4)) stop("invalid 'side' argument")
    x <- sort(as.double(x[is.finite(x)]))
    if (!length(x)) return(invisible(NULL))
    u <- .g_limits()
    horizontal <- any(side == c(1L,3L))
    limits <- if (horizontal) u[1:2] else u[3:4]
    if (any(x < min(limits) | x > max(limits))) warning("some values will be clipped")
    if (horizontal) {
        edge <- if (side == 1L) 0 else 1
        y <- grconvertY(c(edge, edge + (1 - 2*edge)*ticksize), "npc", "user")
        segments(x, y[1L], x, y[2L], lwd = lwd, col = col, ...)
    } else {
        edge <- if (side == 2L) 0 else 1
        at <- grconvertX(c(edge, edge + (1 - 2*edge)*ticksize), "npc", "user")
        segments(at[1L], x, at[2L], x, lwd = lwd, col = col, ...)
    }
    invisible(x)
}

xspline <- function(x, y = NULL, shape = 0, open = TRUE,
                    repEnds = TRUE, draw = TRUE, border = par("fg"), col = NA, ...) {
    xy <- xy.coords(x, y, setLab = FALSE)
    minimum <- if (!open) 3L else if (repEnds) 2L else 4L
    if (length(xy$x) < minimum) stop("too few points for xspline")
    if (!length(shape) || any(!is.finite(shape))) stop("invalid 'shape' value")
    if (any(shape != 0)) stop("nonzero xspline shape is not supported")
    if (open && !repEnds) {
        interior <- seq.int(2L, length(xy$x) - 1L)
        xy$x <- xy$x[interior]
        xy$y <- xy$y[interior]
    }
    if (draw) {
        if (open) lines(xy, col = border, ...) else polygon(xy, border = border, col = col, ...)
        invisible(NULL)
    } else {
        invisible(list(x = xy$x, y = xy$y))
    }
}

rasterImage <- function(image, xleft, ybottom, xright, ytop, angle = 0,
                         interpolate = TRUE, ...) {
    dims <- dim(image)
    mode <- "raster"
    if (inherits(image, "nativeRaster")) {
        mode <- "native_raster"
        pixels <- as.vector(image)
    } else if (length(dims) == 3L && any(dims[3L] == 3:4)) {
        alpha <- if (dims[3L] == 4L) image[,,4L] else 1
        pixels <- rgb(image[,,1L], image[,,2L], image[,,3L], alpha)
        dims <- dims[1:2]
    } else if (length(dims) == 2L) {
        pixels <- as.vector(image)
        if (is.numeric(pixels) || is.logical(pixels)) {
            missing.pixels <- is.na(pixels)
            pixels[missing.pixels] <- 0
            pixels <- gray(as.double(pixels))
            pixels[missing.pixels] <- NA_character_
        }
    } else {
        stop("'image' must be a matrix")
    }
    .g_draw(mode, list(xleft, ybottom, xright, ytop, dims, angle, pixels), list(...))
}

symbols <- function(x, y = NULL, circles, squares, rectangles, stars, thermometers,
                    boxplots, inches = TRUE, add = FALSE, fg = par("col"), bg = NA,
                    xlab = NULL, ylab = NULL, main = NULL, xlim = NULL, ylim = NULL,
                    ..., asp = NA) {
    xy <- xy.coords(x, y)
    if (!missing(circles)) {
        measures <- matrix(circles, ncol = 1L)
        shape <- "circles"
        allowed <- 1L
    } else if (!missing(squares)) {
        measures <- matrix(squares, ncol = 1L)
        shape <- "squares"
        allowed <- 1L
    } else if (!missing(rectangles)) {
        measures <- rectangles
        shape <- "rectangles"
        allowed <- 2L
    } else if (!missing(stars)) {
        measures <- stars
        measures[is.na(measures)] <- 0
        shape <- "stars"
        allowed <- seq.int(3L, ncol(measures))
    } else if (!missing(thermometers)) {
        measures <- thermometers
        shape <- "thermometers"
        allowed <- 3:4
    } else if (!missing(boxplots)) {
        measures <- boxplots
        shape <- "boxplots"
        allowed <- 5L
    } else stop("invalid symbol parameter")
    if (!is.matrix(measures) || nrow(measures) != length(xy$x) || !any(ncol(measures) == allowed))
        stop("invalid symbol parameter")
    if (shape == "stars" && ncol(measures) < 3L) stop("invalid symbol parameter")
    sizes <- if (shape == "boxplots") measures[,1:4] else
        if (shape == "thermometers") measures[,1:2] else measures
    if (any(sizes < 0, na.rm = TRUE)) stop("invalid symbol parameter")
    if (!add) {
        plot.new()
        plot.window(if (is.null(xlim)) range(xy$x, finite = TRUE) else xlim,
                    if (is.null(ylim)) range(xy$y, finite = TRUE) else ylim,
                    asp = asp, ...)
        box()
        title(main = main, xlab = xlab, ylab = ylab)
    }
    if (identical(inches, FALSE)) {
        scale <- c(abs(diff(grconvertX(c(0,1), "user", "device"))),
                   abs(diff(grconvertY(c(0,1), "user", "device"))))
    } else {
        largest <- max(c(0,sizes), na.rm = TRUE)
        if (any(shape == c("circles","stars"))) largest <- 2 * largest
        scale <- if (largest > 0) rep(72 * as.double(inches)[1L] / largest, 2L) else c(0,0)
    }
    .g_draw("symbols", list(xy$x, xy$y, as.vector(measures), scale, shape),
            list(col = fg, bg = bg, ...))
}

curve <- function(expr, from = NULL, to = NULL, n = 101, add = FALSE,
                   type = "l", xname = "x", xlab = xname, ylab = NULL,
                   log = NULL, xlim = NULL, ...) {
    e <- substitute(expr)
    if (is.null(ylab)) ylab <- paste(deparse(substitute(expr)), collapse = " ")
    if (n < 2L) stop("'n' must be at least 2")
    use.active <- isTRUE(add) || is.na(add)
    limits <- if (use.active) .g_limits()[1:2] else c(0,1)
    if (!is.null(xlim)) limits <- xlim
    if (is.null(from)) from <- limits[1L]
    if (is.null(to)) to <- limits[2L]
    if (is.null(log)) log <- if (use.active && par("xlog")) "x" else ""
    xs <- if (grepl("x", log, fixed = TRUE)) exp(seq(log(from), log(to), length.out = n)) else
        seq(from, to, length.out = n)
    env <- new.env(parent = parent.frame())
    assign(xname, xs, envir = env)
    if (typeof(e) == "symbol" && as.character(e) != xname) {
        fun <- eval(e, envir = parent.frame())
        if (!is.function(fun)) stop("'expr' must be a function or an expression in 'x'")
        ys <- fun(xs)
    } else {
        ys <- eval(e, envir = env)
        if (is.function(ys)) ys <- ys(xs)
    }
    if (length(ys) != length(xs)) stop("'expr' did not evaluate to an object of length 'n'")
    if (!isTRUE(add)) {
        plot.new()
        plot.window(if (is.null(xlim)) range(xs, finite = TRUE) else xlim,
                    range(ys, finite = TRUE), log = log)
        box()
        title(xlab = xlab, ylab = ylab)
    }
    plot.xy(list(x = xs, y = ys), type, ...)
    invisible(list(x = xs, y = ys))
}

locator <- function(n = 512, type = "n", ...) {
    if (length(n) != 1L || !is.finite(n) || n < 1) stop("invalid number of points in locator()")
    invisible(NULL)
}

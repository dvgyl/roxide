
matplot <- function(x, y, type = "p", lty = 1:5, lwd = 1, pch = NULL,
                     col = 1:6, cex = NULL, bg = NA, xlab = NULL, ylab = NULL,
                     xlim = NULL, ylim = NULL, log = "", add = FALSE,
                     verbose = getOption("verbose"), ...) {
    xl <- deparse(substitute(x))[1L]
    yl <- if (missing(y)) xl else deparse(substitute(y))[1L]
    if (missing(y)) { y <- x; x <- seq_len(if (is.null(dim(y))) length(y) else nrow(y)); xl <- "Index" }
    x <- as.matrix(x)
    y <- as.matrix(y)
    if (nrow(x) != nrow(y)) stop("'x' and 'y' must have same number of rows")
    n <- max(ncol(x), ncol(y))
    if (is.null(pch)) pch <- strsplit("123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0", "", fixed = TRUE)[[1L]]
    if (is.character(pch) && length(pch) == 1L) pch <- strsplit(pch, "", fixed = TRUE)[[1L]]
    if (is.null(cex)) cex <- 1
    x <- matrix(grDevices:::.gd_log_coords(x, "x", log), nrow(x))
    y <- matrix(grDevices:::.gd_log_coords(y, "y", log), nrow(y))
    if (!add) {
        if (is.null(xlim)) xlim <- .g_plot_range(x, "x")
        if (is.null(ylim)) ylim <- .g_plot_range(y, "y")
        plot.default(xlim, ylim, type = "n", log = log, xlim = xlim, ylim = ylim,
                     xlab = if (is.null(xlab)) xl else xlab,
                     ylab = if (is.null(ylab)) yl else ylab, ...)
    }
    type <- rep(type, length.out = n)
    lty <- rep(lty, length.out = n)
    lwd <- rep(lwd, length.out = n)
    pch <- rep(pch, length.out = n)
    col <- rep(col, length.out = n)
    cex <- rep(cex, length.out = n)
    bg <- rep(bg, length.out = n)
    for (i in seq_len(n))
        plot.xy(list(x = x[,(i-1L) %% ncol(x)+1L], y = y[,(i-1L) %% ncol(y)+1L]),
                type[i], pch = pch[i], lty = lty[i], lwd = lwd[i],
                col = col[i], cex = cex[i], bg = bg[i])
    invisible(NULL)
}

matpoints <- function(x, y, type = "p", ...) matplot(x, y, type = type, add = TRUE, ...)
matlines <- function(x, y, type = "l", ...) matplot(x, y, type = type, add = TRUE, ...)

pairs <- function(x, ...) UseMethod("pairs")
pairs.default <- function(x, labels = colnames(x), panel = points, ...,
                          lower.panel = panel, upper.panel = panel, diag.panel = NULL,
                          text.panel = text, gap = 1, row1attop = TRUE, main = NULL) {
    x <- as.matrix(x)
    n <- ncol(x)
    if (n < 2L) stop("only one column in the argument to 'pairs'")
    if (is.null(labels)) labels <- paste("var", seq_len(n))
    old <- par(mfrow = c(n,n), mar = rep(gap/2,4L),
               oma = c(2,2,if (is.null(main)) 2 else 4,2))
    on.exit(par(old))
    for (i in seq_len(n)) {
        row <- if (row1attop) i else n+1L-i
        for (j in seq_len(n)) {
            xi <- x[,j]
            yi <- x[,row]
            draw <- if (i > j) lower.panel else upper.panel
            plot.default(xi, yi, type = "n", axes = FALSE, ann = FALSE)
            if (row == j) {
                if (!is.null(diag.panel)) diag.panel(xi, ...)
                if (!is.null(text.panel)) text.panel(.g_mean(range(xi)), .g_mean(range(yi)), labels[j])
            } else if (!is.null(draw)) {
                draw(xi, yi, ...)
                if (i == n) axis(1L)
                if (j == 1L) axis(2L)
            }
            box()
        }
    }
    if (!is.null(main)) mtext(main, side = 3, outer = TRUE, line = 1, font = 2)
    invisible(NULL)
}

image <- function(x, ...) UseMethod("image")
image.default <- function(x = seq(0,1,length.out = nrow(z)),
                          y = seq(0,1,length.out = ncol(z)), z,
                          zlim = .g_plot_range(z, "z"), xlim = range(x), ylim = range(y),
                          col = hcl.colors(12, "YlOrRd", rev = TRUE), add = FALSE,
                          xaxs = "i", yaxs = "i", xlab = NULL, ylab = NULL,
                          breaks, oldstyle = FALSE, useRaster, ...) {
    if (missing(z) && !missing(x)) {
        if (is.list(x)) { z <- x$z; y <- x$y; x <- x$x }
        else if (is.matrix(x)) { z <- x; x <- seq(0,1,length.out = nrow(z)) }
    }
    if (!is.matrix(z) || !is.numeric(z)) stop("'z' must be a matrix")
    edges <- function(v, n, limits) {
        if (length(v) == n+1L) return(v)
        if (length(v) != n || any(!is.finite(v)) || any(diff(v) <= 0))
            stop("invalid image coordinates")
        if (n == 1L) return(if (diff(limits) == 0) limits + c(-.5,.5) else limits)
        c(v[1L] - diff(v)[1L]/2, v[-n] + diff(v)/2, v[n] + diff(v)[n-1L]/2)
    }
    ex <- edges(x, nrow(z), xlim)
    ey <- edges(y, ncol(z), ylim)
    if (missing(breaks)) {
        if (diff(zlim) == 0) zlim <- zlim + c(-.5,.5)
        breaks <- seq(zlim[1L], zlim[2L], length.out = length(col)+1L)
    }
    if (length(breaks) != length(col)+1L || any(diff(breaks) <= 0)) stop("invalid 'breaks'")
    if (!add) plot.default(xlim, ylim, type = "n", xaxs = xaxs, yaxs = yaxs,
                           xlim = xlim, ylim = ylim, xlab = xlab, ylab = ylab, ...)
    for (k in seq_along(col)) {
        keep <- !is.na(z) & z <= breaks[k+1L] &
                if (k == 1L) z >= breaks[k] else z > breaks[k]
        pts <- seq_along(z)[keep]
        if (length(pts)) {
            i <- (pts-1L) %% nrow(z) + 1L
            j <- (pts-1L) %/% nrow(z) + 1L
            rect(ex[i], ey[j], ex[i+1L], ey[j+1L], col = col[k], border = NA)
        }
    }
    invisible(NULL)
}

pie <- function(x, labels = names(x), edges = 200, radius = .8, clockwise = FALSE,
                init.angle = if (clockwise) 90 else 0, density = NULL, angle = 45,
                col = NULL, border = NULL, lty = NULL, main = NULL, ...) {
    if (!is.numeric(x) || any(!is.finite(x) | x < 0) || !length(x) || max(x) <= 0)
        stop("'x' values must be positive.")
    if (is.null(labels)) labels <- as.character(seq_along(x))
    if (is.null(col)) col <- if (is.null(density)) c("lightblue","mistyrose","lightcyan","lavender","cornsilk","white") else par("fg")
    n <- length(x)
    col <- rep(col, length.out = n)
    if (is.null(lty)) lty <- par("lty")
    lty <- rep(lty, length.out = n)
    angle <- rep(angle, length.out = n)
    if (!is.null(density)) density <- rep(density, length.out = n)
    if (!is.null(border)) border <- rep(border, length.out = n)
    frac <- x / max(x)
    frac <- frac / sum(frac)
    direction <- if (clockwise) -1 else 1
    pi <- 3.141592653589793
    angle.at <- init.angle*pi/180 + direction*2*pi*c(0,cumsum(frac))
    plot.new()
    plot.window(c(-1,1), c(-1,1), asp = 1, xaxs = "i", yaxs = "i")
    for (i in seq_len(n)) {
        if (frac[i] == 0) next
        a <- seq(angle.at[i], angle.at[i+1L], length.out = max(2,ceiling(edges*frac[i])))
        polygon(c(0,radius*cos(a)), c(0,radius*sin(a)), col = col[i],
                density = density[i], angle = angle[i], border = border[i], lty = lty[i])
        m <- .g_mean(angle.at[c(i,i+1L)])
        xx <- cos(m); yy <- sin(m)
        if (!is.na(labels[i]) && nzchar(labels[i])) {
            segments(radius*xx, radius*yy, 1.05*radius*xx, 1.05*radius*yy)
            text(1.1*radius*xx, 1.1*radius*yy, labels[i], adj = c(if (xx < 0) 1 else 0,.5), ...)
        }
    }
    title(main = main)
    invisible(NULL)
}

stripchart <- function(x, ...) UseMethod("stripchart")
stripchart.default <- function(x, method = c("overplot","jitter","stack"), jitter = .1,
                                offset = 1/3, vertical = FALSE, group.names,
                                add = FALSE, at = NULL, xlim = NULL, ylim = NULL,
                                ylab = NULL, xlab = NULL, dlab = NULL, glab = NULL,
                                log = "", pch = 0, col = par("fg"), cex = par("cex"),
                                axes = TRUE, frame.plot = axes, ...) {
    method <- grDevices:::.gd_choice(method, c("overplot","jitter","stack"))
    if (!is.list(x)) x <- list(x)
    n <- length(x)
    if (missing(group.names)) group.names <- if (is.null(names(x))) as.character(seq_len(n)) else names(x)
    if (is.null(at)) at <- seq_len(n)
    if (length(at) != n) stop("invalid 'at' argument")
    if (!add) {
        vals <- .g_plot_range(do.call(c, x), if (vertical) "y" else "x")
        groups <- range(at) + c(-.5,.5)
        if (is.null(xlim)) xlim <- if (vertical) groups else vals
        if (is.null(ylim)) ylim <- if (vertical) vals else groups
        plot.new()
        plot.window(xlim, ylim, log = log, ...)
        if (axes) {
            g <- if (vertical) 1L else 2L
            axis(g, at, group.names, ...)
            axis(3L-g, ...)
        }
        if (frame.plot) box(...)
        if (is.null(xlab)) xlab <- if (vertical) glab else dlab
        if (is.null(ylab)) ylab <- if (vertical) dlab else glab
        title(xlab = xlab, ylab = ylab, ...)
    }
    pch <- rep(pch, length.out = n)
    col <- rep(col, length.out = n)
    for (i in seq_len(n)) {
        v <- x[[i]]
        v <- v[is.finite(v)]
        pos <- rep(at[i], length(v))
        if (method == "jitter") pos <- pos + stats::runif(length(v), -jitter, jitter)
        if (method == "stack") {
            for (k in seq_along(v)) pos[k] <- pos[k] + offset * sum(v[seq_len(k-1L)] == v[k])
        }
        if (vertical) points(pos, v, pch = pch[i], col = col[i], cex = cex, ...)
        else points(v, pos, pch = pch[i], col = col[i], cex = cex, ...)
    }
    invisible(NULL)
}
stripchart.formula <- function(formula, data = NULL, ..., subset, na.action = NULL) {
    mf <- .g_formula_frame(match.call(expand.dots = FALSE), parent.frame())
    stripchart.default(.g_split(mf[[1L]], mf[[2L]]), ...)
}

.g_split <- function(x, groups, drop = FALSE, sep = ".", lex.order = FALSE) {
    if (is.list(groups)) {
        factors <- lapply(groups, function(g) if (is.factor(g)) g else factor(g))
        if (lex.order) factors <- rev(factors)
        ids <- rep(1, length(x))
        labels <- ""
        stride <- 1
        for (f in factors) {
            lev <- levels(f)
            if (!length(lev)) return(list())
            ids <- ids + (as.integer(f) - 1) * stride
            count <- stride * length(lev)
            index <- seq_len(count) - 1
            first <- labels[index %% stride + 1]
            second <- lev[index %/% stride + 1]
            labels <- if (stride == 1 && identical(labels, "")) second else
                if (lex.order) paste(second, first, sep = sep) else paste(first, second, sep = sep)
            stride <- count
        }
    } else {
        if (!is.factor(groups)) groups <- factor(groups)
        ids <- as.integer(groups)
        labels <- levels(groups)
    }
    keep <- seq_along(labels)
    if (drop) {
        used <- sort(ids[!is.na(ids)])
        keep <- if (length(used)) used[c(TRUE, diff(used) != 0)] else integer()
    }
    result <- rep(list(NULL), length(keep))
    for (i in seq_along(keep)) result[[i]] <- x[!is.na(ids) & ids == keep[i]]
    names(result) <- labels[keep]
    result
}

.g_formula_frame <- function(call, env) {
    args <- as.list(call)[-1L]
    .Internal(C_rx_formula_frame(args, env, quote(stats::model.frame), quote(stats::na.pass)))
}

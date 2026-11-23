
hist <- function(x, ...) UseMethod("hist")

hist.default <- function(x, breaks = "Sturges", freq = NULL, probability = !freq,
                         include.lowest = TRUE, right = TRUE, fuzz = 1e-7,
                         density = NULL, angle = 45, col = "lightgray", border = NULL,
                         main = paste("Histogram of", xname), xlim = range(breaks),
                         ylim = NULL, xlab = xname, ylab, axes = TRUE, plot = TRUE,
                         labels = FALSE, nclass = NULL, warn.unused = TRUE, ...) {
    xname <- paste(deparse(substitute(x)), collapse = " ")
    if (!is.numeric(x)) stop("'x' must be numeric")
    x <- x[is.finite(x)]
    if (missing(breaks) && !is.null(nclass)) breaks <- nclass
    if (is.character(breaks)) {
        rule <- .g_choice(tolower(breaks), c("sturges", "fd", "freedman-diaconis", "scott"), "breaks")
        breaks <- if (rule == "sturges") nclass.Sturges(x) else
            if (rule == "scott") nclass.scott(x) else nclass.FD(x)
    } else if (is.function(breaks)) breaks <- breaks(x)
    if (length(breaks) == 1L) {
        if (!is.finite(breaks) || breaks < 1 || !length(x)) stop("invalid number of 'breaks'")
        breaks <- pretty(range(x), n = min(1e6, breaks), min.n = 1)
    } else breaks <- sort(breaks)
    if (length(breaks) < 2L) stop("invalid number of 'breaks'")
    widths <- diff(as.double(breaks))
    equidist <- length(widths) == 1L || diff(range(widths)) < 1e-7 * .g_mean(widths)
    counts <- .Internal(C_rx_hist_counts(x, breaks, right, include.lowest, fuzz))
    ans <- structure(list(breaks = breaks, counts = counts,
                          density = counts / (length(x) * widths),
                          mids = breaks[-1L] / 2 + breaks[-length(breaks)] / 2,
                          xname = xname, equidist = equidist), class = "histogram")
    if (plot) {
        if (is.null(freq)) freq <- if (missing(probability)) equidist else !probability
        if (missing(ylab)) ylab <- if (freq) "Frequency" else "Density"
        plot.histogram(ans, freq = freq, density = density, angle = angle, col = col,
                       border = border, main = main, xlim = xlim, ylim = ylim, xlab = xlab,
                       ylab = ylab, axes = axes, labels = labels, ...)
        return(invisible(ans))
    }
    if (warn.unused) {
        given <- !c(missing(freq), missing(probability), missing(density), missing(angle),
                    missing(col), missing(border), missing(main), missing(xlim),
                    missing(ylim), missing(xlab), missing(ylab), missing(axes), missing(labels))
        used <- c(c("freq", "probability", "density", "angle", "col", "border", "main",
                    "xlim", "ylim", "xlab", "ylab", "axes", "labels")[given], names(list(...)))
        if (length(used)) {
            word <- if (length(used) == 1L) "argument" else "arguments"
            verb <- if (length(used) == 1L) "is" else "are"
            warning(paste(word, paste0("‘", used, "’", collapse = ", "), verb, "not made use of"))
        }
    }
    ans
}

plot.histogram <- function(x, freq = x$equidist, density = NULL, angle = 45, col = "lightgray",
                           border = NULL, main = paste("Histogram of", x$xname),
                           xlim = range(x$breaks), ylim = NULL, xlab = x$xname,
                           ylab = if (freq) "Frequency" else "Density",
                           axes = TRUE, labels = FALSE, add = FALSE, ...) {
    if (freq && !x$equidist) warning("the AREAS in the plot are wrong -- rather use 'freq = FALSE'")
    height <- if (freq) x$counts else x$density
    if (!add) {
        if (is.null(ylim)) ylim <- c(0, max(height))
        plot.new()
        plot.window(xlim, ylim, ...)
        title(main = main, xlab = xlab, ylab = ylab, ...)
        if (axes) { axis(1L, ...); axis(2L, ...) }
    }
    k <- length(x$breaks)
    rect(x$breaks[-k], 0, x$breaks[-1L], height, density = density,
         angle = angle, col = col, border = border, ...)
    if (!identical(labels, FALSE)) {
        if (isTRUE(labels)) labels <- format(height, trim = TRUE)
        text(x$mids, height, labels, pos = 3L, ...)
    }
    invisible(NULL)
}

boxplot <- function(x, ...) UseMethod("boxplot")

boxplot.default <- function(x, ..., range = 1.5, width = NULL, varwidth = FALSE,
                            notch = FALSE, outline = TRUE, names = NULL, plot = TRUE,
                            border = par("fg"), col = "lightgray", log = "",
                            pars = list(boxwex = .8, staplewex = .5, outwex = .5),
                            horizontal = FALSE, add = FALSE, at = NULL) {
    extras <- list(...)
    if (is.matrix(x)) {
        groups <- rep(list(NULL), ncol(x))
        for (i in seq_along(groups)) groups[[i]] <- x[,i]
        names(groups) <- colnames(x)
    } else if (is.list(x)) groups <- x
    else {
        labels <- base::names(extras)
        if (is.null(labels)) {
            groups <- c(list(x), extras)
            extras <- list()
        } else {
            groups <- c(list(x), extras[labels == ""])
            groups <- structure(groups, names = rep("", length(groups)))
            extras <- extras[labels != ""]
        }
    }
    if (!length(groups)) stop("invalid first argument")
    if (is.null(names)) names <- base::names(groups)
    if (is.null(names)) names <- as.character(seq_along(groups))
    n <- numeric(length(groups))
    stats <- matrix(NA_real_, 5L, length(n))
    conf <- matrix(NA_real_, 2L, length(n))
    out <- group <- numeric()
    for (i in seq_along(groups)) {
        b <- boxplot.stats(as.double(groups[[i]]), coef = range)
        stats[,i] <- b$stats
        conf[,i] <- b$conf
        n[i] <- b$n
        out <- c(out, b$out)
        group <- c(group, rep(as.double(i), length(b$out)))
    }
    ans <- list(stats = stats, n = n, conf = conf, out = out, group = group, names = names)
    if (!plot) return(ans)
    do.call(bxp, c(list(z = ans, width = width, varwidth = varwidth, notch = notch,
                         outline = outline, border = border, col = col, log = log,
                         pars = pars, horizontal = horizontal, add = add, at = at), extras))
    invisible(ans)
}

boxplot.formula <- function(formula, data = NULL, ..., subset, na.action = NULL,
                            drop = FALSE, sep = ".", lex.order = FALSE) {
    mf <- .g_formula_frame(match.call(expand.dots = FALSE), parent.frame())
    if (length(mf) < 2L) stop("invalid formula")
    by <- if (length(mf) == 2L) mf[[2L]] else mf[-1L]
    boxplot.default(.g_split(mf[[1L]], by, drop, sep, lex.order), ...)
}

bxp <- function(z, notch = FALSE, width = NULL, varwidth = FALSE, outline = TRUE,
                notch.frac = .5, log = "", border = par("fg"), col = "lightgray",
                pars = NULL, frame.plot = axes, horizontal = FALSE, add = FALSE,
                at = NULL, show.names = NULL, xlim = NULL, ylim = NULL, axes = TRUE,
                xlab = NULL, ylab = NULL, main = NULL, sub = NULL, ...) {
    n <- ncol(z$stats)
    if (!length(n) || n == 0L) stop("invalid boxplot statistics")
    if (is.null(at)) at <- seq_len(n)
    if (length(at) != n) stop("'at' must have same length as 'z$n'")
    if (is.null(show.names)) show.names <- axes
    extra <- list(...)
    boxwex <- if (!is.null(extra$boxwex)) extra$boxwex else
        if (!is.null(pars$boxwex)) pars$boxwex else .8
    staplewex <- if (!is.null(extra$staplewex)) extra$staplewex else
        if (!is.null(pars$staplewex)) pars$staplewex else .5
    if (is.null(width)) width <- if (varwidth) sqrt(z$n) else rep(1, n)
    half <- rep(width / max(width) * boxwex / 2, length.out = n)
    border <- rep(border, length.out = n)
    col <- rep(col, length.out = n)
    box.axis <- if (horizontal) 2L else 1L
    value.axis <- 3L - box.axis
    if (!add) {
        category <- range(at) + c(-.5,.5)
        values <- c(z$stats, if (outline) z$out)
        if (grepl(if (horizontal) "x" else "y", log, fixed = TRUE)) values <- values[values > 0]
        values <- .g_plot_range(values, if (horizontal) "x" else "y")
        if (is.null(xlim)) xlim <- if (horizontal) values else category
        if (is.null(ylim)) ylim <- if (horizontal) category else values
        plot.new()
        plot.window(xlim, ylim, log = log, ...)
    }
    stroke <- function(x0, y0, x1, y1, ...) {
        if (horizontal) segments(y0, x0, y1, x1, ...)
        else segments(x0, y0, x1, y1, ...)
    }
    for (i in seq_len(n)) {
        v <- z$stats[,i]
        if (any(is.na(v))) next
        pos <- at[i]
        radius <- half[i]
        l <- pos - radius
        r <- pos + radius
        ink <- border[i]
        stroke(pos, v[c(1L,4L)], pos, v[c(2L,5L)], col = ink, lty = 2L)
        stroke(pos - radius*staplewex, v[c(1L,5L)], pos + radius*staplewex, v[c(1L,5L)], col = ink)
        if (notch) {
            ci <- z$conf[,i]
            if (ci[1L] < v[2L] || ci[2L] > v[4L])
                warning("some notches went outside hinges ('box'): maybe set notch=FALSE")
            x <- c(l, r, r, pos+radius*notch.frac, r, r, l, l, pos-radius*notch.frac, l)
            y <- c(v[2L], v[2L], ci[1L], v[3L], ci[2L], v[4L], v[4L], ci[2L], v[3L], ci[1L])
            if (horizontal) polygon(y, x, col = col[i], border = ink)
            else polygon(x, y, col = col[i], border = ink)
            l <- pos - radius*notch.frac
            r <- pos + radius*notch.frac
        } else if (horizontal) rect(v[2L], l, v[4L], r, col = col[i], border = ink)
        else rect(l, v[2L], r, v[4L], col = col[i], border = ink)
        stroke(l, v[3L], r, v[3L], col = if (is.null(extra$medcol)) ink else extra$medcol,
               lwd = if (is.null(extra$medlwd)) 3 else extra$medlwd)
    }
    if (outline && length(z$out)) {
        xx <- at[z$group]
        pch <- if (is.null(extra$pch)) 1L else extra$pch
        if (horizontal) points(z$out, xx, col = border[z$group], pch = pch)
        else points(xx, z$out, col = border[z$group], pch = pch)
    }
    if (show.names) axis(box.axis, at, z$names, ...)
    if (!add) {
        if (axes) axis(value.axis, ...)
        if (frame.plot) box(...)
        title(main, sub, xlab, ylab, ...)
    }
    invisible(at)
}

barplot <- function(height, ...) UseMethod("barplot")

barplot.default <- function(height, width = 1, space = NULL, names.arg = NULL,
                            legend.text = NULL, beside = FALSE, horiz = FALSE,
                            density = NULL, angle = 45, col = NULL, border = par("fg"),
                            main = NULL, sub = NULL, xlab = NULL, ylab = NULL,
                            xlim = NULL, ylim = NULL, xpd = TRUE, log = "",
                            axes = TRUE, axisnames = TRUE, cex.axis = par("cex.axis"),
                            cex.names = par("cex.axis"), inside = TRUE, plot = TRUE,
                            axis.lty = 0, offset = 0, add = FALSE, ann = !add,
                            args.legend = NULL, ...) {
    vector.input <- is.numeric(height) && is.null(dim(height))
    if (!vector.input && !is.matrix(height)) stop("'height' must be a vector or a matrix")
    if (is.null(names.arg)) names.arg <- if (vector.input) names(height) else colnames(height)
    if (vector.input) {
        height <- matrix(height, nrow = 1L)
        beside <- TRUE
    }
    nr <- nrow(height)
    nc <- ncol(height)
    if (!length(height) || !length(width) || any(!is.finite(width)) || any(width <= 0))
        stop("invalid bar dimensions")
    if (is.null(space)) space <- if (beside && !vector.input) c(0,1) else .2
    space <- space * .g_mean(width)
    if (beside && !vector.input && length(space) == 2L) {
        space <- rep(c(space[2L], rep(space[1L], nr - 1L)), nc)
    }
    widths <- if (beside && !vector.input) rep(rep(width, length.out = nr), nc) else
        rep(width, length.out = nc)
    right <- cumsum(widths + rep(space, length.out = length(widths)))
    left <- right - widths
    mids <- right - widths / 2
    if (beside) mids <- matrix(mids, nrow = if (vector.input) nc else nr)
    if (!plot) return(mids)
    is.log <- grepl(if (horiz) "x" else "y", log, fixed = TRUE)
    base <- if (is.log) .9 * min(height, na.rm = TRUE) else 0
    if (is.log && (!is.finite(base) || base <= 0)) stop("log scale error: at least one 'height + offset' value <= 0")
    if (beside) {
        top <- as.double(height) + rep(offset, length.out = length(height))
        bottom <- base + rep(offset, length.out = length(top))
    } else {
        top <- bottom <- height
        offset <- rep(offset, length.out = nc)
        for (j in seq_len(nc)) {
            h <- cumsum(height[,j])
            bottom[,j] <- c(base, h[-nr]) + offset[j]
            top[,j] <- h + offset[j]
        }
    }
    if (!add) {
        bounds <- range(c(left,right))
        values <- .g_plot_range(c(top,bottom, if (!is.log) -.01*height), if (horiz) "x" else "y")
        if (is.null(xlim)) xlim <- if (horiz) values else bounds
        if (is.null(ylim)) ylim <- if (horiz) bounds else values
        plot.new()
        opts <- list(...)
        opts$xaxs <- if (horiz) "i" else "r"
        opts$yaxs <- if (horiz) "r" else "i"
        do.call(plot.window, c(list(xlim, ylim, log = log), opts))
    }
    if (is.null(col)) col <- if (vector.input) "gray" else gray.colors(nr)
    draw <- function(l, r, b, t, fill) {
        if (horiz) rect(b, l, t, r, col = fill, border = border, density = density,
                        angle = angle, xpd = xpd, ...)
        else rect(l, b, r, t, col = fill, border = border, density = density,
                  angle = angle, xpd = xpd, ...)
    }
    if (beside) draw(left, right, bottom, top, if (vector.input) col else rep(col, length.out = nr))
    else {
        fill <- rep(col, length.out = nr)
        for (i in seq_len(nr)) draw(left, right, bottom[i,], top[i,], fill[i])
    }
    category.axis <- if (horiz) 2L else 1L
    if (axisnames && length(names.arg)) {
        at <- if (beside && !vector.input) colMeans(mids) else as.double(mids)
        axis(category.axis, at, names.arg, tick = FALSE, cex.axis = cex.names, ...)
    }
    if (axes && !add) axis(3L - category.axis, lty = axis.lty, cex.axis = cex.axis, ...)
    if (ann) title(main, sub, xlab, ylab, ...)
    if (!is.null(legend.text) && !identical(legend.text, FALSE)) {
        labs <- if (identical(legend.text, TRUE)) rownames(height) else legend.text
        if (length(labs)) {
            opts <- list(x = "topright", legend = labs, fill = col, border = border)
            for (key in names(args.legend)) opts[[key]] <- args.legend[[key]]
            do.call(legend, opts)
        }
    }
    invisible(mids)
}

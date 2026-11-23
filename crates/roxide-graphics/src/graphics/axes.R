
axis <- function(side, at = NULL, labels = TRUE, tick = TRUE, line = NA,
                 pos = NA, outer = FALSE, font = NA, lty = "solid",
                 lwd = 1, lwd.ticks = lwd, col = NULL, col.ticks = NULL,
                 hadj = NA, padj = NA, gap.axis = NA, ...) {
    if (length(side) != 1L || !any(side == 1:4)) stop("invalid 'side' argument")
    .Internal(C_rx_require_frame())
    if (par(if (side == 1L || side == 3L) "xaxt" else "yaxt") == "n") return(invisible(NULL))
    if (is.null(at)) at <- axTicks(side)
    if (!length(at)) return(invisible(NULL))
    if (!is.logical(labels) && length(labels) != length(at))
        stop("'at' and 'labels' lengths differ")
    index <- order(at, na.last = TRUE)
    at <- as.double(at[index])
    if (!is.logical(labels)) labels <- as.character(labels[index])
    u <- .g_limits()
    horizontal <- side == 1L || side == 3L
    along <- if (horizontal) u[1:2] else u[3:4]
    keep <- is.finite(at) & at >= min(along) & at <= max(along)
    if (!any(keep)) return(invisible(at))
    ticks <- at[keep]
    mgp <- par("mgp")
    if (is.na(line)) line <- mgp[3L]
    if (is.null(col)) col <- par("fg")
    if (is.null(col.ticks)) col.ticks <- col
    convert <- if (horizontal) grconvertY else grconvertX
    upper <- side == 3L || side == 4L
    direction <- if (upper) 1 else -1
    edge <- if (upper) 1 else 0
    if (is.na(pos)) {
        ref <- if (outer) "nic" else "npc"
        inches <- convert(edge, ref, "inches") + direction * line * par("csi") * par("mex")
    } else inches <- convert(pos, "user", "inches")
    tck <- par("tck")
    pin <- par("pin")
    span <- if (horizontal) pin[2L] else pin[1L]
    len <- if (is.na(tck)) par("tcl") * par("csi") * par("mex") else
        tck * if (abs(tck) < .5) min(pin) else span
    endpoints <- convert(c(inches, inches - direction * len), "inches", "user")
    if (tick) {
        ext <- range(ticks)
        if (horizontal) {
            segments(ext[1L], endpoints[1L], ext[2L], endpoints[1L],
                     col = col, lty = lty, lwd = lwd, xpd = NA)
            segments(ticks, endpoints[1L], ticks, endpoints[2L],
                     col = col.ticks, lty = lty, lwd = lwd.ticks, xpd = NA)
        } else {
            segments(endpoints[1L], ext[1L], endpoints[1L], ext[2L],
                     col = col, lty = lty, lwd = lwd, xpd = NA)
            segments(endpoints[1L], ticks, endpoints[2L], ticks,
                     col = col.ticks, lty = lty, lwd = lwd.ticks, xpd = NA)
        }
    }
    if (!identical(labels, FALSE)) {
        if (is.logical(labels)) labels <- format(at, trim = TRUE)
        extra <- list(...)
        las <- if (is.null(extra$las)) par("las") else extra$las
        cex <- if (is.null(extra$cex.axis)) par("cex.axis") else extra$cex.axis
        ink <- if (is.null(extra$col.axis)) par("col.axis") else extra$col.axis
        if (is.na(font)) font <- par("font.axis")
        if (!is.na(pos)) line <- (inches - convert(edge, "npc", "inches")) /
                                 (direction * par("csi") * par("mex"))
        .g_draw("margin", list(side, line + mgp[2L] - mgp[3L], ticks, hadj,
                               las, labels[keep], outer),
                list(cex = cex, col = ink, font = font, padj = padj))
    }
    invisible(at)
}

Axis <- function(x = NULL, at = NULL, ..., side, labels = NULL) {
    if (is.null(x)) Axis.default(at = at, ..., side = side, labels = labels)
    else UseMethod("Axis")
}

Axis.default <- function(x = NULL, at = NULL, ..., side, labels = NULL) {
    if (is.null(labels)) labels <- TRUE
    axis(side, at = at, labels = labels, ...)
}

.g_plot_range <- function(x, name) {
    x <- x[is.finite(x)]
    if (!length(x)) stop(paste0("need finite '", name, "lim' values"))
    range(x)
}

plot <- function(x, y, ...) UseMethod("plot")

plot.default <- function(x, y = NULL, type = "p", xlim = NULL, ylim = NULL,
                         log = "", main = NULL, sub = NULL, xlab = NULL, ylab = NULL,
                         ann = par("ann"), axes = TRUE, frame.plot = axes,
                         panel.first = NULL, panel.last = NULL, asp = NA,
                         xgap.axis = NA, ygap.axis = NA, ...) {
    label.x <- deparse(substitute(x))[1L]
    label.y <- if (is.null(y)) label.x else deparse(substitute(y))[1L]
    xy <- xy.coords(x, y, label.x, label.y, log)
    if (is.null(xlab)) xlab <- xy$xlab
    if (is.null(ylab)) ylab <- xy$ylab
    if (is.null(xlim)) xlim <- .g_plot_range(xy$x, "x")
    if (is.null(ylim)) ylim <- .g_plot_range(xy$y, "y")
    plot.new()
    plot.window(xlim, ylim, log = log, asp = asp, ...)
    panel.first
    plot.xy(xy, type, ...)
    panel.last
    if (axes) {
        Axis(x, side = 1L, gap.axis = xgap.axis, ...)
        Axis(if (is.null(y)) x else y, side = 2L, gap.axis = ygap.axis, ...)
    }
    if (frame.plot) box(...)
    if (ann) title(main = main, sub = sub, xlab = xlab, ylab = ylab, ...)
    invisible(NULL)
}

.g_mean <- function(x) sum(x) / length(x)

legend <- function(x, y = NULL, legend, fill = NULL, col = par("col"),
                   border = "black", lty, lwd, pch, angle = 45, density = NULL,
                   bty = "o", bg = par("bg"), box.lwd = par("lwd"),
                   box.lty = par("lty"), box.col = par("fg"), pt.bg = NA,
                   cex = 1, pt.cex = cex, pt.lwd = lwd, xjust = 0, yjust = 1,
                   x.intersp = 1, y.intersp = 1, adj = c(0,.5), text.width = NULL,
                   text.col = par("col"), text.font = NULL, merge = has.lines && has.points,
                   trace = FALSE, plot = TRUE, ncol = 1, horiz = FALSE, title = NULL,
                   inset = 0, xpd, title.col = text.col[1L], title.adj = .5,
                   title.cex = cex[1L], title.font = text.font, seg.len = 2) {
    if (missing(legend)) { legend <- y; y <- NULL }
    .Internal(C_rx_require_frame())
    n <- length(legend)
    if (!n) stop("'legend' is of length 0")
    if (!is.finite(ncol) || ncol < 1) stop("invalid 'ncol' argument")
    ncol <- if (horiz) n else as.integer(ncol)
    if (is.na(ncol) || ncol < 1L) stop("invalid 'ncol' argument")
    nr <- ceiling(n / ncol)
    index <- seq_len(n) - 1L
    rows <- index %% nr + 1L
    cols <- index %/% nr + 1L
    cex <- rep(cex, length.out = n)
    has.lines <- (!missing(lwd) && length(lwd) > 0L) ||
                 (!missing(lty) && length(lty) > 0L && (is.character(lty) || any(lty != 0)))
    has.points <- !missing(pch) && length(pch) > 0L
    if (missing(lty)) lty <- 1L
    if (missing(lwd)) lwd <- par("lwd")
    if (missing(xpd)) xpd <- par("xpd")

    usr <- par("usr")
    pin <- par("pin")
    cin <- par("cin") * par("cex")
    sx <- diff(usr[1:2]) / pin[1L]
    sy <- diff(usr[3:4]) / pin[2L]
    xc <- cex * cin[1L] * sx
    yc <- cex * cin[2L] * sy
    metric <- function(fun) {
        if (all(cex == cex[1L])) return(fun(legend, "inches", cex = cex[1L], font = text.font))
        vapply(seq_len(n), function(i) fun(legend[i], "inches", cex = cex[i], font = text.font), numeric(1))
    }
    sw <- metric(strwidth) * sx
    if (is.null(text.width)) text.width <- rep(max(abs(sw)), ncol)
    else if (length(text.width) == 1L && is.na(text.width)) {
        text.width <- vapply(seq_len(ncol), function(i) max(c(0, abs(sw[cols == i]))), numeric(1))
    } else text.width <- rep(text.width, length.out = ncol)
    sample <- if (has.lines) seg.len - if (merge && has.points) .7 else 0 else 0
    fill.size <- if (is.null(fill)) 0 else .8
    offset <- (1 + rep(x.intersp, length.out = n) + sample + fill.size) * xc
    col.width <- vapply(seq_len(ncol), function(i) {
        v <- offset[cols == i]
        max(abs(if (length(v)) v else offset))
    }, numeric(1)) +
                 abs(text.width)
    col.start <- c(0, cumsum(col.width))[seq_len(ncol)]
    w <- sign(sx) * (sum(col.width) + .5 * max(abs(xc)))
    sh <- metric(strheight) * sy
    row.height <- vapply(seq_len(nr), function(i) max(abs(c(yc[rows == i], sh[rows == i]))), numeric(1))
    row.height <- sign(sy) * row.height * rep(y.intersp, length.out = nr)
    title.height <- 0
    extra <- 0
    if (length(title)) {
        title.height <- sign(sy) * max(abs(c(title.cex * cin[2L] * sy,
                           strheight(title, "inches", cex = title.cex, font = title.font) * sy)))
        title.width <- max(strwidth(title, "inches", cex = title.cex, font = title.font)) * sx
        title.width <- title.width + sign(sx) * .5 * max(abs(xc))
        if (abs(title.width) > abs(w)) { extra <- (title.width - w)/2; w <- title.width }
    }
    h <- sum(row.height) + title.height + row.height[1L]/2 + row.height[nr]/2
    logx <- par("xlog")
    logy <- par("ylog")
    if (is.character(x)) {
        key <- grDevices:::.gd_choice(x, c("bottomright", "bottom", "bottomleft", "left",
                               "topleft", "top", "topright", "right", "center"))
        inset <- rep(inset, length.out = 2L)
        left <- if (any(key == c("bottomleft","left","topleft"))) usr[1L] + inset[1L] * diff(usr[1:2])
                else if (any(key == c("bottomright","right","topright"))) usr[2L] - inset[1L] * diff(usr[1:2]) - w
                else .g_mean(usr[1:2]) - w/2
        top <- if (any(key == c("topleft","top","topright"))) usr[4L] - inset[2L] * diff(usr[3:4])
               else if (any(key == c("bottomleft","bottom","bottomright"))) usr[3L] + inset[2L] * diff(usr[3:4]) + h
               else .g_mean(usr[3:4]) + h/2
    } else {
        xy <- xy.coords(x, y, setLab = FALSE)
        x <- if (logx) log10(xy$x) else xy$x
        y <- if (logy) log10(xy$y) else xy$y
        if (length(x) == 2L) { left <- min(x); top <- max(y); w <- diff(range(x)); h <- diff(range(y)) }
        else if (length(x) == 1L) { left <- x - xjust*w; top <- y + (1-yjust)*h }
        else stop("invalid legend coordinates")
    }
    tx <- left + sign(sx)*col.start[cols] + offset + extra
    ty <- top - title.height - row.height[1L]/2 -
          (cumsum(row.height) - row.height/2)[rows]
    result <- list(rect = list(w = w, h = h, left = left, top = top),
                   text = list(x = tx, y = ty))
    if (trace) print(result)
    if (plot) {
        xx <- function(x) if (logx) 10^x else x
        yy <- function(y) if (logy) 10^y else y
        if (bty != "n") rect(xx(left), yy(top-h), xx(left+w), yy(top),
                             col = bg, border = box.col, lty = box.lty, lwd = box.lwd, xpd = xpd)
        mark <- tx - rep(x.intersp, length.out = n)*xc
        if (!is.null(fill)) {
            edge <- mark - (sample + .2)*xc
            rect(xx(edge - .8*xc), yy(ty - .4*yc), xx(edge), yy(ty + .4*yc),
                 col = fill, border = border, density = density, angle = angle, xpd = xpd)
        }
        if (has.lines) segments(xx(mark - seg.len*xc), yy(ty), xx(mark), yy(ty),
                                col = col, lty = lty, lwd = lwd, xpd = xpd)
        if (has.points) points(xx(mark - if (merge && has.lines) seg.len*xc/2 else 0),
                               yy(ty), pch = pch, col = col, bg = pt.bg,
                               cex = pt.cex, lwd = pt.lwd, xpd = xpd)
        text(xx(tx + adj[1L]*sw), yy(ty), legend, adj = adj, cex = cex,
             col = text.col, font = text.font, xpd = xpd)
        if (title.height != 0)
            text(xx(left + w*title.adj), yy(top - row.height[1L]/2 - title.height/2),
                 title, adj = c(title.adj,.5), cex = title.cex, col = title.col,
                 font = title.font, xpd = xpd)
    }
    invisible(result)
}

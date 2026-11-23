.grid_names$grob <- 0L
.grid_half <- unit(.5, "npc")
.grid_one <- unit(1, "npc")
.grid_zero <- unit(0, "npc")
.grid_span <- unit(c(0, 1), "npc")
.grid_char <- unit(1, "char")

is.grob <- function(x) inherits(x, "grob")

grobName <- function(grob = NULL, prefix = "GRID")
    .Internal(C_grid_grob_name(grob, prefix, .grid_names))

.grid_validate_grob <- function(x)
    validDetails(.Internal(C_grid_grob_validate(x, .grid_names)))

grob <- function(..., name = NULL, gp = NULL, vp = NULL, cl = NULL) {
    .grid_validate_grob(structure(c(list(...), list(name = name, gp = gp, vp = vp)),
        class = c(cl, "grob", "gDesc")))
}

gList <- function(...) .Internal(C_grid_glist(list(...)))

.grid_children <- function(x, children) .Internal(C_grid_children(x, children))

gTree <- function(..., name = NULL, gp = NULL, vp = NULL, children = NULL,
                  childrenvp = NULL, cl = NULL) {
    fields <- c(list(...), list(name = name, gp = gp, vp = vp))
    if (!is.null(childrenvp)) fields$childrenvp <- childrenvp
    x <- structure(fields, class = c(cl, "gTree", "grob", "gDesc"))
    if (is.null(children)) children <- gList()
    .grid_validate_grob(.grid_children(x, children))
}

gEdit <- function(...) structure(list(...), class = "gEdit")
gEditList <- function(...) structure(list(...), class = "gEditList")

childNames <- function(gTree) gTree$childrenOrder

addGrob <- function(gTree, child, gPath = NULL, strict = FALSE, grep = FALSE, global = FALSE) {
    if (!inherits(gTree, "gTree") || !is.grob(child)) stop("invalid grob")
    if (!is.null(gPath)) {
        paths <- .grid_matches(list(gTree), gPath, strict, grep)
        if (!length(paths)) stop("'gPath' not found")
        trees <- list(gTree)
        for (path in paths[seq_len(if (global) length(paths) else 1L)])
            trees <- .grid_replace(trees, path, function(x) addGrob(x, child))
        return(trees[[1L]])
    }
    .grid_children(gTree, structure(c(unclass(gTree$children), list(child)), class = "gList"))
}

setChildren <- function(x, children) .grid_children(x, children)

gPath <- function(...) {
    parts <- c(...)
    if (!is.character(parts) || !length(parts) || any(is.na(parts)))
        stop("invalid grob name")
    parts <- strsplit(paste(parts, collapse = "::"), "::", fixed = TRUE)[[1L]]
    if (!length(parts) || any(!nzchar(parts))) stop("invalid grob name")
    n <- length(parts)
    structure(list(path = if (n == 1L) NULL else paste(parts[-n], collapse = "::"),
        name = parts[n], n = as.integer(n)), class = c("gPath", "path"))
}

as.character.gPath <- function(x, ...) {
    if (is.null(x$path)) x$name else paste(x$path, x$name, sep = "::")
}

print.gPath <- function(x, ...) {
    cat(as.character.gPath(x), " ", "\n", sep = "")
    invisible(x)
}

as.character.grob <- function(x, ...) paste0(class(x)[1L], "[", x$name, "]")

print.grob <- function(x, ...) {
    cat(as.character.grob(x), " ", "\n", sep = "")
    invisible(x)
}

print.gList <- function(x, ...) {
    cat("(", paste(vapply(x, as.character.grob, ""), collapse = ", "), ")\n", sep = "")
    invisible(x)
}

validDetails <- function(x) UseMethod("validDetails")
validDetails.default <- function(x) x
makeContext <- function(x) UseMethod("makeContext")
makeContext.default <- function(x) x
makeContext.frame <- function(x) {
    x$vp <- if (is.null(x$vp)) x$framevp else vpStack(x$vp, x$framevp)
    x
}
makeContent <- function(x) UseMethod("makeContent")
makeContent.default <- function(x) x
preDrawDetails <- function(x) UseMethod("preDrawDetails")
preDrawDetails.default <- function(x) invisible(NULL)
postDrawDetails <- function(x) UseMethod("postDrawDetails")
postDrawDetails.default <- function(x) invisible(NULL)
drawDetails <- function(x, recording = TRUE) UseMethod("drawDetails")
drawDetails.default <- function(x, recording = TRUE) {
    node <- .grid_state()$current
    just <- if (is.null(x$just)) c(.5, .5) else .grid_just(x$just)
    .Internal(C_grid_draw(x, node, just))
    invisible(NULL)
}

.grid_coord <- function(x, units) {
    if (is.unit(x)) x else unit(x, units)
}

rectGrob <- function(x = .grid_half, y = .grid_half, width = .grid_one, height = .grid_one, just = "centre", hjust = NULL,
    vjust = NULL, default.units = "npc", name = NULL, gp = gpar(), vp = NULL) {
    x <- .grid_coord(x, default.units)
    y <- .grid_coord(y, default.units)
    width <- .grid_coord(width, default.units)
    height <- .grid_coord(height, default.units)
    grob(x = x, y = y, width = width, height = height, just = just, hjust = hjust,
         vjust = vjust, name = name, gp = gp, vp = vp, cl = "rect")
}

grid.rect <- function(x = .grid_half, y = .grid_half, width = .grid_one, height = .grid_one, just = "centre", hjust = NULL,
    vjust = NULL, default.units = "npc", name = NULL, gp = gpar(), draw = TRUE,
    vp = NULL) {
    x <- rectGrob(x = x, y = y, width = width, height = height, just = just, hjust = hjust,
        vjust = vjust, default.units = default.units, name = name, gp = gp, vp = vp)
    if (draw) grid.draw(x)
    invisible(x)
}

linesGrob <- function(x = .grid_span, y = .grid_span, default.units = "npc", arrow = NULL, name = NULL,
    gp = gpar(), vp = NULL) {
    x <- .grid_coord(x, default.units)
    y <- .grid_coord(y, default.units)
    grob(x = x, y = y, arrow = arrow, name = name, gp = gp, vp = vp, cl = "lines")
}

grid.lines <- function(x = .grid_span, y = .grid_span, default.units = "npc", arrow = NULL, name = NULL,
    gp = gpar(), draw = TRUE, vp = NULL) {
    x <- linesGrob(x = x, y = y, default.units = default.units, arrow = arrow, name = name,
        gp = gp, vp = vp)
    if (draw) grid.draw(x)
    invisible(x)
}

polylineGrob <- function(x = .grid_span, y = .grid_span, id = NULL, id.lengths = NULL, default.units = "npc",
    arrow = NULL, name = NULL, gp = gpar(), vp = NULL) {
    x <- .grid_coord(x, default.units)
    y <- .grid_coord(y, default.units)
    .grid_check_groups(x, y, id, id.lengths)
    grob(x = x, y = y, id = id, id.lengths = id.lengths, arrow = arrow, name = name,
         gp = gp, vp = vp, cl = "polyline")
}

grid.polyline <- function(x = .grid_span, y = .grid_span, id = NULL, id.lengths = NULL, default.units = "npc",
    arrow = NULL, name = NULL, gp = gpar(), draw = TRUE, vp = NULL) {
    x <- polylineGrob(x = x, y = y, id = id, id.lengths = id.lengths, default.units = default.units,
        arrow = arrow, name = name, gp = gp, vp = vp)
    if (draw) grid.draw(x)
    invisible(x)
}

segmentsGrob <- function(x0 = .grid_zero, y0 = .grid_zero, x1 = .grid_one, y1 = .grid_one, default.units = "npc", arrow = NULL, name = NULL,
    gp = gpar(), vp = NULL) {
    x0 <- .grid_coord(x0, default.units)
    y0 <- .grid_coord(y0, default.units)
    x1 <- .grid_coord(x1, default.units)
    y1 <- .grid_coord(y1, default.units)
    grob(x0 = x0, y0 = y0, x1 = x1, y1 = y1, arrow = arrow, name = name, gp = gp,
         vp = vp, cl = "segments")
}

grid.segments <- function(x0 = .grid_zero, y0 = .grid_zero, x1 = .grid_one, y1 = .grid_one, default.units = "npc", arrow = NULL, name = NULL,
    gp = gpar(), draw = TRUE, vp = NULL) {
    x <- segmentsGrob(x0 = x0, y0 = y0, x1 = x1, y1 = y1, default.units = default.units,
        arrow = arrow, name = name, gp = gp, vp = vp)
    if (draw) grid.draw(x)
    invisible(x)
}

polygonGrob <- function(x = c(0, .5, 1, .5), y = c(.5, 1, .5, 0), id = NULL, id.lengths = NULL,
    default.units = "npc", name = NULL, gp = gpar(), vp = NULL) {
    x <- .grid_coord(x, default.units)
    y <- .grid_coord(y, default.units)
    .grid_check_groups(x, y, id, id.lengths)
    grob(x = x, y = y, id = id, id.lengths = id.lengths, name = name, gp = gp,
         vp = vp, cl = "polygon")
}

grid.polygon <- function(x = c(0, .5, 1, .5), y = c(.5, 1, .5, 0), id = NULL, id.lengths = NULL,
    default.units = "npc", name = NULL, gp = gpar(), draw = TRUE, vp = NULL) {
    x <- polygonGrob(x = x, y = y, id = id, id.lengths = id.lengths, default.units = default.units,
        name = name, gp = gp, vp = vp)
    if (draw) grid.draw(x)
    invisible(x)
}

pathGrob <- function(x, y, id = NULL, id.lengths = NULL, pathId = NULL,
    pathId.lengths = NULL, rule = "winding", default.units = "npc", name = NULL,
    gp = gpar(), vp = NULL) {
    x <- .grid_coord(x, default.units)
    y <- .grid_coord(y, default.units)
    rule <- .grid_match_arg(rule, c("winding", "evenodd"))
    .grid_check_groups(x, y, id, id.lengths)
    grob(x = x, y = y, id = id, id.lengths = id.lengths, pathId = pathId,
         pathId.lengths = pathId.lengths, rule = rule, name = name, gp = gp, vp = vp,
         cl = "pathgrob")
}

grid.path <- function(x, y, id = NULL, id.lengths = NULL, pathId = NULL,
    pathId.lengths = NULL, rule = "winding", default.units = "npc", name = NULL,
    gp = gpar(), draw = TRUE, vp = NULL) {
    x <- pathGrob(x = x, y = y, id = id, id.lengths = id.lengths, pathId = pathId,
        pathId.lengths = pathId.lengths, rule = rule, default.units = default.units,
        name = name, gp = gp, vp = vp)
    if (draw) grid.draw(x)
    invisible(x)
}

circleGrob <- function(x = .5, y = .5, r = .5, default.units = "npc", name = NULL, gp = gpar(), vp = NULL) {
    x <- .grid_coord(x, default.units)
    y <- .grid_coord(y, default.units)
    r <- .grid_coord(r, default.units)
    grob(x = x, y = y, r = r, name = name, gp = gp, vp = vp, cl = "circle")
}

grid.circle <- function(x = .5, y = .5, r = .5, default.units = "npc", name = NULL, gp = gpar(),
    draw = TRUE, vp = NULL) {
    x <- circleGrob(x = x, y = y, r = r, default.units = default.units, name = name, gp = gp,
        vp = vp)
    if (draw) grid.draw(x)
    invisible(x)
}

pointsGrob <- function(x = stats::runif(10), y = stats::runif(10), pch = 1, size = .grid_char,
    default.units = "native", name = NULL, gp = gpar(), vp = NULL) {
    x <- .grid_coord(x, default.units)
    y <- .grid_coord(y, default.units)
    if (length(x) != length(y)) stop("'x' and 'y' must be 'unit' objects and have the same length")
    grob(x = x, y = y, pch = pch, size = size, name = name, gp = gp, vp = vp,
         cl = "points")
}

grid.points <- function(x = stats::runif(10), y = stats::runif(10), pch = 1, size = .grid_char,
    default.units = "native", name = NULL, gp = gpar(), draw = TRUE, vp = NULL) {
    x <- pointsGrob(x = x, y = y, pch = pch, size = size, default.units = default.units,
        name = name, gp = gp, vp = vp)
    if (draw) grid.draw(x)
    invisible(x)
}

textGrob <- function(label, x = .grid_half, y = .grid_half, just = "centre", hjust = NULL, vjust = NULL, rot = 0,
    check.overlap = FALSE, default.units = "npc", name = NULL, gp = gpar(), vp = NULL) {
    x <- .grid_coord(x, default.units)
    y <- .grid_coord(y, default.units)
    if (typeof(label) != "expression") label <- as.character(label)
    grob(label = label, x = x, y = y, just = just, hjust = hjust, vjust = vjust,
         rot = rot, check.overlap = check.overlap, name = name, gp = gp, vp = vp,
         cl = "text")
}

grid.text <- function(label, x = .grid_half, y = .grid_half, just = "centre", hjust = NULL, vjust = NULL, rot = 0,
    check.overlap = FALSE, default.units = "npc", name = NULL, gp = gpar(),
    draw = TRUE, vp = NULL) {
    x <- textGrob(label = label, x = x, y = y, just = just, hjust = hjust, vjust = vjust,
        rot = rot, check.overlap = check.overlap, default.units = default.units,
        name = name, gp = gp, vp = vp)
    if (draw) grid.draw(x)
    invisible(x)
}

nullGrob <- function(x = .grid_half, y = .grid_half, default.units = "npc", name = NULL, vp = NULL) {
    x <- .grid_coord(x, default.units)
    y <- .grid_coord(y, default.units)
    grob(x = x, y = y, name = name, vp = vp, cl = "null")
}

grid.null <- function(x = .grid_half, y = .grid_half, default.units = "npc", name = NULL, vp = NULL, draw = TRUE) {
    x <- nullGrob(x = x, y = y, default.units = default.units, name = name, vp = vp)
    if (draw) grid.draw(x)
    invisible(x)
}

rasterGrob <- function(image, x = .grid_half, y = .grid_half, width = NULL, height = NULL, just = "centre", hjust = NULL,
    vjust = NULL, interpolate = TRUE, default.units = "npc", name = NULL, gp = gpar(),
    vp = NULL) {
    x <- .grid_coord(x, default.units)
    y <- .grid_coord(y, default.units)
    if (!is.null(width)) width <- .grid_coord(width, default.units)
    if (!is.null(height)) height <- .grid_coord(height, default.units)
    raster <- .grid_raster(image)
    grob(raster = raster, x = x, y = y, width = width, height = height, just = just,
         hjust = hjust, vjust = vjust, interpolate = interpolate, name = name,
         gp = gp, vp = vp, cl = "rastergrob")
}

grid.raster <- function(image, x = .grid_half, y = .grid_half, width = NULL, height = NULL, just = "centre", hjust = NULL,
    vjust = NULL, interpolate = TRUE, default.units = "npc", name = NULL, gp = gpar(),
    vp = NULL, draw = TRUE) {
    x <- rasterGrob(image = image, x = x, y = y, width = width, height = height, just = just,
        hjust = hjust, vjust = vjust, interpolate = interpolate,
        default.units = default.units, name = name, gp = gp, vp = vp)
    if (draw) grid.draw(x)
    invisible(x)
}

.grid_raster <- function(image) {
    if (inherits(image, "raster")) return(image)
    d <- dim(image)
    if (is.null(d)) d <- c(length(image), 1L)
    if (length(d) == 3L && (d[3L] == 3L || d[3L] == 4L)) {
        image <- if (d[3L] == 3L) grDevices::rgb(image[, , 1L], image[, , 2L], image[, , 3L])
                 else grDevices::rgb(image[, , 1L], image[, , 2L], image[, , 3L], image[, , 4L])
        d <- d[1:2]
    } else if (is.numeric(image) || is.logical(image)) {
        image <- grDevices::gray(as.numeric(image))
    }
    if (length(d) != 2L) stop("invalid raster dimensions")
    structure(as.character(image), dim = d, class = "raster")
}

.grid_axis <- function(at, label, main, edits, name, gp, vp, axis) {
    x <- gTree(at = at, label = label, main = main, edits = edits,
               name = name, gp = gp, vp = vp, cl = c(axis, "axis"))
    if (!is.null(at)) return(.grid_axis_content(x, at, axis == "xaxis"))
    x["children"] <- list(NULL)
    x
}

xaxisGrob <- function(at = NULL, label = TRUE, main = TRUE, edits = NULL,
                      name = NULL, gp = gpar(), vp = NULL)
    .grid_axis(at, label, main, edits, name, gp, vp, "xaxis")

yaxisGrob <- function(at = NULL, label = TRUE, main = TRUE, edits = NULL,
                      name = NULL, gp = gpar(), vp = NULL)
    .grid_axis(at, label, main, edits, name, gp, vp, "yaxis")

grid.xaxis <- function(at = NULL, label = TRUE, main = TRUE, edits = NULL,
                       name = NULL, gp = gpar(), draw = TRUE, vp = NULL) {
    x <- xaxisGrob(at, label, main, edits, name, gp, vp)
    if (draw) grid.draw(x)
    invisible(x)
}

grid.yaxis <- function(at = NULL, label = TRUE, main = TRUE, edits = NULL,
                       name = NULL, gp = gpar(), draw = TRUE, vp = NULL) {
    x <- yaxisGrob(at, label, main, edits, name, gp, vp)
    if (draw) grid.draw(x)
    invisible(x)
}

.grid_axis_content <- function(x, at, horizontal) {
    if (!length(at)) return(x)
    axis <- unit(at, "native")
    ends <- unit(range(at), "native")
    edge <- as.numeric(!x$main)
    direction <- if (x$main) -1 else 1
    base <- unit(edge, "npc")
    beyond <- unit(direction * .5, "lines")
    margin <- unit(direction * (if (horizontal) 1.5 else 1), "lines")
    if (!x$main) {
        beyond <- base + beyond
        margin <- base + margin
    }
    labels <- if (isTRUE(x$label)) format(at, trim = TRUE) else x$label
    show <- !identical(labels, FALSE)
    if (show && length(labels) != length(at)) stop("invalid 'label' length")
    baseline <- unit(rep(edge, 2L), "npc")
    if (horizontal) {
        major <- linesGrob(ends, baseline, name = "major")
        ticks <- segmentsGrob(axis, base, axis, beyond, name = "ticks")
        lab <- if (show) textGrob(labels, axis, margin, name = "labels", check.overlap = TRUE)
    } else {
        major <- linesGrob(baseline, ends, name = "major")
        ticks <- segmentsGrob(base, axis, beyond, axis, name = "ticks")
        just <- c(if (x$main) "right" else "left", "centre")
        lab <- if (show) textGrob(labels, margin, axis, just = just,
                                  name = "labels", check.overlap = TRUE)
    }
    x <- .grid_children(x, gList(major, ticks, lab))
    for (edit in x$edits) x <- do.call(editGrob, c(list(x), unclass(edit)))
    x
}

.grid_axis_at <- function(scale) {
    limits <- range(scale)
    at <- grDevices::pretty(limits)
    at[at >= limits[1L] & at <= limits[2L]]
}

makeContent.xaxis <- function(x) {
    if (!is.null(x$at)) return(x)
    .grid_axis_content(x, .grid_axis_at(current.viewport()$xscale), TRUE)
}

makeContent.yaxis <- function(x) {
    if (!is.null(x$at)) return(x)
    .grid_axis_content(x, .grid_axis_at(current.viewport()$yscale), FALSE)
}

frameGrob <- function(layout = NULL, name = NULL, gp = gpar(), vp = NULL)
    gTree(framevp = viewport(layout = layout), name = name, gp = gp, vp = vp, cl = "frame")

grid.frame <- function(layout = NULL, name = NULL, gp = gpar(), vp = NULL, draw = TRUE) {
    x <- frameGrob(layout, name, gp, vp)
    if (draw) grid.draw(x)
    invisible(x)
}

makeContext.cellGrob <- function(x) {
    x$vp <- x$cellvp
    x
}

.grid_cell <- function(frame, grob, row, col, border, dynamic) {
    cell <- viewport(layout.pos.row = row, layout.pos.col = col)
    if (!is.null(border)) {
        inner <- viewport(x = border[2L], y = border[1L],
            width = .grid_one - (border[2L] + border[4L]),
            height = .grid_one - (border[1L] + border[3L]),
            just = c("left", "bottom"))
        cell <- vpStack(cell, inner)
    }
    addGrob(frame, gTree(col = col, row = row, border = border, dynamic = dynamic,
        cellvp = cell, children = gList(grob), cl = "cellGrob"))
}

placeGrob <- function(frame, grob, row = 1, col = 1) {
    if (is.null(frame$framevp$layout)) stop("the frame has no layout")
    .grid_cell(frame, grob, row, col, NULL, FALSE)
}

packGrob <- function(frame, grob, side = NULL, row = NULL, row.before = NULL,
                     row.after = NULL, col = NULL, col.before = NULL, col.after = NULL,
                     width = NULL, height = NULL, force.width = FALSE,
                     force.height = FALSE, border = NULL, dynamic = FALSE) {
    layout <- frame$framevp$layout
    nr <- if (is.null(layout)) 0L else layout$nrow
    nc <- if (is.null(layout)) 0L else layout$ncol
    w <- if (is.null(width)) grobWidth(grob) else .grid_coord(width, "npc")
    h <- if (is.null(height)) grobHeight(grob) else .grid_coord(height, "npc")
    if (!is.null(border)) {
        if (!is.unit(border) || length(border) != 4L) stop("invalid 'border'")
        w <- border[2L] + w + border[4L]
        h <- border[1L] + h + border[3L]
    }
    if (is.null(side) && is.null(row) && is.null(col) && is.null(row.before) &&
        is.null(row.after) && is.null(col.before) && is.null(col.after)) side <- "bottom"
    if (!is.null(side)) {
        side <- .grid_match_arg(side, c("top", "bottom", "left", "right"))
        if (side == "top") row.before <- 1L
        if (side == "bottom") row.after <- nr
        if (side == "left") col.before <- 1L
        if (side == "right") col.after <- nc
    }
    if (!is.null(row.before)) row <- row.before
    if (!is.null(row.after)) row <- row.after + 1L
    if (!is.null(col.before)) col <- col.before
    if (!is.null(col.after)) col <- col.after + 1L
    add.row <- nr == 0L || !is.null(row.before) || !is.null(row.after) || (!is.null(row) && max(row) > nr)
    add.col <- nc == 0L || !is.null(col.before) || !is.null(col.after) || (!is.null(col) && max(col) > nc)
    if (is.null(row)) row <- if (nr == 0L) 1 else c(1, nr)
    if (is.null(col)) col <- if (nc == 0L) 1 else c(1, nc)
    if (add.row) nr <- nr + 1L
    if (add.col) nc <- nc + 1L
    heights <- .grid_pack_tracks(layout$heights, h, row, nr, add.row, force.height)
    widths <- .grid_pack_tracks(layout$widths, w, col, nc, add.col, force.width)
    if (length(frame$children) && (add.row || add.col)) {
        children <- lapply(frame$children, function(child) {
            if (add.row) child$row <- child$row + (child$row >= row[1L])
            if (add.col) child$col <- child$col + (child$col >= col[1L])
            stack <- inherits(child$cellvp, "vpStack")
            previous <- if (stack) child$cellvp[[1L]] else child$cellvp
            cell <- viewport(layout.pos.row = child$row, layout.pos.col = child$col,
                             name = previous$name)
            if (stack) child$cellvp[[1L]] <- cell else child$cellvp <- cell
            child
        })
        frame <- .grid_children(frame, structure(children, class = "gList"))
    }
    frame$framevp$layout <- grid.layout(nr, nc, widths, heights)
    .grid_cell(frame, grob, row, col, border, dynamic)
}

.grid_pack_tracks <- function(tracks, size, index, n, insert, force) {
    if (is.null(tracks)) return(size)
    if (index[1L] < 1L || index[1L] > n) stop("invalid layout position")
    if (insert) {
        at <- index[1L]
        if (at == 1L) return(unit.c(size, tracks))
        if (at == n) return(unit.c(tracks, size))
        return(unit.c(tracks[seq_len(at - 1L)], size, tracks[seq.int(at, n - 1L)]))
    }
    at <- index[1L]
    if (all(index == at)) tracks[at] <- if (force) size else max(size, tracks[at])
    tracks
}

grid.place <- function(gPath, grob, row = 1, col = 1, redraw = TRUE) {
    .grid_update(gPath, function(x) placeGrob(x, grob, row, col), redraw = redraw)
    invisible(NULL)
}

grid.pack <- function(gPath, grob, redraw = TRUE, side = NULL, row = NULL,
                      row.before = NULL, row.after = NULL, col = NULL, col.before = NULL,
                      col.after = NULL, width = NULL, height = NULL, force.width = FALSE,
                      force.height = FALSE, border = NULL, dynamic = FALSE) {
    .grid_update(gPath, function(x) packGrob(x, grob, side, row, row.before, row.after,
        col, col.before, col.after, width, height, force.width, force.height, border, dynamic),
        redraw = redraw)
    invisible(NULL)
}


.grid_check_groups <- function(x, y, id, id.lengths) {
    if (length(x) != length(y)) stop("'x' and 'y' must be same length")
    if (!is.null(id) && length(id) != length(x)) stop("'x' and 'y' and 'id' must all be same length")
    if (!is.null(id.lengths) && sum(id.lengths) != length(x))
        stop("'x' and 'y' and 'id.lengths' must specify same overall length")
}


arrow <- function(angle = 30, length = unit(.25, "inches"),
                  ends = "last", type = "open") {
    if (!is.unit(length)) stop("'length' must be a unit object")
    ends <- .grid_match(.grid_match_arg(ends, c("first", "last", "both")), c("first", "last", "both"))
    type <- .grid_match(.grid_match_arg(type, c("open", "closed")), c("open", "closed"))
    structure(list(angle = as.numeric(angle), length = length, ends = ends, type = type),
              class = "arrow")
}

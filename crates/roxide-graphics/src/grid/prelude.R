.grid_match <- function(x, table) .Internal(C_grid_match(x, table))

.grid_flatten <- function(x) do.call(c, unname(x))

.grid_any_na <- function(x) any(is.na(x))
.grid_rep_len <- function(x, n) rep(x, length.out = n)
`%.grid_in%` <- function(x, table) !is.na(.grid_match(x, table))
.grid_intersect <- function(x, y) x[x %.grid_in% y]

.grid_match_arg <- function(x, choices) {
    if (length(x) != 1L || is.na(x)) stop("invalid argument")
    if (any(x == choices)) return(x)
    hits <- choices[startsWith(choices, x)]
    if (length(hits) == 1L) return(hits)
    stop(paste("'arg' should be one of", paste(choices, collapse = ", ")))
}

.grid_unit_codes <- c(0:24, 103:106, 201:203)
.grid_unit_names <- c(
    "npc", "cm", "inches", "lines", "native", "null", "snpc", "mm",
    "points", "picas", "bigpts", "dida", "cicero", "scaledpts", "strwidth",
    "strheight", "strascent", "strdescent", "char", "grobx", "groby",
    "grobwidth", "grobheight", "grobascent", "grobdescent",
    "mylines", "mychar", "mystrwidth", "mystrheight", "sum", "min", "max"
)
.grid_simple <- function(x, code, class.first = FALSE) {
    cls <- c("simpleUnit", "unit", "unit_v2")
    if (class.first) structure(x, class = cls, unit = code)
    else structure(x, unit = code, class = cls)
}

.grid_units <- function(x) structure(x, class = c("unit", "unit_v2"))

.grid_grob_data <- function(x) {
    if (is.character(x)) {
        if (length(x) != 1L || is.na(x) || grepl("::", x, fixed = TRUE))
            stop("'gPath' must have depth 1 in 'grobwidth/height' units")
        return(structure(list(path = NULL, name = x, n = 1L), class = c("gPath", "path")))
    }
    if (inherits(x, "gPath")) {
        if (x$n != 1L) stop("'gPath' must have depth 1 in 'grobwidth/height' units")
        return(x)
    }
    if (inherits(x, "grob")) return(x)
    stop("no 'grob' supplied for 'grobwidth/height' unit")
}

unit <- function(x, units, data = NULL) .Internal(C_grid_unit(x, units, data))

is.unit <- function(x) inherits(x, "unit")

.grid_as_unit <- function(x) {
    if (!is.unit(x)) stop("object is not coercible to a unit")
    x
}

.grid_items <- function(x) .Internal(C_grid_items(x))

unitType <- function(x, recurse = FALSE) .Internal(C_grid_unit_types(x, recurse))

unit.c <- function(...) {
    values <- list(...)
    if (!length(values)) stop("attempt to set an attribute on NULL")
    if (all(vapply(values, inherits, FALSE, "simpleUnit"))) {
        codes <- vapply(values, function(x) attr(x, "unit"), 0L)
        if (all(codes == codes[1L]))
            return(.grid_simple(.grid_flatten(lapply(values, as.vector)), codes[1L], TRUE))
    }
    .grid_units(.grid_flatten(lapply(values, .grid_items)))
}

`[.unit` <- function(x, index, ...) {
    if (missing(index)) return(x)
    if (is.numeric(index) && any(index > length(x)))
        stop('index out of bounds ("unit" subsetting)', call. = FALSE)
    y <- unclass(x)[index]
    if (!length(y))
        stop('Cannot create zero-length unit vector ("unit" subsetting)', call. = FALSE)
    if (inherits(x, "simpleUnit")) .grid_simple(y, attr(x, "unit"))
    else .grid_units(y)
}

`[<-.unit` <- function(x, index, value) {
    value <- .grid_as_unit(value)
    if (missing(index)) index <- seq_along(x)
    if (inherits(x, "simpleUnit") && inherits(value, "simpleUnit") &&
        identical(attr(x, "unit"), attr(value, "unit"))) {
        y <- as.vector(x)
        y[index] <- as.vector(value)
        return(.grid_simple(y, attr(x, "unit")))
    }
    y <- .grid_items(x)
    y[index] <- .grid_items(value)
    .grid_units(y)
}

length.unit <- function(x) length(unclass(x))
rep.unit <- function(x, ...) x[rep(seq_along(x), ...)]

unit.rep <- function(x, ...) {
    warning("'unit.rep' has been deprecated in favour of a unit method for the generic rep function")
    rep.unit(x, ...)
}

unit.length <- function(x) {
    warning("'unit.length' has been deprecated in favour of a unit method for the generic length function")
    length(x)
}

as.double.unit <- function(x, ...) vapply(.grid_items(x), function(it) it[[1L]], 0.0)
as.integer.unit <- function(x, ...) as.integer(as.double.unit(x))

.grid_unit_format <- function(x, render) {
    vapply(.grid_items(x), function(it) {
        v <- it[[1L]]
        code <- it[[3L]]
        name <- .grid_unit_names[.grid_match(code, .grid_unit_codes)]
        if (code < 201L) return(paste0(render(v), name))
        inner <- if (is.null(it[[2L]])) "" else paste(.grid_unit_format(it[[2L]], render), collapse = ", ")
        expr <- paste0(name, "(", inner, ")")
        if (!is.na(v) && v == 1) expr else paste0(render(v), "*", expr)
    }, "")
}

as.character.unit <- function(x, ...) .Internal(C_grid_unit_text(x))
format.unit <- function(x, ..., digits = getOption("digits")) {
    .grid_unit_format(x, function(v) {
        if (!is.null(digits)) v <- signif(v, digits)
        format(v, ...)
    })
}
print.unit <- function(x, ...) {
    print(format(x, ...), quote = FALSE)
    invisible(x)
}

.grid_scale_units <- function(x, factor) {
    if (!is.numeric(factor)) stop("non-unit operand must be numeric")
    if (!length(factor)) stop("Cannot create zero-length unit vector")
    if (inherits(x, "simpleUnit"))
        return(.grid_simple(as.vector(x) * factor, attr(x, "unit")))
    items <- .grid_items(x)
    n <- max(length(items), length(factor))
    items <- .grid_rep_len(items, n)
    factor <- .grid_rep_len(factor, n)
    for (i in seq_len(n)) items[[i]][[1L]] <- items[[i]][[1L]] * factor[i]
    .grid_units(items)
}

.grid_sum_terms <- function(item) {
    if (item[[3L]] == 201L && !is.null(item[[2L]]))
        .grid_items(.grid_scale_units(item[[2L]], item[[1L]]))
    else list(item)
}

.grid_summary <- function(values, code) {
    if (code == 201L) return(sum(values))
    if (code == 202L) return(min(values))
    max(values)
}

.grid_add <- function(a, b) .Internal(C_grid_add(a, b))

Ops.unit <- function(e1, e2) {
    if (missing(e2)) {
        if (.Generic == "+") return(e1)
        if (.Generic == "-") return(.grid_scale_units(e1, -1))
    } else {
        if (.Generic == "*") {
            if (is.unit(e1) && is.unit(e2)) stop("only one operand may be a unit")
            if (is.unit(e1)) return(.grid_scale_units(e1, e2))
            return(.grid_scale_units(e2, e1))
        }
        if (.Generic == "/" && is.unit(e1) && !is.unit(e2)) {
            if (!is.numeric(e2)) stop("non-unit operand must be numeric")
            return(.grid_scale_units(e1, 1 / e2))
        }
        if (.Generic == "/") {
            if (is.unit(e1) && is.unit(e2)) stop("only one operand may be a unit")
            stop("can't divide with a unit")
        }
        if (.Generic == "+" || .Generic == "-") {
            e1 <- .grid_as_unit(e1)
            e2 <- .grid_as_unit(e2)
            if (.Generic == "-") e2 <- .grid_scale_units(e2, -1)
            return(.grid_add(e1, e2))
        }
    }
    stop(paste("operator", .Generic, "not meaningful for units"))
}

Summary.unit <- function(..., na.rm = FALSE) {
    x <- unit.c(...)
    code <- .grid_match(.Generic, c("sum", "min", "max")) + 200L
    if (is.na(code)) stop(paste(.Generic, "not meaningful for units"))
    if (inherits(x, "simpleUnit")) {
        values <- as.vector(x)
        v <- .grid_summary(values, code)
        return(.grid_simple(v, attr(x, "unit"), TRUE))
    }
    items <- .grid_items(x)
    if (code == 201L)
        x <- .grid_units(.grid_flatten(lapply(items, .grid_sum_terms)))
    else x <- .grid_units(items)
    .grid_units(list(list(1, x, code)))
}

unit.pmin <- function(...) .Internal(C_grid_unit_extrema(list(...), 202L))
unit.pmax <- function(...) .Internal(C_grid_unit_extrema(list(...), 203L))

absolute.size <- function(unit) {
    if (inherits(unit, "simpleUnit") && attr(unit, "unit") %.grid_in% c(0L, 4L, 6L))
        return(.grid_simple(.grid_rep_len(1, length(unit)), 5L))
    if (inherits(unit, "simpleUnit")) return(unit)
    .grid_units(lapply(.grid_items(unit), function(item) {
        code <- item[[3L]]
        if (code %.grid_in% c(0L, 4L, 6L)) return(list(1, NULL, 5L))
        if (code >= 201L && !is.null(item[[2L]])) item[[2L]] <- absolute.size(item[[2L]])
        item
    }))
}

stringWidth <- function(string) unit(.grid_rep_len(1, length(string)), "strwidth", as.list(as.character(string)))
stringHeight <- function(string) unit(.grid_rep_len(1, length(string)), "strheight", as.list(as.character(string)))
stringAscent <- function(string) unit(.grid_rep_len(1, length(string)), "strascent", as.list(as.character(string)))
stringDescent <- function(string) unit(.grid_rep_len(1, length(string)), "strdescent", as.list(as.character(string)))

.grid_gp_names <- c("fill", "col", "lty", "lwd", "cex", "fontsize", "lineheight",
    "font", "fontfamily", "alpha", "lineend", "linejoin", "linemitre", "lex")

gpar <- function(...) {
    gp <- list(...)
    if (!length(gp)) return(structure(gp, class = "gpar"))
    known <- .grid_gp_names
    if (!is.null(gp$font) && !is.null(gp$fontface))
        stop("must specify only one of 'font' and 'fontface'")
    for (key in .grid_intersect(names(gp), c(known, "fontface"))) {
        value <- gp[[key]]
        if (is.null(value)) {
            gp[[key]] <- NULL
            next
        }
        if (!length(value)) stop(paste0("'gpar' element '", key, "' must not be length 0"))
        if (key %.grid_in% c("cex", "lwd", "lex", "fontsize", "lineheight", "linemitre", "alpha"))
            value <- as.numeric(value)
        if (key == "alpha" && any(value < 0 | value > 1)) stop("invalid 'alpha' value")
        if (key == "linemitre" && any(value < 1)) stop("invalid 'linemitre' value")
        if (key %.grid_in% c("cex", "lwd", "lex", "fontsize", "lineheight", "font", "fontface",
                       "fontfamily", "linemitre", "alpha")) {
            if (all(is.na(value))) {
                gp[[key]] <- NULL
                next
            }
            if (.grid_any_na(value)) stop(paste("mixture of missing and non-missing values for", key))
        }
        if (key == "fontfamily") value <- as.character(value)
        if (key == "fontface") {
            gp[[key]] <- NULL
            if (is.character(value)) {
                code <- .grid_match(value, c("plain", "bold", "italic", "bold.italic", "symbol"))
                if (.grid_any_na(code)) stop(paste("invalid fontface", value[is.na(code)][1L]))
                value <- setNames(code, value)
            } else value <- as.integer(value)
            gp$font <- value
        } else {
            if (key == "font") value <- as.integer(value)
            gp[[key]] <- value
        }
    }
    structure(gp, class = "gpar")
}

.grid_inherit_gp <- function(parent, gp) .Internal(C_grid_gp(parent, gp))

get.gpar <- function(names = NULL) {
    gp <- .grid_state()$current$gp
    if (is.null(names)) return(gp)
    if (any(!names %.grid_in% c(base::names(gp), "fontface")))
        stop("must specify only valid 'gpar' names")
    structure(unclass(gp)[names], class = "gpar")
}

convertUnit <- function(x, unitTo, axisFrom = "x", typeFrom = "location",
                        axisTo = axisFrom, typeTo = typeFrom, valueOnly = FALSE)
    .Internal(C_grid_convert(x, unitTo, axisFrom, typeFrom, axisTo, typeTo, NULL, valueOnly))

convertX <- function(x, unitTo, valueOnly = FALSE)
    .Internal(C_grid_convert(x, unitTo, "x", "location", "x", "location", NULL, valueOnly))

convertY <- function(x, unitTo, valueOnly = FALSE)
    .Internal(C_grid_convert(x, unitTo, "y", "location", "y", "location", NULL, valueOnly))

convertWidth <- function(x, unitTo, valueOnly = FALSE)
    .Internal(C_grid_convert(x, unitTo, "x", "dimension", "x", "dimension", NULL, valueOnly))

convertHeight <- function(x, unitTo, valueOnly = FALSE)
    .Internal(C_grid_convert(x, unitTo, "y", "dimension", "y", "dimension", NULL, valueOnly))

.grid_just <- function(just) {
    if (!length(just) || length(just) > 2L || .grid_any_na(just))
        stop("invalid justification")
    if (is.numeric(just)) {
        if (length(just) == 1L) return(c(as.numeric(just), 0.5))
        return(as.numeric(just))
    }
    horizontal <- c(left = 0, center = 0.5, centre = 0.5, right = 1)
    vertical <- c(bottom = 0, center = 0.5, centre = 0.5, top = 1)
    if (length(just) == 1L) {
        if (just %.grid_in% names(horizontal)) return(c(unname(horizontal[just]), 0.5))
        if (just %.grid_in% names(vertical)) return(c(0.5, unname(vertical[just])))
        stop("invalid justification")
    }
    x <- unname(horizontal[just[1L]])
    y <- unname(vertical[just[2L]])
    if (is.na(x)) stop("invalid horizontal justification")
    if (is.na(y)) stop("invalid vertical justification")
    c(x, y)
}

grid.layout <- function(nrow = 1, ncol = 1,
                        widths = unit(.grid_rep_len(1, ncol), "null"),
                        heights = unit(.grid_rep_len(1, nrow), "null"),
                        default.units = "null", respect = FALSE, just = "centre") {
    nrow <- as.integer(nrow)
    ncol <- as.integer(ncol)
    if (length(nrow) != 1L || is.na(nrow) || nrow < 1L ||
        length(ncol) != 1L || is.na(ncol) || ncol < 1L)
        stop("invalid layout dimensions")
    if (!is.unit(widths)) widths <- unit(widths, default.units)
    if (!is.unit(heights)) heights <- unit(heights, default.units)
    if (is.matrix(respect) && identical(dim(respect), c(nrow, ncol))) {
        marks <- matrix(as.integer(respect), nrow, ncol)
        respect <- 2
    } else if (is.logical(respect) && length(respect) == 1L && !is.na(respect)) {
        marks <- matrix(0L, nrow, ncol)
    } else stop("'respect' must be logical or an 'nrow' by 'ncol' matrix")
    structure(list(nrow = nrow, ncol = ncol, widths = widths, heights = heights,
        respect = respect, valid.respect = as.integer(respect), respect.mat = marks,
        just = just, valid.just = .grid_just(just)), class = "layout")
}

.grid_names <- new.env(parent = emptyenv())
.grid_names$viewport <- 0L

viewport <- function(x = 0.5, y = 0.5, width = 1, height = 1,
                     default.units = "npc", just = "centre", gp = gpar(),
                     clip = "inherit", xscale = c(0, 1), yscale = c(0, 1),
                     angle = 0, layout = NULL, layout.pos.row = NULL,
                     layout.pos.col = NULL, name = NULL, mask = "inherit",
                     hjust = NULL, vjust = NULL) {
    .Internal(C_grid_viewport(x, y, width, height, default.units, just, gp, clip,
        xscale, yscale, angle, layout, layout.pos.row, layout.pos.col, name, mask,
        hjust, vjust, .grid_names))
}

.grid_vp_group <- function(values, class) {
    if (!all(vapply(values, inherits, FALSE, "viewport"))) stop("only viewports allowed")
    structure(values, class = c(class, "viewport"))
}

vpList <- function(...) .grid_vp_group(list(...), "vpList")
vpStack <- function(...) .grid_vp_group(list(...), "vpStack")

vpTree <- function(parent, children) {
    if (!identical(class(parent), "viewport") || !inherits(children, "vpList"))
        stop("'parent' must be a viewport and 'children' must be a 'vpList' in 'vpTree'")
    structure(list(parent = parent, children = children), class = c("vpTree", "viewport"))
}

vpPath <- function(...) {
    pieces <- .grid_flatten(list(...))
    if (!length(pieces) || !is.character(pieces)) stop("invalid viewport name")
    pieces <- .grid_flatten(strsplit(pieces, "::", fixed = TRUE))
    n <- length(pieces)
    path <- if (n > 1L) paste(pieces[-n], collapse = "::") else NULL
    structure(list(path = path, name = pieces[n], n = as.integer(n)),
        class = c("vpPath", "path"))
}

as.character.vpPath <- function(x, ...) {
    if (is.null(x$path)) x$name else paste(x$path, x$name, sep = "::")
}

print.vpPath <- function(x, ...) {
    cat(as.character(x), "\n")
    invisible(x)
}

.grid_vp_label <- function(x) {
    if (inherits(x, "vpTree"))
        return(paste0(.grid_vp_label(x$parent), "->", .grid_vp_label(x$children)))
    if (inherits(x, "vpList"))
        return(paste0("(", paste(vapply(x, .grid_vp_label, ""), collapse = ", "), ")"))
    if (inherits(x, "vpStack"))
        return(paste(vapply(x, .grid_vp_label, ""), collapse = "->"))
    paste0("viewport[", x$name, "]")
}

print.viewport <- function(x, ...) {
    cat(.grid_vp_label(x), "\n")
    invisible(x)
}

.grid_state <- function() .Internal(C_grid_state())

.grid_select <- function(state, node) invisible(.Internal(C_grid_select(state, node)))

grid.newpage <- function(recording = TRUE) {
    .Internal(C_grid_page())
    state <- .grid_state()
    .grid_select(state, state$root)
}

.grid_rect <- function(x, y, width, height, transform) {
    corners <- cbind(c(x, x + width, x, x + width),
        c(y, y, y + height, y + height), 1) %*% transform
    c(min(corners[, 1L]), min(corners[, 2L]),
        max(corners[, 1L]), max(corners[, 2L]))
}

.grid_push <- function(vp, state) {
    if (!inherits(vp, "viewport") && !inherits(vp, "vpList") &&
        !inherits(vp, "vpStack") && !inherits(vp, "vpTree")) stop("only valid to push viewports")
    if (inherits(vp, "vpList")) {
        parent <- state$current
        for (child in vp) {
            state$current <- parent
            .grid_push(child, state)
        }
    } else if (inherits(vp, "vpStack")) {
        for (child in vp) .grid_push(child, state)
    } else if (inherits(vp, "vpTree")) {
        .grid_push(vp$parent, state)
        .grid_push(vp$children, state)
    } else {
        .Internal(C_grid_push(vp, state$current, state$root, state))
    }
    invisible(NULL)
}

pushViewport <- function(..., recording = TRUE) {
    state <- .grid_state()
    for (vp in list(...)) {
        .grid_push(vp, state)
        if (recording) .grid_record(state, "push", vp)
    }
    invisible(NULL)
}

current.viewport <- function() .grid_state()$current$vp

current.vpPath <- function() {
    path <- .grid_state()$current$path
    if (length(path)) vpPath(path) else NULL
}

.grid_vp_tree <- function(node) {
    if (length(node$children)) vpTree(node$vp,
        .grid_vp_group(lapply(node$children, .grid_vp_tree), "vpList"))
    else node$vp
}

current.vpTree <- function(all = TRUE) {
    state <- .grid_state()
    .grid_vp_tree(if (all) state$root else state$current)
}

current.parent <- function(n = 1) {
    n <- as.integer(n)
    node <- .grid_state()$current
    if (length(n) != 1L || is.na(n) || n < 1L || n > length(node$path) + 1L)
        stop("Invalid number of generations")
    for (i in seq_len(n)) node <- node$parent
    if (is.null(node)) NULL else node$vp
}

current.transform <- function() .grid_state()$current$transform
current.rotation <- function() .grid_state()$current$rotation

popViewport <- function(n = 1, recording = TRUE)
    invisible(.Internal(C_grid_up(n, TRUE, recording)))

upViewport <- function(n = 1, recording = TRUE)
    invisible(.Internal(C_grid_up(n, FALSE, recording)))

.grid_find <- function(node, path, strict) {
    if (!length(path)) return(node)
    child <- node$children[[path[1L]]]
    if (!is.null(child)) {
        result <- .grid_find(child, path[-1L], TRUE)
        if (!is.null(result)) return(result)
    }
    if (!strict && length(node$children)) {
        for (key in sort(names(node$children))) {
            result <- .grid_find(node$children[[key]], path, FALSE)
            if (!is.null(result)) return(result)
        }
    }
    NULL
}

.grid_path <- function(name) {
    if (!inherits(name, "vpPath")) name <- vpPath(name)
    strsplit(as.character(name), "::", fixed = TRUE)[[1L]]
}

.grid_down <- function(state, root, name, strict) {
    path <- .grid_path(name)
    found <- .grid_find(root, path, strict)
    if (is.null(found)) stop(paste0("Viewport '", paste(path, collapse = "::"), "' was not found"))
    .grid_select(state, found)
    invisible(as.integer(length(found$path) - length(root$path)))
}

downViewport <- function(name, strict = FALSE, recording = TRUE) {
    state <- .grid_state()
    distance <- .grid_down(state, state$current, name, strict)
    if (recording) .grid_record(state, "down", name)
    invisible(distance)
}

seekViewport <- function(name, recording = TRUE) {
    state <- .grid_state()
    .grid_select(state, state$root)
    distance <- .grid_down(state, state$root, name, FALSE)
    if (recording) .grid_record(state, "seek", name)
    invisible(distance)
}

grid.clip <- function(x = 0.5, y = 0.5, width = 1, height = 1,
                      just = "centre", hjust = NULL, vjust = NULL,
                      default.units = "npc", name = NULL, vp = NULL) {
    state <- .grid_state()
    if (!is.null(vp)) {
        if (!identical(class(vp), "viewport")) stop("invalid viewport")
        node <- .Internal(C_grid_push(vp, state$current, state$root, NULL))
    } else node <- state$current
    units <- lapply(list(x, y, width, height), function(v) {
        if (!is.unit(v)) v <- unit(v, default.units)
        if (length(v) != 1L)
            stop("'x', 'y', 'width', and 'height' must all be units of length 1")
        v
    })
    size <- c(.grid_inches(units[[3L]], node, "x", "dimension"),
              .grid_inches(units[[4L]], node, "y", "dimension"))
    anchor <- c(.grid_inches(units[[1L]], node, "x", "location"),
                .grid_inches(units[[2L]], node, "y", "location"))
    just <- .grid_just(just)
    if (!is.null(hjust)) just[1L] <- hjust
    if (!is.null(vjust)) just[2L] <- vjust
    origin <- anchor - just * size
    rect <- .grid_rect(origin[1L], origin[2L], size[1L], size[2L], node$transform)
    state$current$clip <- rect
    .Internal(C_grid_clip(rect))
    invisible(NULL)
}

.Internal(C_grid_s3(`[<-.unit`, as.double.unit, as.integer.unit, length.unit, rep.unit))

.grid_inches <- function(x, node, axis, type)
    .Internal(C_grid_convert(x, "in", axis, type, axis, type, node, TRUE))

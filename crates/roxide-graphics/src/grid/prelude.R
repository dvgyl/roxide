.grid_match <- function(x, table) .Internal(C_grid_match(x, table))

.grid_flatten <- function(x) do.call(c, unname(x))

.grid_any_na <- function(x) any(is.na(x))
.grid_rep_len <- function(x, n) rep(x, length.out = n)
`%.grid_in%` <- function(x, table) !is.na(.grid_match(x, table))
.grid_intersect <- function(x, y) x[x %.grid_in% y]

.grid_match_arg <- function(x, choices) {
    if (length(x) != 1L || is.na(x)) stop("invalid argument")
    if (x %.grid_in% choices) return(x)
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
.grid_aliases <- c(inch = 2L, `in` = 2L, pt = 8L, point = 8L,
    centimeter = 1L, centimeters = 1L, centimetre = 1L, centimetres = 1L,
    millimeter = 7L, millimeters = 7L, millimetre = 7L, millimetres = 7L)

.grid_codes <- function(units) {
    codes <- .grid_unit_codes[.grid_match(units, .grid_unit_names)]
    unknown <- is.na(codes)
    if (any(unknown)) codes[unknown] <- unname(.grid_aliases[units[unknown]])
    if (.grid_any_na(codes)) stop("Invalid unit")
    codes
}

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
    if (inherits(x, "gPath") && x$n == 1L || inherits(x, "grob")) return(x)
    stop("no 'grob' supplied for 'grobwidth/height' unit")
}

unit <- function(x, units, data = NULL) {
    if (!is.character(units)) stop("'units' must be character")
    if (!length(x) || !length(units)) stop("'x' and 'units' must have length > 0")
    codes <- .grid_codes(units)
    x <- as.numeric(x)
    if (length(codes) == 1L && !(codes %.grid_in% 14:17 || codes %.grid_in% 19:24))
        return(.grid_simple(x, codes))
    n <- max(length(x), length(codes))
    x <- .grid_rep_len(x, n)
    codes <- .grid_rep_len(codes, n)
    if (is.character(data) || inherits(data, "grob") || inherits(data, "gPath"))
        data <- list(data)
    values <- rep(list(NULL), n)
    for (i in seq_len(n)) {
        code <- codes[i]
        datum <- NULL
        if (code %.grid_in% c(14:17, 19:24)) {
            if (length(data)) {
                if (!is.list(data)) stop("'data' must be a list")
                datum <- data[[(i - 1L) %% length(data) + 1L]]
            }
            if (code %.grid_in% 14:17 && !is.character(datum))
                stop("no string supplied for 'strwidth/height' unit")
            if (code %.grid_in% 19:24) datum <- .grid_grob_data(datum)
        }
        values[[i]] <- list(x[i], datum, code)
    }
    .grid_units(values)
}

is.unit <- function(x) inherits(x, "unit")

.grid_as_unit <- function(x) {
    if (!is.unit(x)) stop("object is not coercible to a unit")
    x
}

.grid_items <- function(x) {
    x <- .grid_as_unit(x)
    if (!inherits(x, "simpleUnit")) return(unclass(x))
    code <- attr(x, "unit")
    lapply(as.vector(x), function(v) list(v, NULL, code))
}

unitType <- function(x, recurse = FALSE) {
    items <- .grid_items(x)
    codes <- vapply(items, function(it) it[[3L]], 0L)
    types <- .grid_unit_names[.grid_match(codes, .grid_unit_codes)]
    if (!recurse) return(types)
    setNames(lapply(seq_along(items), function(i) {
        if (codes[i] >= 201L && !is.null(items[[i]][[2L]]))
            unitType(items[[i]][[2L]], TRUE)
        else types[i]
    }), types)
}

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

as.character.unit <- function(x, ...) .grid_unit_format(x, as.character)
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

.grid_add_items <- function(a, b) {
    if (a[[3L]] == b[[3L]] && a[[3L]] < 201L && identical(a[[2L]], b[[2L]])) {
        a[[1L]] <- a[[1L]] + b[[1L]]
        return(a)
    }
    list(1, .grid_units(c(.grid_sum_terms(a), .grid_sum_terms(b))), 201L)
}

.grid_add <- function(a, b) {
    if (inherits(a, "simpleUnit") && inherits(b, "simpleUnit") &&
        identical(attr(a, "unit"), attr(b, "unit"))) {
        return(.grid_simple(as.vector(a) + as.vector(b), attr(a, "unit")))
    }
    a <- .grid_items(a)
    b <- .grid_items(b)
    n <- max(length(a), length(b))
    a <- .grid_rep_len(a, n)
    b <- .grid_rep_len(b, n)
    .grid_units(lapply(seq_len(n), function(i) .grid_add_items(a[[i]], b[[i]])))
}

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
        if (.Generic %.grid_in% c("+", "-")) {
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

.grid_unit_extrema <- function(values, code) {
    if (!length(values)) return(.grid_units(list()))
    n <- max(vapply(values, length, 0L))
    op <- if (code == 202L) min else max
    codes <- vapply(values, function(x) {
        if (inherits(x, "simpleUnit")) attr(x, "unit") else as.integer(NA)
    }, 0L)
    if (!.grid_any_na(codes) && all(codes == codes[1L])) {
        values <- lapply(values, function(x) .grid_rep_len(as.vector(x), n))
        result <- vapply(seq_len(n), function(i) {
            op(vapply(values, function(x) x[i], 0.0))
        }, 0.0)
        return(.grid_simple(result, codes[1L], TRUE))
    }
    values <- lapply(values, function(x) .grid_rep_len(.grid_items(x), n))
    result <- rep(list(NULL), n)
    # A mismatch makes all remaining positions composite units.
    same <- TRUE
    for (i in seq_len(n)) {
        items <- lapply(values, function(x) x[[i]])
        first <- items[[1L]]
        if (same) {
            same <- first[[3L]] < 201L && all(vapply(items, function(item) {
                item[[3L]] == first[[3L]] && identical(item[[2L]], first[[2L]])
            }, FALSE))
        }
        if (same) {
            first[[1L]] <- op(vapply(items, function(item) item[[1L]], 0.0))
            result[[i]] <- first
        } else result[[i]] <- list(1, .grid_units(items), code)
    }
    .grid_units(result)
}

unit.pmin <- function(...) .grid_unit_extrema(list(...), 202L)
unit.pmax <- function(...) .grid_unit_extrema(list(...), 203L)

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

.grid_default_gp <- function(size) {
    structure(list(fill = "transparent", col = "black", lty = "solid",
        lwd = 1, cex = 1, fontsize = size, lineheight = 1.2, font = 1L,
        fontfamily = "", alpha = 1, lineend = "round", linejoin = "round",
        linemitre = 10, lex = 1), class = "gpar")
}

gpar <- function(...) {
    gp <- list(...)
    known <- names(.grid_default_gp(12))
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

.grid_inherit_gp <- function(parent, gp) {
    for (key in .grid_intersect(names(parent), names(gp))) {
        if (key %.grid_in% c("cex", "lex", "alpha"))
            parent[[key]] <- parent[[key]] * gp[[key]]
        else parent[[key]] <- gp[[key]]
    }
    parent
}

get.gpar <- function(names = NULL) {
    gp <- .grid_state()$current$gp
    if (is.null(names)) return(gp)
    if (any(!names %.grid_in% c(base::names(gp), "fontface")))
        stop("must specify only valid 'gpar' names")
    structure(unclass(gp)[names], class = "gpar")
}

.grid_recycle <- function(x, i) x[(i - 1L) %% length(x) + 1L]

.grid_font_params <- function(gp, i) {
    c(.grid_recycle(gp$fontsize, i), .grid_recycle(gp$cex, i),
      .grid_recycle(gp$font, i), .grid_recycle(gp$lineheight, i))
}

.grid_inches_per_unit <- c(`1` = 1 / 2.54, `2` = 1, `7` = 1 / 25.4,
    `8` = 1 / 72.27, `9` = 12 / 72.27, `10` = 1 / 72,
    `11` = (1238 / 1157) / 72.27, `12` = (12 * 1238 / 1157) / 72.27,
    `13` = 1 / (65536 * 72.27))

.grid_scale <- function(node, axis) {
    if (axis == 1L) node$vp$xscale else node$vp$yscale
}

.grid_measure <- function(item, node, axis, position, i, gp = node$gp, mode = 201L) {
    code <- item[[3L]]
    value <- item[[1L]]
    if (code >= 201L) {
        terms <- vapply(.grid_items(item[[2L]]), .grid_measure, 0.0,
            node = node, axis = axis, position = position, i = i, gp = gp, mode = code)
        return(value * .grid_summary(terms, code))
    }
    if (code == 0L) return(value * node$size[axis])
    if (code == 4L) {
        scale <- .grid_scale(node, axis)
        if (position) value <- value - scale[1L]
        return(value / (scale[2L] - scale[1L]) * node$size[axis])
    }
    if (code == 5L) return(.grid_null_inches(mode, node$size[axis]))
    if (code == 6L) return(value * min(node$size))
    physical <- .grid_inches_per_unit[as.character(code)]
    if (!is.na(physical)) return(value * unname(physical))
    if (code %.grid_in% 19:24) return(.grid_grob_measure(item, node, axis, position, i))
    if (code %.grid_in% 103:106) gp <- node$gp
    font <- .grid_font_params(gp, i)
    if (code %.grid_in% c(3L, 18L, 103L, 104L)) {
        em <- font[1L] * font[2L] / 72
        if (code %.grid_in% c(3L, 103L)) em <- em * font[4L]
        return(value * em)
    }
    if (code %.grid_in% c(14:17, 105:106))
        return(value * .Internal(C_grid_metrics(code, item[[2L]], font,
            .grid_recycle(gp$fontfamily, i))))
    stop("invalid unit or unit not yet implemented")
}

.grid_grob_measure <- function(item, node, axis, position, i) {
    stop("grob units require a grob measurement method")
}

.grid_fraction <- function(x, size) {
    if (abs(size) < 1e-6) {
        if (isTRUE(x == 0)) return(0)
        stop("Viewport has zero dimension(s)")
    }
    x / size
}

.grid_inverse <- function(inches, code, node, axis, position, i) {
    if (code == 0L) return(.grid_fraction(inches, node$size[axis]))
    if (code == 4L) {
        if (abs(node$size[axis]) < 1e-6 && isTRUE(inches == 0)) return(0)
        scale <- .grid_scale(node, axis)
        x <- .grid_fraction(inches, node$size[axis]) * (scale[2L] - scale[1L])
        if (position) x <- x + scale[1L]
        return(x)
    }
    if (code == 6L) return(.grid_fraction(inches, min(node$size)))
    if (!code %.grid_in% c(1:3, 7:13, 18L)) stop("invalid unit or unit not yet implemented")
    inches / .grid_measure(list(1, NULL, code), node, axis, position, i)
}

convertUnit <- function(x, unitTo, axisFrom = "x", typeFrom = "location",
                        axisTo = axisFrom, typeTo = typeFrom, valueOnly = FALSE) {
    from <- .grid_match_arg(axisFrom, c("x", "y"))
    to <- .grid_match_arg(axisTo, c("x", "y"))
    location.from <- .grid_match_arg(typeFrom, c("location", "dimension")) == "location"
    location.to <- .grid_match_arg(typeTo, c("location", "dimension")) == "location"
    from <- .grid_match(from, c("x", "y"))
    to <- .grid_match(to, c("x", "y"))
    if (!is.unit(x)) stop("Not a unit object")
    items <- .grid_items(x)
    if (!length(unitTo)) stop("invalid unit or unit not yet implemented")
    codes <- .grid_rep_len(.grid_codes(unitTo), length(items))
    node <- .grid_state()$current
    values <- vapply(seq_along(items), function(i) {
        a <- items[[i]][[3L]]
        b <- codes[i]
        if (a %.grid_in% c(0L, 4L) && b %.grid_in% c(0L, 4L) && from == to) {
            v <- items[[i]][[1L]]
            scale <- .grid_scale(node, from)
            delta <- scale[2L] - scale[1L]
            if (a == 4L) v <- (v - if (location.from) scale[1L] else 0) / delta
            if (b == 4L) v <- v * delta + if (location.to) scale[1L] else 0
            return(v)
        }
        inches <- .grid_measure(items[[i]], node, from, location.from, i)
        .grid_inverse(inches, b, node, to, location.to, i)
    }, 0.0)
    if (valueOnly) values
    else unit(values, unitTo)
}

convertX <- function(x, unitTo, valueOnly = FALSE)
    convertUnit(x, unitTo, "x", "location", valueOnly = valueOnly)

convertY <- function(x, unitTo, valueOnly = FALSE)
    convertUnit(x, unitTo, "y", "location", valueOnly = valueOnly)

convertWidth <- function(x, unitTo, valueOnly = FALSE)
    convertUnit(x, unitTo, "x", "dimension", valueOnly = valueOnly)

convertHeight <- function(x, unitTo, valueOnly = FALSE)
    convertUnit(x, unitTo, "y", "dimension", valueOnly = valueOnly)

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

# Use the grid null-unit sentinel for each summary operation.
.grid_null_inches <- function(mode, size) {
    if (mode == 202L) return(size * 2.54)
    if (mode == 203L) return(2.2250738585072014e-308)
    0
}

.grid_null_weight <- function(item) {
    code <- item[[3L]]
    value <- item[[1L]]
    if (code == 5L) return(value)
    if (code < 201L || is.null(item[[2L]])) return(as.numeric(NA))
    weights <- vapply(.grid_items(item[[2L]]), .grid_null_weight, 0.0)
    value * .grid_summary(weights, code)
}

.grid_layout_axis <- function(x, n, node, axis) {
    items <- .grid_items(x)
    items <- .grid_rep_len(items, n)
    weights <- vapply(items, .grid_null_weight, 0.0)
    fixed <- numeric(n)
    for (i in seq_along(weights)[is.na(weights)]) fixed[i] <- .grid_measure(items[[i]], node, axis, FALSE, i)
    list(fixed = fixed, weights = weights)
}

.grid_layout_size <- function(node) {
    layout <- node$vp$layout
    if (is.null(layout)) return(NULL)
    w <- .grid_layout_axis(layout$widths, layout$ncol, node, 1L)
    h <- .grid_layout_axis(layout$heights, layout$nrow, node, 2L)
    mark.cols <- .grid_rep_len(layout$valid.respect == 1L, layout$ncol)
    mark.rows <- .grid_rep_len(layout$valid.respect == 1L, layout$nrow)
    if (layout$valid.respect == 2L) {
        mark.cols <- colSums(layout$respect.mat != 0L) > 0
        mark.rows <- rowSums(layout$respect.mat != 0L) > 0
    }
    tracks <- .Internal(C_grid_layout_tracks(node$size,
        w$fixed, w$weights, mark.cols, h$fixed, h$weights, mark.rows))
    list(widths = tracks[seq_len(layout$ncol)],
         heights = tracks[layout$ncol + seq_len(layout$nrow)])
}

.grid_layout_region <- function(vp, parent) {
    tracks <- parent$tracks
    rows <- vp$valid.pos.row
    cols <- vp$valid.pos.col
    if (is.null(tracks) || is.null(rows) && is.null(cols)) return(NULL)
    if (is.null(rows)) rows <- c(1L, length(tracks$heights))
    if (is.null(cols)) cols <- c(1L, length(tracks$widths))
    if (rows[1L] < 1L || rows[2L] > length(tracks$heights)) stop("invalid 'layout.pos.row'")
    if (cols[1L] < 1L || cols[2L] > length(tracks$widths)) stop("invalid 'layout.pos.col'")
    width <- sum(tracks$widths[seq.int(cols[1L], cols[2L])])
    height <- sum(tracks$heights[seq.int(rows[1L], rows[2L])])
    total <- c(sum(tracks$widths), sum(tracks$heights))
    offset <- parent$vp$layout$valid.just * (parent$size - total)
    left <- offset[1L] + sum(tracks$widths[seq_len(cols[1L] - 1L)])
    bottom <- offset[2L] + total[2L] - sum(tracks$heights[seq_len(rows[2L])])
    c(left, bottom, width, height)
}

.grid_names <- new.env(parent = emptyenv())
.grid_names$viewport <- 0L

.grid_position <- function(index) {
    if (is.null(index)) return(NULL)
    if (!length(index) || any(!is.finite(index))) stop("invalid layout position")
    as.integer(range(index))
}

viewport <- function(x = 0.5, y = 0.5, width = 1, height = 1,
                     default.units = "npc", just = "centre", gp = gpar(),
                     clip = "inherit", xscale = c(0, 1), yscale = c(0, 1),
                     angle = 0, layout = NULL, layout.pos.row = NULL,
                     layout.pos.col = NULL, name = NULL, mask = "inherit",
                     hjust = NULL, vjust = NULL) {
    coords <- lapply(list(x, y, width, height), function(v) {
        if (is.unit(v)) v else unit(v, default.units)
    })
    if (any(vapply(coords, length, 0L) != 1L))
        stop("'x', 'y', 'width', and 'height' must all be units of length 1")
    if (!inherits(gp, "gpar")) stop("invalid 'gp' value")
    if (!is.null(layout) && !inherits(layout, "layout")) stop("invalid 'layout' value")
    if (length(xscale) != 2L || any(!is.finite(xscale)) || xscale[1L] == xscale[2L])
        stop("invalid 'xscale' in viewport")
    if (length(yscale) != 2L || any(!is.finite(yscale)) || yscale[1L] == yscale[2L])
        stop("invalid 'yscale' in viewport")
    if (length(angle) != 1L || !is.finite(angle)) stop("invalid 'angle' in viewport")
    just <- .grid_just(just)
    if (!is.null(hjust)) just[1L] <- as.numeric(hjust)[1L]
    if (!is.null(vjust)) just[2L] <- as.numeric(vjust)[1L]
    row <- .grid_position(layout.pos.row)
    col <- .grid_position(layout.pos.col)
    if (is.character(clip))
        clip <- c(inherit = FALSE, on = TRUE, off = NA)[[.grid_match_arg(clip, c("inherit", "on", "off"))]]
    if (!is.logical(clip) || length(clip) != 1L) stop("invalid 'clip' value")
    if (is.character(mask)) mask <- .grid_match_arg(mask, c("inherit", "none")) == "inherit"
    if (is.null(name)) {
        .grid_names$viewport <- .grid_names$viewport + 1L
        name <- paste0("GRID.VP.", .grid_names$viewport)
    }
    if (!is.character(name) || length(name) != 1L || is.na(name) || !nzchar(name))
        stop("invalid viewport name")
    structure(list(x = coords[[1L]], y = coords[[2L]], width = coords[[3L]],
        height = coords[[4L]], justification = just, gp = gp, clip = clip,
        xscale = as.numeric(xscale), yscale = as.numeric(yscale),
        angle = as.numeric(angle), layout = layout,
        layout.pos.row = row, layout.pos.col = col, valid.just = just,
        valid.pos.row = row, valid.pos.col = col, name = name, parentgpar = NULL,
        gpar = NULL, trans = NULL, widths = NULL, heights = NULL, width.cm = NULL,
        height.cm = NULL, rotation = NULL, cliprect = NULL, parent = NULL,
        children = NULL, devwidth = NULL, devheight = NULL, clippath = NULL,
        mask = mask, resolvedmask = NULL), class = "viewport")
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

.grid_root <- function(size) {
    node <- new.env(parent = emptyenv())
    node$vp <- viewport(clip = TRUE, xscale = c(0, size[1L] * 72),
        yscale = c(0, size[2L] * 72), name = "ROOT", mask = "none")
    node$size <- size[1:2]
    node$gp <- .grid_default_gp(size[3L])
    node$transform <- diag(3L)
    node$rotation <- 0
    node$parent <- NULL
    node$children <- list()
    node$path <- character()
    node$clip <- c(0, 0, node$size)
    node$tracks <- NULL
    node
}

.grid_state <- function() {
    state <- .Internal(C_grid_state())
    if (is.null(state)) {
        state <- new.env(parent = emptyenv())
        state$root <- .grid_root(.Internal(C_grid_device()))
        state$current <- state$root
        .Internal(C_grid_set_state(state))
    }
    state
}

.grid_select <- function(state, node) {
    .Internal(C_grid_clip(node$clip))
    state$current <- node
    invisible(NULL)
}

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

.grid_node <- function(vp, parent, root) {
    region <- .grid_layout_region(vp, parent)
    angle <- vp$angle * 0.017453292519943295
    rotation <- matrix(c(cos(angle), sin(angle), -sin(angle), cos(angle)), 2L, byrow = TRUE)
    if (is.null(region)) {
        size <- c(.grid_measure(.grid_items(vp$width)[[1L]], parent, 1L, FALSE, 1L),
                  .grid_measure(.grid_items(vp$height)[[1L]], parent, 2L, FALSE, 1L))
        anchor <- c(.grid_measure(.grid_items(vp$x)[[1L]], parent, 1L, TRUE, 1L),
                    .grid_measure(.grid_items(vp$y)[[1L]], parent, 2L, TRUE, 1L))
        origin <- anchor - as.vector((vp$valid.just * size) %*% rotation)
    } else {
        size <- region[3:4]
        origin <- region[1:2]
    }
    if (any(!is.finite(c(size, origin)))) stop("non-finite location and/or size for viewport")
    trans <- matrix(c(rotation[1L, ], 0, rotation[2L, ], 0, origin, 1), 3L, byrow = TRUE)
    node <- new.env(parent = emptyenv())
    node$vp <- vp
    node$parent <- parent
    node$children <- list()
    node$path <- c(parent$path, vp$name)
    node$size <- size
    node$transform <- trans %*% parent$transform
    node$rotation <- parent$rotation + vp$angle
    node$gp <- .grid_inherit_gp(parent$gp, vp$gp)
    .Internal(C_grid_colors(node$gp$col[1L], node$gp$fill[1L]))
    node$tracks <- .grid_layout_size(node)
    node$clip <- parent$clip
    if (is.na(vp$clip)) node$clip <- root$clip
    if (isTRUE(vp$clip)) {
        if (node$rotation %% 90 != 0) warning("cannot clip to rotated viewport")
        else node$clip <- .grid_rect(0, 0, size[1L], size[2L], node$transform)
    }
    node
}

.grid_push <- function(vp, state) {
    if (!inherits(vp, "viewport")) stop("only valid to push viewports")
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
        parent <- state$current
        node <- .grid_node(vp, parent, state$root)
        parent$children[[vp$name]] <- node
        .grid_select(state, node)
    }
    invisible(NULL)
}

pushViewport <- function(..., recording = TRUE) {
    state <- .grid_state()
    for (vp in list(...)) .grid_push(vp, state)
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

.grid_up <- function(n, remove) {
    state <- .grid_state()
    node <- state$current
    depth <- length(node$path)
    n <- as.integer(n)
    if (length(n) != 1L || is.na(n) || n < 0L || n > depth)
        stop("cannot pop the top-level viewport")
    if (n == 0L) n <- depth
    if (n == 0L) return(NULL)
    path <- vpPath(node$path[seq.int(depth - n + 1L, depth)])
    for (i in seq_len(n)) {
        child <- node
        node <- node$parent
    }
    if (remove) node$children[[child$vp$name]] <- NULL
    .grid_select(state, node)
    path
}

popViewport <- function(n = 1, recording = TRUE) {
    .grid_up(n, TRUE)
    invisible(NULL)
}

upViewport <- function(n = 1, recording = TRUE) invisible(.grid_up(n, FALSE))

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
    invisible(.grid_down(state, state$current, name, strict))
}

seekViewport <- function(name, recording = TRUE) {
    state <- .grid_state()
    .grid_select(state, state$root)
    invisible(.grid_down(state, state$root, name, FALSE))
}

grid.clip <- function(x = 0.5, y = 0.5, width = 1, height = 1,
                      just = "centre", hjust = NULL, vjust = NULL,
                      default.units = "npc", name = NULL, vp = NULL) {
    state <- .grid_state()
    if (!is.null(vp)) {
        if (!identical(class(vp), "viewport")) stop("invalid viewport")
        node <- .grid_node(vp, state$current, state$root)
    } else node <- state$current
    units <- lapply(list(x, y, width, height), function(v) {
        if (!is.unit(v)) v <- unit(v, default.units)
        if (length(v) != 1L)
            stop("'x', 'y', 'width', and 'height' must all be units of length 1")
        .grid_items(v)[[1L]]
    })
    size <- c(.grid_measure(units[[3L]], node, 1L, FALSE, 1L),
              .grid_measure(units[[4L]], node, 2L, FALSE, 1L))
    anchor <- c(.grid_measure(units[[1L]], node, 1L, TRUE, 1L),
                .grid_measure(units[[2L]], node, 2L, TRUE, 1L))
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

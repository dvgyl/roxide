
c.Date <- function(..., recursive = FALSE) {
    values <- lapply(list(...), function(x) if (is.null(x)) NULL else unclass(as.Date(x)))
    result <- do.call("c", values)
    if (is.null(result)) result <- numeric()
    structure(result, class = "Date")
}
c.POSIXct <- function(..., recursive = FALSE) {
    values <- lapply(list(...), function(x) if (is.null(x)) NULL else as.POSIXct(x))
    zones <- lapply(values[!vapply(values, is.null, FALSE)], attr, "tzone", exact = TRUE)
    if (!length(zones)) return(.rox_posixct(integer(), ""))
    tz <- zones[[1L]]
    if (!all(vapply(zones, identical, FALSE, tz))) tz <- NULL
    .rox_posixct(do.call("c", lapply(values, unclass)), tz)
}
c.POSIXlt <- function(..., recursive = FALSE) as.POSIXlt(c.POSIXct(...))

.rox_posixct <- function(x, tz, classes = c("POSIXct", "POSIXt"))
    structure(x, class = classes, tzone = tz)

.rox_datetime_offset <- function(x, units) {
    if (!inherits(x, "difftime")) return(x)
    units(x) <- units
    as.double(x)
}

`+.Date` <- function(e1, e2) {
    if (missing(e2)) return(e1)
    if (inherits(e1, "Date") && inherits(e2, "Date"))
        stop('binary + is not defined for "Date" objects')
    if (inherits(e1, "Date")) {
        result <- unclass(e1) + .rox_datetime_offset(e2, "days")
    } else {
        result <- .rox_datetime_offset(e1, "days") + unclass(e2)
    }
    structure(result, class = "Date")
}
`-.Date` <- function(e1, e2) {
    if (missing(e2)) stop('unary - is not defined for "Date" objects')
    if (!inherits(e1, "Date")) stop('can only subtract from "Date" objects')
    if (inherits(e2, "Date"))
        return(structure(unclass(e1) - unclass(e2), class = "difftime", units = "days"))
    structure(unclass(e1) - .rox_datetime_offset(e2, "days"), class = "Date")
}
`+.POSIXt` <- function(e1, e2) {
    if (missing(e2)) return(e1)
    if (inherits(e1, "POSIXt") && inherits(e2, "POSIXt"))
        stop("binary '+' is not defined for \"POSIXt\" objects")
    if (inherits(e1, "POSIXt")) {
        x <- as.POSIXct(e1)
        result <- unclass(x) + .rox_datetime_offset(e2, "secs")
    } else {
        x <- as.POSIXct(e2)
        result <- .rox_datetime_offset(e1, "secs") + unclass(x)
    }
    .rox_posixct(result, attr(x, "tzone", exact = TRUE))
}
`-.POSIXt` <- function(e1, e2) {
    if (missing(e2)) stop("unary '-' is not defined for \"POSIXt\" objects")
    if (!inherits(e1, "POSIXt")) stop('can only subtract from "POSIXt" objects')
    if (inherits(e2, "POSIXt")) return(difftime(e1, e2))
    x <- as.POSIXct(e1)
    .rox_posixct(unclass(x) - .rox_datetime_offset(e2, "secs"), attr(x, "tzone", exact = TRUE))
}

`[.Date` <- function(x, ..., drop = TRUE)
    structure(.subset(unclass(x), ..., drop = drop), class = class(x))
`[[.Date` <- function(x, ...)
    structure(.subset2(unclass(x), ...), class = class(x))
`[.POSIXct` <- function(x, ..., drop = TRUE)
    .rox_posixct(.subset(unclass(x), ..., drop = drop), attr(x, "tzone", exact = TRUE), class(x))
`[[.POSIXct` <- function(x, ...)
    .rox_posixct(.subset2(unclass(x), ...), attr(x, "tzone", exact = TRUE), class(x))

`[<-.Date` <- function(x, ..., value) {
    if (!length(value)) return(x)
    result <- unclass(x)
    result[...] <- unclass(as.Date(value))
    structure(result, class = class(x))
}
`[<-.POSIXct` <- function(x, ..., value) {
    if (!length(value)) return(x)
    result <- unclass(x)
    result[...] <- unclass(as.POSIXct(value))
    .rox_posixct(result, attr(x, "tzone", exact = TRUE), class(x))
}

length.POSIXlt <- function(x) max(lengths(unclass(x)), 0L)
names.POSIXlt <- function(x) names(.subset2(unclass(x), "year"))
`names<-.POSIXlt` <- function(x, value) {
    fields <- unclass(x)
    year <- fields[["year"]]
    n <- length(x)
    if (length(year) != n && length(year)) year <- rep(year, length.out = n)
    names(year) <- value
    fields[["year"]] <- year
    attr(fields, "balanced") <- if (isTRUE(attr(x, "balanced", exact = TRUE))) NA else NULL
    structure(fields, class = class(x))
}
is.na.POSIXlt <- function(x) is.na(as.POSIXct(x))

.rox_lt_rows <- function(x, i, single = FALSE) {
    fields <- unclass(x)
    n <- length(x)
    positions <- seq_len(n)
    names(positions) <- names(x)
    if (missing(i)) return(x)
    if (single) positions <- positions[[i]] else positions <- positions[i]
    result <- lapply(fields, function(field) {
        if (single) field[[positions]] else field[positions]
    })
    balanced <- if (anyNA(positions) || any(lengths(fields) != n)) {
        NA
    } else attr(x, "balanced", exact = TRUE)
    structure(result, class = class(x),
              tzone = attr(x, "tzone", exact = TRUE), balanced = balanced)
}
`[.POSIXlt` <- function(x, i, j, ..., drop = TRUE) {
    if (missing(j)) return(.rox_lt_rows(x, i))
    if (!is.character(j) || length(j) != 1L)
        stop("component subscript must be a character string")
    fields <- unclass(x)
    field <- .subset2(fields, j)
    if (missing(i) || is.null(field)) return(field)
    if (is.character(i)) i <- match(i, names(x))
    field[i]
}
`[[.POSIXlt` <- function(x, i, ...) .rox_lt_rows(x, i, single = TRUE)
`[<-.POSIXlt` <- function(x, i, j, ..., value) {
    if (!missing(j)) {
        if (!is.character(j) || length(j) != 1L)
            stop("component subscript must be a character string")
        fields <- unclass(x)
        if (missing(i)) fields[[j]] <- value else fields[[j]][i] <- value
        return(structure(fields, class = class(x)))
    }
    if (!length(value)) return(x)
    result <- as.POSIXct(x)
    result[i] <- value
    structure(as.POSIXlt(result), class = class(x))
}

rep.Date <- function(x, ...)
    structure(rep(unclass(x), ...), class = class(x))
rep.POSIXct <- function(x, ...)
    .rox_posixct(rep(unclass(x), ...), attr(x, "tzone", exact = TRUE), class(x))
rep.POSIXlt <- function(x, ...) x[rep(seq_len(length(x)), ...)]

.rep_int_datetime <- function(x, times) {
    x[rep.int(seq_len(length(x)), times)]
}
rep.int.Date <- rep.int.POSIXct <- rep.int.POSIXlt <- .rep_int_datetime

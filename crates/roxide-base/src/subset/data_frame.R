
I <- function(x) {
    class(x) <- unique(c("AsIs", attr(x, "class", exact = TRUE)))
    x
}

as.data.frame <- function(x, row.names = NULL, optional = FALSE, ...)
    UseMethod("as.data.frame")

.set_row_names <- function(n) if (n > 0L) c(NA_integer_, -n) else integer()

row.names <- function(x) UseMethod("row.names")
row.names.default <- function(x) rownames(x)
row.names.data.frame <- function(x) as.character(attr(x, "row.names", exact = TRUE))

`row.names<-` <- function(x, value) UseMethod("row.names<-")
`row.names<-.default` <- function(x, value) {
    rownames(x) <- value
    x
}
`row.names<-.data.frame` <- function(x, value) {
    n <- nrow(x)
    if (is.null(value)) {
        value <- .set_row_names(n)
    } else {
        if (length(value) != n) stop("invalid 'row.names' length")
        if (!is.integer(value)) value <- as.character(value)
        if (anyDuplicated(value)) {
            duplicates <- sort(unique(value[duplicated(value)]))
            kind <- if (length(duplicates) == 1L) "value" else "values"
            warning("non-unique ", kind, " when setting 'row.names': ",
                    paste(paste0("‘", duplicates, "’"), collapse = ", "))
            stop("duplicate 'row.names' are not allowed")
        }
        if (anyNA(value)) stop("missing values in 'row.names' are not allowed")
    }
    attr(x, "row.names") <- value
    x
}
.row_names_info <- function(x, type = 1L) .Internal(rox_row_names_info(x, type))

data.frame <- function(..., row.names = NULL, check.rows = FALSE, check.names = TRUE,
                       fix.empty.names = TRUE, stringsAsFactors = FALSE) {
    x <- list(...)
    expressions <- as.list(substitute(list(...)))[-1L]
    .frame_list(x, expressions, row.names, missing(row.names), check.rows,
                check.names, fix.empty.names, stringsAsFactors)
}

as.data.frame.NULL <- function(x, row.names = NULL, optional = FALSE, ...)
    structure(list(), names = character(), row.names = integer(), class = "data.frame")

as.data.frame.default <- function(x, ..., nm = deparse1(substitute(x))) {
    if (!is.null(x) && is.atomic(x)) return(as.data.frame.vector(x, ..., nm = nm))
    stop("cannot coerce class ‘", deparse1(class(x)), "’ to a data.frame")
}

as.data.frame.data.frame <- function(x, row.names = NULL, ...) {
    classes <- class(x)
    first <- match("data.frame", classes)
    if (first > 1L) class(x) <- classes[seq.int(first, length(classes))]
    if (!is.null(row.names)) {
        n <- nrow(x)
        if (length(row.names) != n)
            stop("invalid 'row.names', length ", length(row.names), " for a data frame with ", n, " rows")
        if (!(is.character(row.names) || is.integer(row.names)))
            stop("row names must be 'character' or 'integer', not '", typeof(row.names), "'")
        attr(x, "row.names") <- row.names
    }
    x
}

.frame_vector <- function(x, row.names, optional, nm, stringsAsFactors = FALSE, keep.names = FALSE) {
    n <- length(x)
    if (is.null(row.names)) {
        labels <- names(x)
        row.names <- if (!n) character() else if (length(labels) && !anyDuplicated(labels)) {
            labels
        } else .set_row_names(n)
    } else if (!(is.character(row.names) || is.integer(row.names)) || length(row.names) != n) {
        stop("'row.names' is not a character or integer vector of length ", n)
    }
    if (stringsAsFactors) x <- factor(x)
    if (!keep.names) names(x) <- NULL
    columns <- list(x)
    if (!optional) names(columns) <- nm
    structure(columns, row.names = row.names, class = "data.frame")
}

as.data.frame.vector <- function(x, row.names = NULL, optional = FALSE, ..., nm = deparse1(substitute(x)))
    .frame_vector(x, row.names, optional, nm)

.frame_legacy_method <- function(method) {
    force(method)
    function(x, row.names = NULL, optional = FALSE, ..., nm = deparse1(substitute(x))) {
        if (!exists(".Generic", envir = environment(), inherits = FALSE))
            warning("Direct call of 'as.data.frame.", method,
                    "()' is deprecated.  Use 'as.data.frame.vector()' or 'as.data.frame()' instead")
        .frame_vector(x, row.names, optional, nm)
    }
}

as.data.frame.numeric <- .frame_legacy_method("numeric")
as.data.frame.integer <- .frame_legacy_method("integer")
as.data.frame.logical <- .frame_legacy_method("logical")
as.data.frame.complex <- .frame_legacy_method("complex")
as.data.frame.raw <- .frame_legacy_method("raw")
as.data.frame.factor <- .frame_legacy_method("factor")
as.data.frame.Date <- .frame_legacy_method("Date")
as.data.frame.POSIXct <- .frame_legacy_method("POSIXct")

as.data.frame.character <- function(x, row.names = NULL, optional = FALSE, ...,
                                    stringsAsFactors = FALSE, nm = deparse1(substitute(x)))
    .frame_vector(x, row.names, optional, nm, stringsAsFactors)

as.data.frame.POSIXlt <- function(x, row.names = NULL, optional = FALSE, ...)
    .frame_vector(as.POSIXct(x), row.names, optional, deparse1(substitute(x)))

as.data.frame.AsIs <- function(x, row.names = NULL, optional = FALSE, ...) {
    if (length(dim(x)) == 2L) {
        labels <- rownames(x)
        if (is.null(labels)) labels <- .set_row_names(nrow(x))
        columns <- list(x)
        if (!optional) names(columns) <- "x"
        return(structure(columns, class = "data.frame", row.names = labels))
    }
    .frame_vector(x, row.names, optional, deparse1(substitute(x)), keep.names = TRUE)
}

as.data.frame.matrix <- function(x, row.names = NULL, optional = FALSE, make.names = TRUE, ...,
                                 stringsAsFactors = FALSE) {
    n <- nrow(x)
    p <- ncol(x)
    if (is.null(row.names)) row.names <- rownames(x)
    if (!is.null(row.names) && length(row.names) != n) row.names <- NULL
    if (!is.null(row.names) && (anyNA(row.names) || anyDuplicated(row.names))) {
        if (is.na(make.names)) row.names <- NULL
        else if (make.names) row.names <- make.names(row.names, unique = TRUE)
        else if (anyNA(row.names)) stop("missing values in 'row.names' are not allowed")
        else if (anyDuplicated(row.names)) {
            warning("non-unique value when setting 'row.names': ",
                    paste(unique(row.names[duplicated(row.names)]), collapse = ", "))
            stop("duplicate 'row.names' are not allowed")
        }
    }
    if (is.null(row.names)) row.names <- .set_row_names(n)
    else if (!is.integer(row.names)) row.names <- as.character(row.names)
    columns <- lapply(seq_len(p), function(i) {
        if (stringsAsFactors && is.character(x)) {
            values <- x[, i]
            return(setNames(factor(values), names(values)))
        }
        unname(x[, i])
    })
    labels <- colnames(x)
    if (!is.null(labels)) {
        empty <- which(!is.na(labels) & labels == "")
        labels[empty] <- paste0("V", empty)
        names(columns) <- labels
    } else if (!p) names(columns) <- character()
    else if (!optional) names(columns) <- paste0("V", seq_len(p))
    structure(columns, class = "data.frame", row.names = row.names)
}

as.data.frame.array <- function(x, row.names = NULL, optional = FALSE, ...) {
    shape <- dim(x)
    axes <- dimnames(x)
    if (length(dim(x)) == 1L) {
        dim(x) <- NULL
        if (!is.null(axes)) names(x) <- axes[[1L]]
        return(.frame_vector(x, row.names, optional, deparse1(substitute(x))))
    }
    p <- prod(shape[-1L])
    labels <- NULL
    if (!is.null(axes)) {
        stride <- 1L
        for (i in seq.int(2L, length(shape))) {
            axis <- axes[[i]]
            if (is.null(axis)) axis <- as.character(seq_len(shape[[i]]))
            part <- rep(axis, each = stride, length.out = p)
            labels <- if (is.null(labels) || !p) part else paste(labels, part, sep = ".")
            stride <- stride * shape[[i]]
        }
        axes <- list(axes[[1L]], labels)
    }
    x <- matrix(x, nrow = shape[[1L]], ncol = p, dimnames = axes)
    as.data.frame.matrix(x, row.names, optional, ...)
}

as.data.frame.list <- function(x, row.names = NULL, optional = FALSE, ..., cut.names = FALSE,
                               col.names = names(x), fix.empty.names = TRUE,
                               new.names = !missing(col.names), check.names = !optional,
                               stringsAsFactors = FALSE) {
    if (new.names || is.null(names(x))) {
        if (is.null(col.names)) col.names <- rep("", length(x))
        names(x) <- col.names
    }
    input_names <- names(x)
    if (!optional && cut.names) {
        width <- max(6L, as.integer(cut.names))
        long <- which(!is.na(input_names) & nchar(input_names) > width)
        input_names[long] <- paste0(substr(input_names[long], 1L, width - 6L), " ...")
        names(x) <- input_names
    }
    .frame_list(x, x, row.names, is.null(row.names),
                FALSE, check.names, fix.empty.names, stringsAsFactors)
}

.frame_list <- function(x, fallback, row.names, infer.rows, check.rows, check.names,
                        fix.empty.names, stringsAsFactors) {
    if (!length(x)) {
        if (is.null(row.names)) row.names <- integer()
        else row.names <- .frame_list_rows(row.names, length(row.names))
        return(structure(list(), names = character(), row.names = row.names, class = "data.frame"))
    }
    input_names <- names(x)
    if (is.null(input_names)) input_names <- rep("", length(x))
    frames <- lapply(x, function(column)
        as.data.frame(column, optional = TRUE, stringsAsFactors = stringsAsFactors))
    sizes <- vapply(frames, nrow, 0L)
    n <- max(sizes)
    short <- which(sizes < n)
    recyclable <- function(value) {
        is.vector(value) || is.factor(value) || inherits(value, "Date") || inherits(value, "POSIXct")
    }
    if (any(sizes[short] == 0L | n %% sizes[short] != 0L) ||
        any(vapply(frames[short], function(frame) any(!vapply(frame, recyclable, FALSE)), FALSE)))
        stop("arguments imply differing number of rows: ", paste(unique(sizes), collapse = ", "))

    columns <- list()
    labels <- character()
    inferred_rows <- inferred_names <- NULL
    for (i in seq_along(frames)) {
        frame <- frames[[i]]
        num_cols <- length(frame)
        label <- input_names[[i]]
        if (!is.na(label) && !nzchar(label) && fix.empty.names) {
            label <- deparse1(fallback[[i]])
        }
        inner <- names(frame)
        if (num_cols == 1L && is.null(inner)) {
            inner <- label
        } else if (num_cols > 1L) {
            if (is.null(inner)) inner <- as.character(seq_len(num_cols))
            if (!is.na(input_names[[i]]) && nzchar(input_names[[i]]))
                inner <- paste(input_names[[i]], inner, sep = ".")
        }
        if (infer.rows && sizes[[i]] && .row_names_info(frame) >= 0L &&
            (is.null(inferred_rows) || check.rows)) {
            current <- attr(frame, "row.names", exact = TRUE)
            if (check.rows) {
                current_names <- as.character(current)
                if (is.null(inferred_names)) inferred_names <- current_names
                else if (!identical(inferred_names, current_names))
                    stop("mismatch of row names in arguments of 'data.frame', item ", i)
            }
            if (is.null(inferred_rows)) {
                if (sizes[[i]] < n) {
                    warning("row names were found from a short variable and have been discarded")
                    inferred_rows <- .set_row_names(n)
                } else inferred_rows <- .frame_list_rows(current, n)
            }
        }
        for (j in seq_len(num_cols)) {
            value <- frame[[j]]
            if (sizes[[i]] < n) value <- rep(value, length.out = n)
            columns[[length(columns) + 1L]] <- value
        }
        labels <- c(labels, inner)
    }
    if (check.names) {
        repair <- if (fix.empty.names) seq_along(labels) else which(is.na(labels) | labels != "")
        labels[repair] <- make.names(labels[repair], unique = TRUE)
    }
    names(columns) <- labels
    if (!is.null(row.names) && length(row.names) == 1L && n != 1L) {
        position <- if (is.character(row.names)) match(row.names, labels) else as.integer(row.names)
        if (is.na(position) || position < 1L || position > length(columns))
            stop("'row.names' should specify one of the variables")
        row.names <- columns[[position]]
        columns[position] <- NULL
    }
    if (is.null(row.names)) row.names <- if (is.null(inferred_rows)) .set_row_names(n) else inferred_rows
    else row.names <- .frame_list_rows(row.names, n)
    structure(columns, class = "data.frame", row.names = row.names)
}

.frame_list_rows <- function(labels, n) {
    if (length(labels) != n) stop("row names supplied are of the wrong length")
    if (!is.integer(labels)) labels <- as.character(labels)
    if (anyNA(labels)) stop("row names contain missing values")
    if (anyDuplicated(labels))
        stop("duplicate row.names: ", paste(unique(labels[duplicated(labels)]), collapse = ", "))
    labels
}

as.data.frame.table <- function(x, row.names = NULL, ..., responseName = "Freq",
                                stringsAsFactors = TRUE, sep = "", base = list(LETTERS)) {
    shape <- dim(x)
    axes <- dimnames(x)
    labels <- names(axes)
    if (is.null(labels)) labels <- rep("", length(shape))
    columns <- vector("list", length(shape) + 1L)
    stride <- 1L
    for (i in seq_along(shape)) {
        values <- if (is.null(axes)) NULL else axes[[i]]
        if (is.null(values)) {
            values <- base[[(i - 1L) %% length(base) + 1L]]
            indices <- seq_len(shape[[i]]) - 1L
            repeat_number <- indices %/% length(values)
            values <- values[indices %% length(values) + 1L]
            repeated <- which(repeat_number > 0L)
            values[repeated] <- paste(values[repeated], repeat_number[repeated], sep = sep)
        }
        if (!nzchar(labels[[i]])) labels[[i]] <- paste0("Var", i)
        if (stringsAsFactors) values <- factor(values, levels = unique(values))
        indices <- rep(seq_len(shape[[i]]), each = stride, length.out = length(x))
        columns[[i]] <- values[indices]
        stride <- stride * shape[[i]]
    }
    columns[[length(columns)]] <- as.vector(x)
    names(columns) <- make.names(c(labels, responseName), unique = TRUE)
    if (is.null(row.names)) row.names <- .set_row_names(length(x))
    else row.names <- .frame_list_rows(row.names, length(x))
    structure(columns, class = "data.frame", row.names = row.names)
}

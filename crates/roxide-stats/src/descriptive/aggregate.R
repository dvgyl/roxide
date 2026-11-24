aggregate <- function(x, ...) {
    if (missing(x)) stop("argument 'x' is  missing -- it has been renamed from 'formula'")
    UseMethod("aggregate")
}

aggregate.default <- function(x, ...) {
    if (is.ts(x)) return(aggregate.ts(x, ...))
    aggregate.data.frame(.aggregate_frame(x), ...)
}

.aggregate_frame <- function(x) {
    if (is.data.frame(x)) return(x)
    if (exists("as.data.frame", envir = asNamespace("base"), mode = "function"))
        return(base::as.data.frame(x))
    if (is.null(x)) return(data.frame())
    if (is.matrix(x)) return(.aggregate_matrix_frame(x))
    if (is.list(x)) {
        labels <- names(x)
        if (is.null(labels)) labels <- rep("", length(x))
        unnamed <- which(is.na(labels) | labels == "")
        labels[unnamed] <- vapply(x[unnamed], deparse1, "")
        names(x) <- labels
        return(do.call(data.frame, x, quote = TRUE))
    }
    data.frame(x = x)
}

.aggregate_matrix_frame <- function(x) {
    fields <- lapply(seq_len(ncol(x)), function(j) unname(x[, j]))
    labels <- colnames(x)
    if (is.null(labels)) labels <- paste0("V", seq_along(fields))
    .aggregate_result(fields, labels, nrow(x))
}

.aggregate_result <- function(fields, labels, rows) {
    if (rows == 0L) row_names <- integer() else row_names <- c(NA_integer_, -as.integer(rows))
    structure(fields, names = labels, row.names = row_names, class = "data.frame")
}

.aggregate_factor <- function(x) if (is.factor(x)) x else factor(x)

.aggregate_slice <- function(x, rows) {
    out <- x[rows]
    if (inherits(x, "Date")) class(out) <- "Date"
    out
}

.aggregate_fun <- function(FUN, caller) {
    if (is.function(FUN)) return(FUN)
    if ((is.character(FUN) && length(FUN) == 1L) || is.symbol(FUN))
        return(get(as.character(FUN), envir = caller, mode = "function"))
    stop(paste0("'", deparse1(FUN), "' is not a function, character or symbol"))
}

.aggregate_collect <- function(values) {
    if (all(vapply(values, is.factor, FALSE))) {
        labels <- unique(unlist(lapply(values, levels), use.names = FALSE))
        codes <- unlist(lapply(values, function(x) match(levels(x), labels)[as.integer(x)]),
                        use.names = FALSE)
        ordered <- all(vapply(values, function(x)
            inherits(x, "ordered") && identical(levels(x), labels), FALSE))
        return(structure(codes, levels = labels,
                         class = if (ordered) c("ordered", "factor") else "factor"))
    }
    flat <- unname(do.call(c, values, quote = TRUE))
    if (all(vapply(values, inherits, FALSE, "Date"))) class(flat) <- "Date"
    flat
}

.aggregate_reduce <- function(column, groups, index, simplify, FUN, ...) {
    values <- lapply(groups, function(rows) FUN(.aggregate_slice(column, rows), ...))
    if (!simplify || !length(values)) return(values[index])

    sizes <- lengths(values)
    width <- sizes[[1L]]
    if (!width || any(sizes != width)) return(values[index])
    .ts_extent(max(length(index), length(values)), width, 16L)
    flat <- .aggregate_collect(values)
    if (length(flat) != length(values) * width) return(values[index])
    if (width == 1L) return(.aggregate_slice(flat, index))

    flat <- if (is.factor(flat)) as.character(flat) else as.vector(flat)
    dim(flat) <- c(width, length(values))
    result <- t(flat[, index, drop = FALSE])
    labels <- names(values[[1L]])
    if (!is.null(labels)) dimnames(result) <- list(NULL, labels)
    result
}

aggregate.data.frame <- function(x, by, FUN, ..., simplify = TRUE, drop = TRUE) {
    FUN <- .aggregate_fun(FUN, parent.frame())
    x <- .aggregate_frame(x)
    if (!nrow(x)) stop("no rows to aggregate")
    if (!is.list(by)) stop("'by' must be a list")
    if (any(lengths(by) != nrow(x))) stop("arguments must have same length")

    keep <- which(complete.cases(by))
    labels <- names(by)
    if (is.null(labels)) labels <- rep("", length(by))
    unnamed <- which(labels == "")
    labels[unnamed] <- paste0("Group.", unnamed)
    by <- lapply(by, function(x) unname(.aggregate_slice(x, keep)))
    factors <- lapply(by, .aggregate_factor)
    codes <- lapply(factors, as.integer)
    groups <- .Internal(C_aggregate_rows(codes))
    first <- vapply(groups, function(rows) rows[[1L]], 1L)
    index <- seq_along(groups)
    if (drop) {
        keys <- lapply(by, function(x) .aggregate_slice(x, first))
    } else {
        sizes <- vapply(factors, nlevels, 1L)
        count <- .ts_extent(prod(as.double(sizes)), length(by) + length(x), 16L)
        stride <- c(1, cumprod(sizes))[seq_along(sizes)]
        at <- rep(1, length(first))
        keys <- vector("list", length(by))
        cell <- seq_len(count) - 1L
        for (j in seq_along(by)) {
            code <- codes[[j]]
            at <- at + (code[first] - 1L) * stride[[j]]
            level <- if (count) 1L + (cell %/% stride[[j]]) %% sizes[[j]] else integer()
            column <- by[[j]]
            keys[[j]] <- if (is.factor(column))
                structure(as.integer(level), levels = levels(column), class = class(column))
            else .aggregate_slice(column, match(level, code))
        }
        index <- match(seq_len(count), at)
    }

    groups <- lapply(groups, function(rows) keep[rows])
    fields <- lapply(x, function(column) .aggregate_reduce(column, groups, index, simplify, FUN, ...))
    .aggregate_result(c(keys, fields), c(labels, names(x)), length(index))
}

aggregate.formula <- function(x, data, FUN, ..., subset, na.action = na.omit) {
    if (length(x) != 3L) stop("formula 'x' must have both left and right hand sides")
    if (missing(data)) data <- environment(x)
    if (is.null(data)) data <- environment(x)

    rhs <- x[[3L]]
    lhs <- x[[2L]]
    if (identical(lhs, quote(.))) {
        if ("." %in% all.vars(rhs)) stop("'.' in formula and no 'data' argument")
        grouping <- terms(as.formula(.model_call("~", list(rhs)), env = environment(x)))
        variables <- as.list(attr(grouping, "variables"))[-1L]
        symbols <- variables[vapply(variables, is.symbol, FALSE)]
        columns <- setdiff(names(data), vapply(symbols, as.character, ""))
        lhs <- .model_call("cbind", lapply(columns, as.name))
    } else if (is.list(data) && "." %in% all.vars(rhs)) {
        # Use the grouping columns that do not occur in the response.
        columns <- setdiff(names(data), all.vars(lhs))
        if (!length(columns) && identical(rhs, quote(.))) rhs <- 1L
        else if (length(columns)) {
            rhs_dot <- Reduce(function(a, b) .model_call("+", list(a, b)), lapply(columns, as.name))
            rhs <- do.call(substitute, list(rhs, list(. = rhs_dot)))
        }
    }
    form <- as.formula(.model_call("~", list(lhs, rhs)), env = environment(x))
    if (missing(subset)) frame <- model.frame(form, data, na.action = na.action)
    else {
        expr <- substitute(model.frame(form, data, subset = selection, na.action = na.action),
                           list(selection = substitute(subset)))
        frame <- eval(expr, environment())
    }

    response <- frame[[1L]]
    fields <- if (is.matrix(response)) .aggregate_matrix_frame(response)
              else .aggregate_result(list(response), names(frame)[1L], nrow(frame))
    if (length(frame) > 1L) return(aggregate.data.frame(fields, as.list(frame)[-1L], FUN, ...))
    if (!nrow(frame)) stop("no rows to aggregate")
    out <- aggregate.data.frame(fields, list(rep(1L, nrow(frame))), FUN, ...)
    .aggregate_result(as.list(out)[-1L], names(out)[-1L], nrow(out))
}

aggregate.ts <- function(x, nfrequency = 1, FUN = sum, ndeltat = 1,
                         ts.eps = getOption("ts.eps"), ...) {
    if (is.null(ts.eps)) ts.eps <- .ts_eps()
    if (missing(nfrequency)) nfrequency <- 1 / ndeltat
    if (!is.ts(x)) x <- as.ts(x)
    if (abs(frequency(x) - nfrequency) < ts.eps) return(x)
    ratio <- frequency(x) / nfrequency
    if (ratio < 1 || abs(ratio - round(ratio)) > ts.eps)
        stop(paste("cannot change frequency from", frequency(x), "to", nfrequency))

    FUN <- .aggregate_fun(FUN, parent.frame())
    tsp_start <- tsp(x)[[1L]]
    width <- round(ratio)
    count <- .ts_rows(x) %/% width
    starts <- (seq_len(count) - 1L) * width
    reduce <- function(column) {
        column <- unname(column)
        values <- lapply(starts, function(start) FUN(column[start + seq_len(width)], ...))
        sizes <- lengths(values)
        if (all(sizes == sizes[[1L]]) && sizes[[1L]] > 0L) return(.aggregate_collect(values))
        matrix(values, ncol = 1L)
    }
    if (!count) stop("'ts' object must have one or more observations")
    if (is.matrix(x)) {
        fields <- lapply(seq_len(ncol(x)), function(j) reduce(x[, j]))
        if (any(lengths(fields) != count)) stop("length of 'dimnames' [2] not equal to array extent")
        x <- matrix(unlist(fields, use.names = FALSE), ncol = length(fields),
                    dimnames = list(NULL, colnames(x)))
    } else x <- reduce(x)
    ts(x, start = tsp_start, frequency = nfrequency, ts.eps = ts.eps)
}

ave <- function(x, ..., FUN = mean) {
    FUN <- .aggregate_fun(FUN, parent.frame())
    by <- list(...)
    if (!length(by)) {
        x[] <- FUN(x)
        return(x)
    }

    codes <- lapply(by, function(g) as.integer(.aggregate_factor(g)))
    sizes <- lengths(codes)
    if (any(sizes == 0L)) {
        if (length(x)) stop("group length is 0 but data length > 0")
        return(x)
    }
    size <- max(sizes)
    if (any(size %% sizes != 0L)) warning("longer object length is not a multiple of shorter object length")
    if (length(x) %% size) {
        warning("data length is not a multiple of split variable")
        warning("data length is not a multiple of split variable")
    }
    groups <- .Internal(C_aggregate_rows(lapply(codes, rep_len, max(size, length(x)))))
    if (length(x) < size) groups <- lapply(groups, function(rows) rows[rows <= length(x)])
    values <- lapply(groups, function(rows) FUN(x[rows]))
    for (i in seq_along(groups)) if (length(groups[[i]])) x[groups[[i]]] <- values[[i]]
    x
}

.aggregate_recode <- function(x, slots, ordered) {
    labels <- levels(x)
    codes <- match(seq_along(labels), slots)[as.integer(x)]
    if (ordered) kind <- c("ordered", "factor") else kind <- "factor"
    structure(codes, names = names(x), levels = labels[slots], class = kind)
}

reorder <- function(x, ...) UseMethod("reorder")
reorder.default <- function(x, X, FUN = mean, ..., order = is.ordered(x), decreasing = FALSE) {
    if (missing(order)) order <- inherits(x, "ordered")
    key <- .aggregate_factor(x)
    if (length(key) != length(X)) stop("arguments must have same length")
    FUN <- .aggregate_fun(FUN, parent.frame())
    scores <- if (nlevels(key))
        tapply(X, key, FUN, ...)
    else structure(logical(), dim = 0L, dimnames = list(NULL))
    if (!is.atomic(scores)) stop("'x' must be atomic")
    result <- .aggregate_recode(key, order(scores, decreasing = decreasing), order)
    attr(result, "scores") <- scores
    result
}

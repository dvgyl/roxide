match <- function(x, table, nomatch = NA_integer_, incomparables = NULL)
    .Internal(rox_match(x, table, nomatch, incomparables))

`%in%` <- function(x, table) match(x, table, nomatch = 0L) > 0L
is.element <- function(el, set) el %in% set

duplicated <- function(x, incomparables = FALSE, ...) UseMethod("duplicated")
duplicated.default <- function(x, incomparables = FALSE, fromLast = FALSE, nmax = NA, ...)
    .Internal(rox_duplicated(x, incomparables, fromLast, FALSE))
anyDuplicated <- function(x, incomparables = FALSE, ...) UseMethod("anyDuplicated")
anyDuplicated.default <- function(x, incomparables = FALSE, fromLast = FALSE, ...)
    .Internal(rox_duplicated(x, incomparables, fromLast, TRUE))

unique <- function(x, incomparables = FALSE, ...) UseMethod("unique")
unique.default <- function(x, incomparables = FALSE, fromLast = FALSE, nmax = NA, ...)
    unname(x[!duplicated(x, incomparables, fromLast = fromLast, nmax = nmax, ...)])
unique.data.frame <- function(x, incomparables = FALSE, fromLast = FALSE, ...)
    x[!duplicated(x, incomparables, fromLast = fromLast, ...), , drop = FALSE]
duplicated.data.frame <- function(x, incomparables = FALSE, fromLast = FALSE, ...)
    duplicated(lapply(seq_len(nrow(x)), function(i) lapply(x, `[[`, i)),
               incomparables, fromLast = fromLast, ...)
anyDuplicated.data.frame <- function(x, incomparables = FALSE, fromLast = FALSE, ...) {
    found <- duplicated(x, incomparables, fromLast = fromLast, ...)
    w <- which(found)
    if (!length(w)) 0L else if (fromLast) w[[length(w)]] else w[[1L]]
}

union <- function(x, y) unique(c(x, y))
setdiff <- function(x, y) {
    x <- unique(x)
    if (!is.factor(y) && is.factor(x)) x <- as.character(x)
    x[!(x %in% y)]
}
intersect <- function(x, y) {
    if (is.factor(x) && is.factor(y)) {
        vals <- intersect(as.character(x), as.character(y))
        return(factor(vals, levels = sort(union(levels(x), levels(y)))))
    }
    if (is.factor(x)) x <- as.character(x)
    x <- unique(x)
    x[x %in% y]
}

unlist <- function(x, recursive = TRUE, use.names = TRUE) .Internal(rox_unlist(x, recursive, use.names))

which <- function(x, arr.ind = FALSE, useNames = TRUE) {
    if (!is.logical(x)) stop("argument to 'which' is not logical")
    ii <- seq_along(x)[!is.na(x) & x]
    d <- dim(x)
    if (arr.ind && !is.null(d)) {
        ans <- matrix(0L, nrow = length(ii), ncol = length(d))
        k <- ii - 1L
        for (j in seq_along(d)) {
            ans[, j] <- as.integer(k %% d[[j]] + 1L)
            k <- k %/% d[[j]]
        }
        if (useNames) {
            dn <- dimnames(x)
            axis <- names(dn)
            if (is.null(axis)) axis <- if (length(d) == 2L) c("row", "col") else paste0("dim", seq_along(d))
            label <- if (is.null(dn)) NULL else dn[[1L]][ans[, 1L]]
            dimnames(ans) <- list(label, axis)
        }
        return(ans)
    }
    if (!is.null(names(x))) names(ii) <- names(x)[ii]
    ii
}
which.max <- function(x) {
    x <- if (is.numeric(x) || is.logical(x)) x else as.double(x)
    i <- which(!is.na(x))
    if (!length(i)) return(integer())
    i[which(x[i] == max(x[i]))[[1L]]]
}
which.min <- function(x) {
    x <- if (is.numeric(x) || is.logical(x)) x else as.double(x)
    i <- which(!is.na(x))
    if (!length(i)) return(integer())
    i[which(x[i] == min(x[i]))[[1L]]]
}

vector <- function(mode = "logical", length = 0L) {
    if (identical(mode, "logical")) return(logical(length))
    if (identical(mode, "integer")) return(integer(length))
    if (mode %in% c("numeric", "double")) return(numeric(length))
    if (identical(mode, "complex")) return(as.complex(numeric(length)))
    if (identical(mode, "character")) return(character(length))
    if (identical(mode, "raw")) return(raw(length))
    if (identical(mode, "list")) return(rep(list(NULL), length.out = length))
    if (identical(mode, "expression")) return(.Internal(rox_expression_vector(length)))
    if (identical(mode, "pairlist")) return(as.vector(rep(list(NULL), length.out = length), "pairlist"))
    stop("vector: cannot make a vector of mode '", mode, "'.")
}
array <- function(data = NA, dim = length(data), dimnames = NULL) {
    dim <- as.integer(dim)
    if (!length(dim)) stop("'dim' cannot be of length 0")
    if (any(is.na(dim)) || any(dim < 0L)) stop("'dims' contains missing or negative values")
    x <- if (is.null(data)) stop("'data' must be of a vector type, was 'NULL'") else
        rep(as.vector(data), length.out = prod(dim))
    dim(x) <- dim
    if (!is.null(dimnames)) dimnames(x) <- dimnames
    x
}
rep_len <- function(x, length.out) {
    out <- unname(rep(unclass(x), length.out = length.out))
    if (is.factor(x)) {
        class(out) <- class(x)
        attr(out, "levels") <- levels(x)
    }
    out
}
append <- function(x, values, after = length(x)) {
    if (after <= 0) return(c(values, x))
    if (after >= length(x)) return(c(x, values))
    c(x[seq_len(after)], values, x[seq.int(after + 1L, length(x))])
}
lengths <- function(x, use.names = TRUE) {
    out <- vapply(x, length, 0L, USE.NAMES = FALSE)
    if (use.names) names(out) <- names(x)
    if (!is.null(dim(x))) {
        dim(out) <- dim(x)
        if (use.names) dimnames(out) <- dimnames(x)
    }
    out
}
NROW <- function(x) if (is.data.frame(x)) nrow(x) else if (is.null(dim(x))) length(x) else dim(x)[[1L]]
NCOL <- function(x) if (is.data.frame(x)) ncol(x) else if (length(dim(x)) < 2L) 1L else dim(x)[[2L]]
nlevels <- function(x) length(levels(x))
setNames <- function(object = nm, nm) {
    names(object) <- nm
    object
}

.subset <- function(x, ...) `[`(unclass(x), ...)
.subset2 <- function(x, ...) `[[`(unclass(x), ...)
getElement <- function(object, name) {
    if (isS4(object)) return(slot(object, name))
    object[[name, exact = TRUE]]
}

`rownames<-` <- function(x, value) {
    if (is.data.frame(x)) {
        row.names(x) <- value
        return(x)
    }
    if (is.null(dim(x))) {
        if (is.null(value)) return(x)
        stop("attempt to set 'rownames' on an object with no dimensions")
    }
    dn <- dimnames(x)
    if (is.null(dn)) dn <- vector("list", length(dim(x)))
    dn[1L] <- list(value)
    dimnames(x) <- dn
    x
}
`oldClass<-` <- function(x, value) {
    attr(x, "class") <- value
    x
}
`storage.mode<-` <- function(x, value) {
    if (identical(typeof(x), value)) return(x)
    a <- attributes(x)
    x <- as.vector(x, mode = value)
    attributes(x) <- a
    x
}
`mode<-` <- function(x, value) {
    if (identical(mode(x), value)) return(x)
    a <- attributes(x)
    x <- as.vector(x, mode = value)
    attributes(x) <- a
    x
}
`is.na<-` <- function(x, value) {
    if (is.list(x)) x[value] <- list(NA) else x[value] <- NA
    x
}

mean <- function(x, ...) UseMethod("mean")
mean.default <- function(x, trim = 0, na.rm = FALSE, ...) {
    if (!(is.numeric(x) || is.complex(x) || is.logical(x))) {
        warning("argument is not numeric or logical: returning NA")
        return(NA_real_)
    }
    if (na.rm) x <- x[!is.na(x)]
    n <- length(x)
    if (trim > 0 && n) {
        if (any(is.na(x))) return(NA_real_)
        if (is.complex(x)) stop("trimmed means are not defined for complex data")
        x <- sort(x)
        if (trim >= 0.5) return((x[[floor((n + 1) / 2)]] + x[[ceiling((n + 1) / 2)]]) / 2)
        edge <- floor(n * trim)
        x <- x[seq.int(edge + 1, n - edge)]
    }
    if (!length(x)) return(sum(x) / 0)
    # Scale large values before summing to reduce overflow.
    result <- sum(x) / length(x)
    if (any(is.infinite(result)) && all(is.finite(x))) result <- sum(x / length(x))
    if (all(is.finite(result))) result <- result + sum((x - result) / length(x))
    result
}

with <- function(data, expr, ...) UseMethod("with")
with.default <- function(data, expr, ...) eval(substitute(expr), data, enclos = parent.frame())
within <- function(data, expr, ...) UseMethod("within")
within.list <- function(data, expr, ...) {
    env <- new.env(parent = parent.frame())
    for (nm in names(data)) assign(nm, data[[nm]], envir = env)
    eval(substitute(expr), env)
    left <- ls(env, all.names = TRUE)
    for (nm in setdiff(names(data), left)) data[[nm]] <- NULL
    for (nm in left) data[[nm]] <- get(nm, envir = env, inherits = FALSE)
    data
}
within.data.frame <- within.list

.rox_apply_fun <- function(FUN, where) {
    if (is.function(FUN)) return(FUN)
    if (is.character(FUN) && length(FUN) == 1L) return(get(FUN, envir = where, mode = "function"))
    if (typeof(FUN) == "symbol") return(get(as.character(FUN), envir = where, mode = "function"))
    stop("'FUN' is not a function, character or symbol")
}
mapply <- function(FUN, ..., MoreArgs = NULL, SIMPLIFY = TRUE, USE.NAMES = TRUE) {
    FUN <- .rox_apply_fun(FUN, parent.frame())
    xs <- list(...)
    if (!length(xs)) return(list())
    ns <- lengths(xs, use.names = FALSE)
    if (any(ns == 0L)) return(list())
    n <- max(ns)
    if (any(n %% ns != 0L)) warning("longer argument not a multiple of length of shorter")
    ans <- lapply(seq_len(n), function(i) {
        args <- lapply(seq_along(xs), function(j) xs[[j]][[(i - 1L) %% ns[[j]] + 1L]])
        names(args) <- names(xs)
        do.call(FUN, c(args, MoreArgs), quote = TRUE)
    })
    if (USE.NAMES) {
        nm <- names(xs[[1L]])
        if (is.null(nm) && is.character(xs[[1L]])) nm <- xs[[1L]]
        if (!is.null(nm)) names(ans) <- rep_len(nm, n)
    }
    if (identical(SIMPLIFY, FALSE)) ans else .rox_simplify(ans, identical(SIMPLIFY, "array"))
}
Map <- function(f, ...) mapply(f, ..., SIMPLIFY = FALSE)

.rox_simplify <- function(x, higher = FALSE) {
    if (!length(x)) return(x)
    len <- lengths(x, use.names = FALSE)
    n <- len[[1L]]
    if (!n || any(len != n)) return(x)
    ans <- unlist(x, recursive = FALSE, use.names = FALSE)
    if (length(ans) != n * length(x)) return(x)
    if (n == 1L) {
        names(ans) <- names(x)
        return(ans)
    }
    d <- dim(x[[1L]])
    if (higher && !is.null(d) && all(vapply(x, function(y) identical(dim(y), d), TRUE))) {
        dim(ans) <- c(d, length(x))
        dn <- dimnames(x[[1L]])
        if (is.null(dn)) dn <- vector("list", length(d))
        dimnames(ans) <- c(dn, list(names(x)))
    } else {
        dim(ans) <- c(n, length(x))
        dimnames(ans) <- list(names(x[[1L]]), names(x))
    }
    ans
}
Position <- function(f, x, right = FALSE, nomatch = NA_integer_) {
    f <- .rox_apply_fun(f, parent.frame())
    ii <- seq_along(x)
    if (right) ii <- rev(ii)
    for (i in ii) if (f(x[[i]])) return(i)
    nomatch
}
Find <- function(f, x, right = FALSE, nomatch = NULL) {
    f <- .rox_apply_fun(f, parent.frame())
    i <- Position(f, x, right)
    if (is.na(i)) nomatch else x[[i]]
}
rapply <- function(object, f, classes = "ANY", deflt = NULL, how = "unlist", ...) {
    f <- .rox_apply_fun(f, parent.frame())
    walk <- function(x) {
        if (is.list(x)) return(lapply(x, walk))
        if ("ANY" %in% classes || any(class(x) %in% classes)) return(f(x, ...))
        if (identical(how, "replace")) x else deflt
    }
    if (!is.list(object) && typeof(object) != "expression") stop("'object' must be a list or expression")
    if (!(how %in% c("unlist", "replace", "list"))) stop("invalid 'how' argument")
    ans <- lapply(object, walk)
    if (identical(how, "unlist")) unlist(ans) else ans
}
replicate <- function(n, expr, simplify = "array") {
    expr <- substitute(expr)
    env <- parent.frame()
    ans <- lapply(seq_len(n), function(i) eval(expr, env))
    if (identical(simplify, FALSE)) ans else .rox_simplify(ans, identical(simplify, "array"))
}

split <- function(x, f, drop = FALSE, ...) UseMethod("split")
split.default <- function(x, f, drop = FALSE, ...) {
    if (is.list(f)) f <- .rox_groups(f, drop, ...)$factor
    if (!is.factor(f)) f <- factor(f)
    if (drop) f <- factor(f)
    if (!length(f) && length(x)) stop("group length is 0 but data length > 0")
    if (length(f) && length(x) %% length(f)) warning("data length is not a multiple of split variable")
    i <- rep_len(as.integer(f), length(x))
    out <- lapply(seq_along(levels(f)), function(j) x[which(i == j)])
    names(out) <- levels(f)
    out
}
split.data.frame <- function(x, f, drop = FALSE, ...) lapply(
    split(seq_len(nrow(x)), f, drop, ...), function(i) x[i, , drop = FALSE])

.rox_groups <- function(index, drop = FALSE, sep = ".", lex.order = FALSE) {
    fs <- lapply(index, function(x) if (is.factor(x) && !drop) x else factor(x))
    lens <- lengths(fs)
    if (length(lens) && any(lens != lens[[1L]])) stop("all arguments must have the same length")
    labels <- lapply(fs, levels)
    dims <- lengths(labels)
    if (!length(fs)) stop("'INDEX' is of length zero")
    code <- rep_len(0L, lens[[1L]])
    group <- ""
    for (j in seq_along(fs)) {
        k <- if (lex.order) length(fs) + 1L - j else j
        code <- code + (as.integer(fs[[k]]) - 1L) * length(group)
        lab <- labels[[k]]
        group <- if (j == 1L) lab else if (lex.order)
            paste(rep(lab, each = length(group)), group, sep = sep) else
            paste(rep(group, length(lab)), rep(lab, each = length(group)), sep = sep)
    }
    list(factor = structure(as.integer(code + 1L), levels = group, class = "factor"),
         dims = as.integer(dims), labels = labels)
}
tapply <- function(X, INDEX, FUN = NULL, ..., default = NA, simplify = TRUE) {
    if (!is.list(INDEX)) INDEX <- list(INDEX)
    info <- .rox_groups(INDEX)
    f <- info$factor
    if (length(f) != length(X)) stop("arguments must have same length")
    if (is.null(FUN)) return(as.integer(f))
    FUN <- .rox_apply_fun(FUN, parent.frame())
    groups <- split(X, f)
    active <- which(lengths(groups) > 0L)
    out <- vector("list", length(groups))
    out[active] <- lapply(groups[active], FUN, ...)
    if (simplify && length(active) && all(lengths(out[active]) == 1L)) {
        data <- unlist(out[active], use.names = FALSE)
        out <- rep(default, length(groups))
        out[active] <- data
    }
    dim(out) <- info$dims
    dimnames(out) <- info$labels
    out
}

apply <- function(X, MARGIN, FUN, ..., simplify = TRUE) {
    FUN <- .rox_apply_fun(FUN, parent.frame())
    if (is.data.frame(X)) X <- as.matrix(X)
    d <- dim(X)
    if (is.null(d)) stop("dim(X) must have a positive length")
    dn <- dimnames(X)
    if (is.character(MARGIN)) MARGIN <- match(MARGIN, names(dn))
    if (any(is.na(MARGIN)) || any(MARGIN < 1L | MARGIN > length(d)) || anyDuplicated(MARGIN))
        stop("'MARGIN' does not match dim(X)")
    keep <- as.integer(MARGIN)
    within <- setdiff(seq_along(d), keep)
    stride <- c(1, cumprod(d))[seq_along(d)]
    offsets <- function(axes) {
        off <- 0
        for (axis in axes) off <- rep(off, d[[axis]]) +
            rep((seq_len(d[[axis]]) - 1L) * stride[[axis]], each = length(off))
        off
    }
    origin <- offsets(keep)
    selection <- offsets(within)
    x <- as.vector(X)
    ans <- lapply(origin, function(o) {
        slice <- x[o + selection + 1L]
        if (length(within) > 1L) {
            dim(slice) <- d[within]
            if (!is.null(dn)) dimnames(slice) <- dn[within]
        } else if (length(within) == 1L && !is.null(dn)) names(slice) <- dn[[within]]
        FUN(slice, ...)
    })
    if (simplify && length(ans)) {
        n <- lengths(ans, use.names = FALSE)
        if (all(n == n[[1L]]) && n[[1L]] > 0L) {
            result <- unlist(ans, recursive = FALSE, use.names = FALSE)
            if (n[[1L]] != 1L) {
                dim(result) <- c(n[[1L]], d[keep])
                dims <- if (is.null(dn)) vector("list", length(keep)) else dn[keep]
                dimnames(result) <- c(list(names(ans[[1L]])), dims)
                return(result)
            }
            ans <- result
        }
    }
    if (length(keep) > 1L) {
        dim(ans) <- d[keep]
        if (!is.null(dn)) dimnames(ans) <- dn[keep]
    } else if (!is.null(dn)) names(ans) <- dn[[keep]]
    ans
}

eapply <- function(env, FUN, ..., all.names = FALSE, USE.NAMES = TRUE) {
    FUN <- .rox_apply_fun(FUN, parent.frame())
    nm <- ls(env, all.names = all.names, sorted = FALSE)
    ans <- lapply(nm, function(name) FUN(get(name, envir = env, inherits = FALSE), ...))
    if (USE.NAMES) names(ans) <- nm
    ans
}

.rox_parallel_extreme <- function(args, na.rm, less) {
    if (!length(args)) stop("no arguments")
    lens <- lengths(args, use.names = FALSE)
    out <- args[[1L]]
    if (length(args) == 1L) return(out)
    if (any(lens == 0L)) return(out[integer()])
    n <- max(lens)
    if (any(n %% lens != 0L)) warning("an argument will be fractionally recycled")
    out <- rep_len(out, n)
    for (i in seq.int(2L, length(args))) {
        y <- rep_len(args[[i]], n)
        # Assignment into a common vector type preserves numeric promotion.
        out <- c(out, y)[seq_len(n)]
        change <- if (less) which(y < out) else which(y > out)
        if (na.rm) change <- c(change, which(is.na(out) & !is.na(y)))
        out[change] <- y[change]
        if (!na.rm) out[is.na(y)] <- y[is.na(y)]
    }
    if (n == length(args[[1L]])) attributes(out) <- attributes(args[[1L]])
    out
}
pmax <- function(..., na.rm = FALSE) .rox_parallel_extreme(list(...), na.rm, FALSE)
pmin <- function(..., na.rm = FALSE) .rox_parallel_extreme(list(...), na.rm, TRUE)

all.equal <- function(target, current, ...) UseMethod("all.equal")
all.equal.default <- function(target, current, ..., check.attributes = TRUE) {
    if (identical(target, current)) return(TRUE)
    if (!identical(mode(target), mode(current)))
        return(paste0("Modes: ", mode(target), ", ", mode(current)))
    if (length(target) != length(current))
        return(sprintf("Lengths (%d, %d) differ", length(target), length(current)))
    if (!check.attributes && identical(as.vector(target), as.vector(current))) return(TRUE)
    "target, current do not match"
}
.rox_equal_attrs <- function(target, current, ...) {
    a <- attributes(target)
    b <- attributes(current)
    found <- character()
    for (nm in union(names(a), names(b))) {
        if (identical(a[[nm]], b[[nm]])) next
        ans <- all.equal(a[[nm]], b[[nm]], ..., check.attributes = FALSE)
        if (!isTRUE(ans)) {
            prefix <- if (nm == "names") "Names" else paste0("Attributes: <", nm, ">")
            found <- c(found, paste0(prefix, ": ", ans))
        }
    }
    found
}
all.equal.numeric <- function(target, current, tolerance = sqrt(.Machine$double.eps), scale = NULL,
                              countEQ = FALSE, formatFUN = function(x) format(x, digits = 7L, trim = TRUE),
                              ..., check.attributes = TRUE) {
    notes <- if (check.attributes) .rox_equal_attrs(target, current, tolerance = tolerance) else character()
    if (!(is.numeric(current) || is.complex(current))) return(c(notes, paste0("target is numeric, current is ", mode(current))))
    if (length(target) != length(current))
        return(c(notes, sprintf("Numeric: lengths (%d, %d) differ", length(target), length(current))))
    bad <- is.na(target) != is.na(current)
    if (any(bad)) notes <- c(notes, sprintf("'is.NA' value mismatch: %d in current %d in target",
                                           sum(is.na(current)), sum(is.na(target))))
    test <- which(!is.na(target) & !is.na(current) & (countEQ | target != current))
    if (length(test)) {
        x <- target[test]
        y <- current[test]
        delta <- mean(abs(x - y))
        if (is.null(scale)) {
            scale <- mean(abs(x))
            relative <- is.finite(scale) && scale > tolerance
            if (!relative) scale <- 1
            label <- if (relative) "relative" else "absolute"
        } else label <- "scaled"
        delta <- delta / scale
        if (is.na(delta) || delta > tolerance)
            notes <- c(notes, paste0("Mean ", label, " difference: ", formatFUN(delta)))
    }
    if (length(notes)) notes else TRUE
}
all.equal.character <- function(target, current, ..., check.attributes = TRUE) {
    if (!is.character(current)) return(paste0("target is character, current is ", mode(current)))
    notes <- if (check.attributes) .rox_equal_attrs(target, current, ...) else character()
    n <- min(length(target), length(current))
    if (length(target) != length(current))
        notes <- c(notes, sprintf("Lengths (%d, %d) differ (string compare on first %d)",
                                  length(target), length(current), n))
    i <- seq_len(n)
    x <- target[i]
    y <- current[i]
    mismatch <- sum(is.na(x) != is.na(y)) + sum(x != y, na.rm = TRUE)
    if (mismatch) notes <- c(notes, sprintf("%d string mismatch%s", mismatch, if (mismatch == 1L) "" else "es"))
    if (length(notes)) notes else TRUE
}
all.equal.list <- function(target, current, ..., check.attributes = TRUE, use.names = TRUE) {
    if (!is.list(current)) return(paste0("target is list, current is ", mode(current)))
    notes <- if (check.attributes) .rox_equal_attrs(target, current, ...) else character()
    n <- min(length(target), length(current))
    if (length(target) != length(current))
        notes <- c(notes, sprintf("Length mismatch: comparison on first %d components", n))
    for (i in seq_len(n)) {
        result <- all.equal(target[[i]], current[[i]], ..., check.attributes = check.attributes)
        if (!isTRUE(result)) {
            nm <- names(target)[i]
            label <- if (use.names && length(nm) && !is.na(nm) && nzchar(nm) &&
                         identical(nm, names(current)[i])) dQuote(nm) else i
            notes <- c(notes, paste0("Component ", label, ": ", result))
        }
    }
    if (length(notes)) notes else TRUE
}
all.equal.language <- function(target, current, ...) {
    if (identical(deparse(target), deparse(current))) TRUE
    else "target, current do not match when deparsed"
}
all.equal.call <- all.equal.language
all.equal.name <- all.equal.language
all.equal.formula <- all.equal.language

summary <- function(object, ...) UseMethod("summary")
summary.default <- function(object, ..., digits, quantile.type = 7) {
    x <- object
    result.class <- c("summaryDefault", "table")
    if (is.matrix(x)) return(summary.matrix(x, ..., digits = digits))
    if (is.factor(x)) return(summary.factor(x, ...))
    n <- length(x)
    if (is.numeric(x)) {
        vals <- sort(x[!is.na(x)])
        p <- c(0, 0.25, 0.5, 0.75, 1)
        if (!length(vals)) q <- rep(NA_real_, 5L)
        else if (quantile.type != 7L && exists("quantile", envir = asNamespace("stats"), inherits = FALSE))
            q <- as.vector(stats::quantile(vals, probs = p, type = quantile.type))
        else {
            k <- (length(vals) - 1L) * p + 1L
            lo <- floor(k)
            hi <- ceiling(k)
            q <- as.double(vals[lo])
            j <- which(lo != hi & vals[lo] != vals[hi])
            q[j] <- (hi[j] - k[j]) * vals[lo[j]] + (k[j] - lo[j]) * vals[hi[j]]
        }
        out <- c(q[1L:3L], mean(vals), q[4L:5L])
        names(out) <- c("Min.", "1st Qu.", "Median", "Mean", "3rd Qu.", "Max.")
        if (!missing(digits)) out <- signif(out, digits)
    } else if (is.logical(x)) {
        out <- c(Mode = "logical")
        if (any(!x, na.rm = TRUE)) out <- c(out, "FALSE" = as.character(sum(!x, na.rm = TRUE)))
        if (any(x, na.rm = TRUE)) out <- c(out, "TRUE" = as.character(sum(x, na.rm = TRUE)))
    } else if (is.character(x)) {
        vals <- x[!is.na(x)]
        sizes <- nchar(vals)
        out <- c(Length = n, N.unique = length(unique(vals)), N.blank = sum(!nzchar(vals)),
                 Min.nchar = if (length(sizes)) min(sizes) else NA_integer_,
                 Max.nchar = if (length(sizes)) max(sizes) else NA_integer_)
        storage.mode(out) <- "integer"
    } else if (is.complex(x)) {
        describe <- function(v) {
            v <- sort(v[!is.na(v)])
            count <- length(v)
            if (!count) return(rep(NA_real_, 3L))
            c(v[[1L]], (v[[floor((count + 1L) / 2)]] + v[[ceiling((count + 1L) / 2)]]) / 2,
              v[[count]])
        }
        out <- c(describe(Mod(x)), describe(Arg(x)))
        names(out) <- c("Min.Mod", "Median.Mod", "Max.Mod", "Min.Arg", "Median.Arg", "Max.Arg")
    } else if (is.list(x)) {
        classes <- vapply(x, function(y) if (is.null(oldClass(y))) "-none-" else oldClass(y)[[1L]], "")
        labels <- names(x)
        if (is.null(labels)) labels <- rep("", n)
        out <- cbind(as.character(lengths(x, use.names = FALSE)), classes, vapply(x, mode, ""))
        dimnames(out) <- list(labels, c("Length", "Class", "Mode"))
        return(structure(out, class = result.class))
    } else {
        out <- c(Length = as.character(n), Class = class(x)[[1L]], Mode = mode(x))
        return(structure(out, class = result.class))
    }
    nas <- sum(is.na(x))
    if (nas) out <- c(out, NAs = nas)
    structure(out, class = result.class)
}
summary.factor <- function(object, maxsum = 100L, ...) {
    counts <- vapply(levels(object), function(level) sum(object == level, na.rm = TRUE), 0L)
    nas <- sum(is.na(object))
    limit <- maxsum - as.integer(nas > 0L)
    if (length(counts) > limit && limit > 0L) {
        i <- order(counts, decreasing = TRUE)[seq_len(limit - 1L)]
        counts <- c(counts[i], "(Other)" = sum(counts) - sum(counts[i]))
    }
    if (nas) counts <- c(counts, NAs = nas)
    counts
}
summary.matrix <- function(object, ..., digits = max(3L, getOption("digits") - 3L)) {
    labels <- colnames(object)
    if (is.null(labels)) labels <- paste0("V", seq_len(ncol(object)))
    ans <- lapply(seq_len(ncol(object)), function(i) {
        vals <- summary.default(object[, i], ...)
        paste0(format(names(vals)), ":", format(unclass(vals), digits = digits), "  ")
    })
    rows <- max(lengths(ans))
    out <- matrix("", nrow = rows, ncol = length(ans))
    for (i in seq_along(ans)) out[seq_along(ans[[i]]), i] <- ans[[i]]
    dimnames(out) <- list(rep("", rows), format(labels, justify = "centre", width = max(nchar(out))))
    structure(out, class = "table")
}

.model_levels <- function(n) {
    if (length(n) == 1L && is.numeric(n)) {
        if (n < 2L) stop("not enough degrees of freedom to define contrasts")
        return(as.character(seq_len(n)))
    }
    if (length(n) < 2L) stop("not enough degrees of freedom to define contrasts")
    as.character(n)
}
.model_indicators <- function(levels) {
    k <- length(levels)
    out <- diag(k)
    dimnames(out) <- list(levels, levels)
    out
}
contr.treatment <- function(n, base = 1, contrasts = TRUE, sparse = FALSE) {
    labels <- .model_levels(n)
    out <- .model_indicators(labels)
    if (!contrasts) return(out)
    if (length(base) != 1L || base < 1L || base > length(labels)) stop("baseline group number out of range")
    out[, -base, drop = FALSE]
}
contr.SAS <- function(n, contrasts = TRUE, sparse = FALSE) {
    labels <- .model_levels(n)
    contr.treatment(labels, base = length(labels), contrasts = contrasts, sparse = sparse)
}
contr.sum <- function(n, contrasts = TRUE, sparse = FALSE) {
    labels <- .model_levels(n)
    out <- .model_indicators(labels)
    if (!contrasts) return(out)
    k <- length(labels)
    out <- out[, -k, drop = FALSE]
    out[k, ] <- -1
    dimnames(out) <- list(labels, NULL)
    out
}
contr.helmert <- function(n, contrasts = TRUE, sparse = FALSE) {
    labels <- .model_levels(n)
    if (!contrasts) return(.model_indicators(labels))
    k <- length(labels)
    out <- matrix(0, k, k - 1L, dimnames = list(labels, NULL))
    for (i in seq_len(k - 1L)) {
        out[seq_len(i), i] <- -1
        out[i + 1L, i] <- i
    }
    out
}
contr.poly <- function(n, scores = seq_len(n), contrasts = TRUE, sparse = FALSE) {
    if (n < 2L) stop("contrasts not defined for ", n - 1L, " degrees of freedom")
    if (n > 95L) stop("orthogonal polynomials cannot be represented accurately enough for ", n - 1L, " degrees of freedom")
    if (length(scores) != n) stop("'scores' argument is of the wrong length")
    if (any(is.na(scores)) || length(.Internal(C_model_distinct(scores))) != n) stop("'scores' must all be different numbers")
    degrees <- seq_len(n - 1L)
    out <- poly(scores, degree = n - 1L, simple = TRUE)
    if (contrasts) {
        labels <- paste0("^", degrees)
        labels[seq_len(min(3L, length(labels)))] <- c(".L", ".Q", ".C")[seq_len(min(3L, length(labels)))]
        dimnames(out) <- list(NULL, labels)
        return(out)
    }
    out <- cbind(1, out)
    dimnames(out) <- list(NULL, paste0("^", 0:(n - 1L)))
    out
}

.model_default_contrast <- function(x) getOption("contrasts", c("contr.treatment", "contr.poly"))[[if (inherits(x, "ordered")) 2L else 1L]]
.model_contrast_fun <- function(x, env) {
    if (is.function(x)) return(x)
    if (is.character(x)) {
        if (x == "contr.treatment") return(contr.treatment)
        if (x == "contr.sum") return(contr.sum)
        if (x == "contr.helmert") return(contr.helmert)
        if (x == "contr.poly") return(contr.poly)
        if (x == "contr.SAS") return(contr.SAS)
        return(get(x, mode = "function", envir = env))
    }
    stop("invalid contrast function")
}
.model_contrast_matrix <- function(x, spec, env, sparse = FALSE) {
    if (is.matrix(spec)) return(spec)
    f <- .model_contrast_fun(spec, env)
    input <- if (identical(f, contr.poly)) length(levels(x)) else levels(x)
    if (sparse) f(input, contrasts = TRUE, sparse = sparse)
    else f(input, contrasts = TRUE)
}
contrasts <- function(x, contrasts = TRUE, sparse = FALSE) {
    if (is.logical(x)) x <- .model_factor(x)
    if (!is.factor(x)) stop("contrasts apply only to factors")
    if (!contrasts) return(.model_indicators(levels(x)))
    spec <- attr(x, "contrasts", exact = TRUE)
    if (is.null(spec)) spec <- .model_default_contrast(x)
    .model_contrast_matrix(x, spec, parent.frame(), sparse)
}
`contrasts<-` <- function(x, how.many = NULL, value) {
    if (is.logical(x)) x <- .model_factor(x)
    if (!is.factor(x)) stop("contrasts apply only to factors")
    n <- length(levels(x))
    if (n < 2L) stop("contrasts can be applied only to factors with 2 or more levels")
    if (is.function(value)) {
        input <- if (identical(value, contr.poly)) length(levels(x)) else levels(x)
        value <- value(input)
    }
    if (is.matrix(value)) {
        if (nrow(value) != n) stop("wrong number of contrast matrix rows")
        if (is.null(how.many)) how.many <- n - 1L
        if (ncol(value) < how.many) {
            q <- qr(cbind(1, value))
            if (q$rank < ncol(value) + 1L) stop("singular contrast matrix")
            value <- cbind(value, qr.Q(q, complete = TRUE)[, seq_len(how.many - ncol(value)) + ncol(value) + 1L, drop = FALSE])
        } else if (ncol(value) > how.many) {
            value <- value[, seq_len(how.many), drop = FALSE]
        }
        dimnames(value) <- list(levels(x), colnames(value))
    } else if (!is.null(value) && !is.character(value)) stop("numeric contrasts or contrast name expected")
    attr(x, "contrasts") <- value
    x
}
C <- function(object, contr, how.many, ...) {
    if (!is.factor(object)) object <- ordered(object)
    if (missing(contr)) contr <- getOption("contrasts")[(if (inherits(object, "ordered")) 2L else 1L)]
    if (is.function(contr)) {
        input <- if (identical(contr, contr.poly)) length(levels(object)) else levels(object)
        contr <- contr(input, ...)
    }
    if (missing(how.many)) contrasts(object) <- contr
    else contrasts(object, how.many) <- contr
    object
}
relevel <- function(x, ref, ...) UseMethod("relevel")
relevel.default <- function(x, ref, ...) stop("'relevel' only for (unordered) factors")
relevel.ordered <- function(x, ref, ...) stop("'relevel' only for unordered factors")
relevel.factor <- function(x, ref, ...) {
    labels <- levels(x)
    if (length(ref) != 1L) stop("'ref' must be of length one")
    if (is.character(ref)) {
        ref <- .model_match(ref, labels)
        if (is.na(ref)) stop("'ref' must be an existing level")
    }
    if (is.na(ref) || ref < 1L || ref > length(labels)) stop("'ref' = ", ref, " must be in 1L:", length(labels))
    factor(x, levels = c(labels[ref], labels[-ref]))
}

model.matrix <- function(object, ...) UseMethod("model.matrix")
model.matrix.lm <- function(object, ...) {
    if (!is.null(object$x)) return(object$x)
    model.matrix.default(terms(object), model.frame(object), contrasts.arg = object$contrasts, ...)
}
model.matrix.default <- function(object, data = environment(object), contrasts.arg = NULL, xlev = NULL, ...) {
    caller <- parent.frame()
    tt <- if (inherits(object, "terms")) object else if (inherits(object, "formula")) terms(object, data = data) else terms(object)
    vars <- as.list(attr(tt, "variables"))[-1L]
    labels <- .Internal(C_model_labels(vars))
    if (is.null(attr(data, "terms"))) data <- model.frame(tt, data, xlev = xlev, ...)
    indices <- .model_match(labels, names(data))
    if (any(is.na(indices))) stop("model frame and formula mismatch in model.matrix()")
    rows <- as.character(.Internal(C_model_rownames(data)))
    columns <- as.list(data)[indices]
    coding <- rep(list(NULL), length(columns))
    contrast_spec <- list()
    response <- attr(tt, "response")
    if (!is.null(contrasts.arg) && !is.list(contrasts.arg)) {
        warning("non-list contrasts argument ignored")
        contrasts.arg <- NULL
    }
    if (length(contrasts.arg) && is.null(names(contrasts.arg))) stop("'contrasts.arg' argument must be named")
    for (name in names(contrasts.arg)) {
        i <- .model_match(name, labels)
        if (is.na(i)) warning(paste0("variable '", name, "' is absent, its contrast will be ignored"))
        else if (!is.factor(columns[[i]]) && !is.character(columns[[i]]) && !is.logical(columns[[i]])) stop("contrasts apply only to factors")
    }
    for (j in seq_along(columns)) {
        x <- columns[[j]]
        if (is.character(x) || is.logical(x)) x <- .model_factor(x)
        columns[[j]] <- x
        if (is.factor(x)) {
            if (length(levels(x)) < 2L) {
                if (j == response) next
                stop("contrasts can be applied only to factors with 2 or more levels")
            }
            name <- labels[[j]]
            spec <- contrasts.arg[[name]]
            if (!is.null(spec)) {
                contrasts(x, if (is.matrix(spec)) ncol(spec) else NULL) <- spec
            }
            spec <- attr(x, "contrasts", exact = TRUE)
            if (is.null(spec)) spec <- .model_default_contrast(x)
            if (j != response) contrast_spec[name] <- list(spec)
            coding[j] <- list(.model_contrast_matrix(x, spec, caller))
        }
    }
    if (!length(contrast_spec)) contrast_spec <- NULL
    .Internal(C_modelmatrix(tt, columns, coding, contrast_spec, rows))
}

options(na.action = "na.omit", contrasts = c(unordered = "contr.treatment", ordered = "contr.poly"))

dummy.coef <- function(object, ...) UseMethod("dummy.coef")
dummy.coef.lm <- function(object, use.na = FALSE, ...) {
    tt <- terms(object)
    labels <- attr(tt, "term.labels")
    b <- coef(object)
    frame <- if (length(labels)) model.frame(object) else NULL
    factors <- attr(tt, "factors")
    level.map <- object[["xlevels"]]
    multi <- is.matrix(b)
    for (name in rownames(factors)) {
        if (is.logical(frame[[name]])) level.map[name] <- list(c("FALSE", "TRUE"))
    }
    if (!length(level.map)) {
        if (!multi) return(as.list(b))
        return(as.list(as.double(b)))
    }
    if (!use.na) b[is.na(b)] <- 0
    ans <- list()
    if (attr(tt, "intercept")) {
        ans["(Intercept)"] <- list(if (multi) b[1L, ] else b[1L])
    }
    for (j in seq_along(labels)) {
        involved <- rownames(factors)[factors[, j] != 0]
        categorical <- involved[!is.na(.model_match(involved, names(level.map)))]
        widths <- vapply(level.map[categorical], length, 0L)
        n <- prod(widths)
        .Internal(C_lm_size(n))
        columns <- list()
        row.labels <- labels[[j]]
        stride <- 1L
        for (name in rownames(factors)) {
            levels <- level.map[[name]]
            if (is.null(levels)) {
                columns[name] <- list(rep(1, n))
            } else {
                level <- rep(levels[[1L]], n)
                if (any(name == categorical)) {
                    level <- rep(rep(levels, each = stride), length.out = n)
                    row.labels <- if (stride == 1L && name == categorical[[1L]]) level
                                  else paste(row.labels, level, sep = ":")
                    stride <- stride * length(levels)
                }
                value <- if (is.logical(frame[[name]])) level == "TRUE"
                         else factor(level, levels = levels, ordered = inherits(frame[[name]], "ordered"))
                columns[name] <- list(value)
            }
        }
        data <- structure(columns, class = "data.frame", row.names = seq_len(n), terms = tt)
        X <- model.matrix(tt, data, contrasts.arg = object[["contrasts"]])
        selected <- attr(X, "assign") == j
        X <- X[, selected, drop = FALSE]
        coefficient <- if (multi) b[selected, , drop = FALSE] else matrix(b[selected], ncol = 1L)
        value <- matrix(0, n, ncol(coefficient),
                        dimnames = list(row.labels, colnames(coefficient)))
        for (i in seq_len(ncol(X))) {
            nonzero <- X[, i] != 0
            for (k in seq_len(ncol(coefficient))) {
                value[nonzero, k] <- value[nonzero, k] + X[nonzero, i] * coefficient[i, k]
            }
        }
        ans[labels[[j]]] <- list(if (multi) t(value) else setNames(as.double(value), row.labels))
    }
    structure(ans, class = "dummy_coef", matrix = multi)
}

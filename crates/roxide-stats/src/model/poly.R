makepredictcall <- function(var, call) UseMethod("makepredictcall")
makepredictcall.default <- function(var, call) call
makepredictcall.poly <- function(var, call) {
    if (is.null(attr(var, "coefs")) || typeof(call) != "language") return(call)
    scope <- parent.frame()
    if (exists(".model_formula_env", envir = scope, inherits = FALSE)) {
        scope <- get(".model_formula_env", envir = scope, inherits = FALSE)
    }
    head <- call[[1L]]
    if (typeof(head) == "symbol") {
        fun <- tryCatch(get(as.character(head), mode = "function", envir = scope), error = function(e) NULL)
    } else if (typeof(head) == "language" && as.character(head[[1L]]) == "::") {
        fun <- tryCatch(eval(head, scope), error = function(e) NULL)
    } else {
        return(call)
    }
    if (identical(fun, poly)) .Internal(C_model_predvar(call, var)) else call
}
poly <- function(x, ..., degree = 1, coefs = NULL, raw = FALSE, simple = FALSE) {
    dots <- list(...)
    if (length(dots) == 1L && length(dots[[1L]]) == 1L && missing(degree)) {
        degree <- dots[[1L]]
        dots <- list()
    }
    if (is.matrix(x)) {
        columns <- list()
        for (i in seq_len(ncol(x))) columns[[i]] <- x[, i]
        if (length(columns) > 1L || length(dots)) return(.model_polym(c(columns, dots), degree, if (length(dots)) NULL else coefs, raw, simple))
        x <- columns[[1L]]
    }
    if (length(dots)) return(.model_polym(c(list(x), dots), degree, NULL, raw, simple))
    if (is.factor(x) && !inherits(x, "ordered") && !raw) x <- as.character(x)
    .Internal(C_model_poly(x, degree, coefs, raw, simple))
}
.model_polym <- function(xs, degree, coefs, raw, simple = FALSE) {
    fits <- list()
    for (j in seq_along(xs)) {
        fits[[j]] <- poly(xs[[j]], degree = degree, coefs = coefs[[j]], raw = raw)
    }
    bases <- lapply(fits, function(fit) cbind(1, fit))
    powers <- matrix(0L, nrow = 1L, ncol = length(xs))
    for (k in seq_along(xs)) {
        old <- powers
        powers <- matrix(0L, nrow = nrow(old) * (degree + 1L), ncol = ncol(old))
        for (j in 0:degree) {
            i <- seq_len(nrow(old)) + j * nrow(old)
            powers[i, ] <- old
            powers[i, k] <- j
        }
    }
    total <- rowSums(powers)
    keep <- total > 0 & total <= degree
    positions <- seq_along(total)[keep]
    powers <- powers[keep, , drop = FALSE]
    total <- total[keep]
    columns <- list()
    labels <- character(nrow(powers))
    for (j in seq_len(nrow(powers))) {
        value <- rep(1, nrow(bases[[1L]]))
        for (k in seq_along(xs)) value <- value * bases[[k]][, powers[j, k] + 1L]
        columns[[j]] <- value
        labels[j] <- paste(powers[j, ], collapse = ".")
    }
    out <- do.call("cbind", columns)
    names(labels) <- as.character(positions)
    dimnames(out) <- list(rownames(out), labels)
    if (!simple && is.null(coefs)) {
        attr(out, "degree") <- as.integer(total)
        if (!raw) attr(out, "coefs") <- lapply(fits, function(fit) attr(fit, "coefs"))
        class(out) <- c("poly", "matrix")
    }
    out
}
polym <- function(..., degree = 1, coefs = NULL, raw = FALSE) {
    dots <- list(...)
    if (length(dots) == 1L) return(poly(dots[[1L]], degree = degree, coefs = coefs, raw = raw))
    .model_polym(dots, degree, coefs, raw)
}
predict <- function(object, ...) UseMethod("predict")
predict.poly <- function(object, newdata, ...) {
    if (missing(newdata)) return(object)
    poly(newdata, degree = max(attr(object, "degree")), coefs = attr(object, "coefs"),
         raw = is.null(attr(object, "coefs")), simple = TRUE)
}

.multivar_scale <- function(x, center, scale) {
    nr <- nrow(x)
    nc <- ncol(x)
    if (is.logical(center)) {
        if (center) center <- colMeans(x, na.rm = TRUE)
    } else {
        if (length(center) != nc)
            stop("length of 'center' must equal the number of columns of 'x'")
    }
    if (!is.logical(center)) {
        dimnames <- dimnames(x)
        x <- x - matrix(rep(center, each = nr), nr, nc)
        dimnames(x) <- dimnames
    }
    if (is.logical(scale)) {
        if (scale) {
            scale <- numeric(nc)
            for (j in seq_len(nc)) {
                v <- x[, j]
                v <- v[!is.na(v)]
                scale[j] <- sqrt(sum(v * v) / max(1L, length(v) - 1L))
            }
            names(scale) <- colnames(x)
        }
    } else {
        if (length(scale) != nc)
            stop("length of 'scale' must equal the number of columns of 'x'")
    }
    if (!is.logical(scale)) {
        dimnames <- dimnames(x)
        x <- x / matrix(rep(scale, each = nr), nr, nc)
        dimnames(x) <- dimnames
    }
    list(matrix = x, center = center, scale = scale)
}

prcomp <- function(x, ...) UseMethod("prcomp")
prcomp.default <- function(x, retx = TRUE, center = TRUE, scale. = FALSE,
                           tol = NULL, rank. = NULL, ...) {
    x <- as.matrix(x)
    scaled <- .multivar_scale(x, center, scale.)
    x <- scaled$matrix
    if (any(scaled$scale == 0) && !identical(scaled$scale, FALSE))
        stop("cannot rescale a constant/zero column to unit variance")
    n <- nrow(x)
    p <- ncol(x)
    rank <- min(n, p)
    if (!is.null(rank.)) {
        rank. <- as.integer(rank.)
        if (length(rank.) != 1L || is.na(rank.) || rank. < 1L)
            stop("as.integer(rank.) > 0 is not TRUE")
        rank <- min(rank, rank.)
    }
    s <- svd(x, nu = 0L, nv = rank)
    sdev <- s$d / sqrt(max(1L, n - 1L))
    rotation <- s$v
    if (!is.null(tol)) {
        wanted <- sum(sdev > sdev[1L] * tol)
        if (wanted < rank) {
            if (wanted == 0L) stop("length of 'dimnames' [2] not equal to array extent")
            rank <- wanted
            rotation <- rotation[, seq_len(rank), drop = FALSE]
        }
    }
    dimnames(rotation) <- list(colnames(x), paste0("PC", seq_len(rank)))
    result <- list(sdev = sdev, rotation = rotation, center = scaled$center,
                   scale = scaled$scale)
    if (retx) result$x <- x %*% rotation
    class(result) <- "prcomp"
    result
}

prcomp.formula <- function(formula, data = NULL, subset, na.action, ...) {
    cl <- match.call()
    frame <- .multivar_model_frame(cl, parent.frame())
    result <- prcomp.default(as.matrix(frame, rownames.force = TRUE), ...)
    result$call <- .Internal(C_multivar_call(cl, "prcomp"))
    na <- attr(frame, "na.action")
    if (!is.null(na)) {
        result$na.action <- na
        if (!is.null(result$x)) result$x <- napredict(na, result$x)
    }
    result
}

.multivar_model_frame <- function(cl, env) {
    object <- eval(.Internal(C_multivar_call(cl, "model.frame")), env)
    if (attr(terms(object), "response") != 0L) stop("response not allowed in formula")
    if (!all(vapply(object, is.numeric, FALSE))) stop("PCA applies only to numerical variables")
    object
}

.multivar_predict <- function(newdata, rotation, center, scale) {
    names <- rownames(rotation)
    newdata <- as.matrix(newdata)
    if (is.null(names)) {
        if (ncol(newdata) != nrow(rotation))
            stop("'newdata' does not have the correct number of columns")
    } else {
        positions <- .model_match(names, colnames(newdata))
        if (any(is.na(positions)))
            stop("'newdata' does not have named columns matching one or more of the original columns")
        newdata <- newdata[, positions, drop = FALSE]
    }
    .multivar_scale(newdata, center, scale)$matrix %*% rotation
}

predict.prcomp <- function(object, newdata, ...) {
    if (missing(newdata)) {
        if (is.null(object$x)) stop("no scores are available: refit with 'retx=TRUE'")
        return(object$x)
    }
    .multivar_predict(newdata, object$rotation, object$center, object$scale)
}

.multivar_importance <- function(sdev, labels, rounded) {
    variance <- sdev^2 / sum(sdev^2)
    cumulative <- cumsum(variance)
    if (rounded) {
        variance <- round(variance, 5L)
        cumulative <- round(cumulative, 5L)
    }
    labels <- c(labels, rep("", length(sdev) - length(labels)))
    result <- rbind(sdev, variance, cumulative)
    dimnames(result) <- list(c("Standard deviation", "Proportion of Variance",
                              "Cumulative Proportion"), labels)
    result
}
summary.prcomp <- function(object, ...) {
    object$importance <- .multivar_importance(object$sdev, colnames(object$rotation), TRUE)
    class(object) <- "summary.prcomp"
    object
}
print.prcomp <- function(x, print.x = FALSE, ..., digits = getOption("digits")) {
    cat("Standard deviations (1, .., p=", length(x$sdev), "):\n", sep = "")
    print(x$sdev, digits = digits, ...)
    d <- dim(x$rotation)
    cat("\nRotation (n x k) = (", d[1L], " x ", d[2L], "):\n", sep = "")
    .Internal(C_multivar_print_matrix(x$rotation, digits))
    if (print.x && !is.null(x$x)) {
        cat("\nRotated variables:\n")
        .Internal(C_multivar_print_matrix(x$x, digits))
    }
    invisible(x)
}
print.summary.prcomp <- function(x, digits = max(3L, getOption("digits") - 3L), ...) {
    rank <- ncol(x$rotation)
    total <- length(x$sdev)
    if (rank < total) {
        cat("Importance of first k=", rank, " (out of ", total, ") components:\n", sep = "")
    } else cat("Importance of components:\n")
    .Internal(C_multivar_print_matrix(x$importance[, seq_len(rank), drop = FALSE], digits))
    invisible(x)
}

princomp <- function(x, ...) UseMethod("princomp")
princomp.default <- function(x, cor = FALSE, scores = TRUE, covmat = NULL,
                             subset = TRUE, fix_sign = TRUE, ...) {
    cl <- match.call()
    cl <- .Internal(C_multivar_call(cl, "princomp"))
    data <- if (missing(x)) NULL else as.matrix(x)[subset, , drop = FALSE]
    score.data <- data
    if (is.null(covmat)) {
        if (is.null(data)) stop("argument is of length zero")
        n <- nrow(data)
        if (n < ncol(data)) stop("'princomp' can only be used with more units than variables")
        if (!is.numeric(data)) stop("'x' must be numeric")
        if (any(!is.finite(data))) stop("'x' must contain finite values only")
        center <- colMeans(data)
        z <- .multivar_scale(data, center, FALSE)$matrix
        if (n == 1L && ncol(z) > 0L) stop("infinite or missing values in 'x'")
        covariance <- crossprod(z) / n
    } else {
        if (is.list(covmat)) {
            if (is.null(covmat$cov) || is.null(covmat$center) || is.null(covmat$n.obs))
                stop("'covmat' is not a valid covariance list")
            covariance <- covmat$cov
            center <- covmat$center
            n <- covmat$n.obs
        } else if (is.matrix(covmat)) {
            if (!missing(x)) warning("both 'x' and 'covmat' were supplied: 'x' will be ignored")
            score.data <- NULL
            covariance <- covmat
            center <- rep(NA_real_, ncol(covmat))
            n <- NA
        } else stop("'covmat' is of unknown type")
    }
    if (!is.matrix(covariance) || nrow(covariance) != ncol(covariance))
        stop("'covmat' is not square")
    variables <- colnames(covariance)
    p <- ncol(covariance)
    scale <- rep(1, p)
    if (cor) {
        scale <- sqrt(diag(covariance))
        if (any(scale == 0)) stop("cannot use 'cor = TRUE' with a constant variable")
        covariance <- covariance / outer(scale, scale)
    }
    names(scale) <- variables
    e <- eigen(covariance, symmetric = TRUE)
    values <- e$values
    if (any(values < -9 * 2^-52 * abs(values[1L])))
        stop("covariance matrix is not non-negative definite")
    values[values < 0] <- 0
    components <- paste0("Comp.", seq_len(p))
    if (fix_sign) {
        for (j in seq_len(p)) {
            if (e$vectors[1L, j] < 0) e$vectors[, j] <- -e$vectors[, j]
        }
    }
    rotation <- e$vectors
    dimnames(rotation) <- list(if (is.null(data)) variables else colnames(data), components)
    transformed <- if (scores && !is.null(score.data))
        .multivar_scale(score.data, center, scale)$matrix %*% rotation else NULL
    class(rotation) <- "loadings"
    structure(list(sdev = setNames(sqrt(values), components), loadings = rotation,
                   center = center, scale = scale, n.obs = n, scores = transformed, call = cl),
              class = "princomp")
}

princomp.formula <- function(formula, data = NULL, subset, na.action, ...) {
    cl <- match.call()
    frame <- .multivar_model_frame(cl, parent.frame())
    result <- princomp.default(as.matrix(frame, rownames.force = TRUE), ...)
    result$call <- .Internal(C_multivar_call(cl, "princomp"))
    na <- attr(frame, "na.action")
    if (!is.null(na)) {
        result$na.action <- na
        if (!is.null(result$scores)) result$scores <- napredict(na, result$scores)
    }
    result
}
predict.princomp <- function(object, newdata, ...) {
    if (missing(newdata)) return(object$scores)
    .multivar_predict(newdata, unclass(object$loadings), object$center, object$scale)
}
summary.princomp <- function(object, loadings = FALSE, cutoff = 0.1, ...) {
    object$cutoff <- cutoff
    object$print.loadings <- loadings
    class(object) <- "summary.princomp"
    object
}
print.princomp <- function(x, ..., digits = getOption("digits")) {
    cat("Call:\n")
    print(x$call)
    cat("\nStandard deviations:\n")
    .Internal(C_multivar_print_named(x$sdev, digits))
    cat("\n ", length(x$sdev), "  variables and  ", x$n.obs, " observations.\n", sep = "")
    invisible(x)
}
print.summary.princomp <- function(x, ..., digits = getOption("digits")) {
    cat("Importance of components:\n")
    importance <- .multivar_importance(x$sdev, names(x$sdev), FALSE)
    .Internal(C_multivar_print_matrix(importance, digits))
    if (x$print.loadings) print(x$loadings, cutoff = x$cutoff, ...)
    invisible(x)
}
print.loadings <- function(x, digits = 3L, cutoff = 0.1, ...) {
    cat("\nLoadings:\n")
    output <- matrix(format(round(as.double(x), digits)), nrow(x), ncol(x),
                     dimnames = dimnames(x))
    output[abs(x) < cutoff] <- ""
    print(output, quote = FALSE, ...)
    ss <- colSums(x^2)
    cat("\n")
    print(rbind("SS loadings" = ss, "Proportion Var" = ss/nrow(x),
                "Cumulative Var" = cumsum(ss)/nrow(x)), digits = digits, ...)
    invisible(x)
}

as.matrix <- function(x, ...) UseMethod("as.matrix")
as.matrix.default <- function(x, ...) {
    if (is.matrix(x)) return(x)
    if (is.null(x)) stop("'data' must be of a vector type, was 'NULL'")
    out <- matrix(x, ncol = 1L)
    if (!is.null(names(x))) dimnames(out) <- list(names(x), NULL)
    out
}
as.matrix.data.frame <- function(x, rownames.force = NA, ...) {
    nr <- nrow(x)
    rn <- NULL
    if (identical(rownames.force, TRUE) || (!identical(rownames.force, FALSE) &&
        .Internal(C_multivar_explicit_rows(x)))) rn <- rownames(x)
    if (!ncol(x)) return(matrix(logical(), nr, 0L, dimnames = list(rn, NULL)))
    columns <- as.list(x)
    text <- FALSE
    for (column in columns) {
        if (is.factor(column) || is.character(column)) text <- TRUE
    }
    if (text) columns <- lapply(columns, as.character)
    result <- do.call(cbind, columns)
    if (!is.matrix(result)) result <- matrix(result, nr)
    dimnames(result) <- list(rn, colnames(result))
    result
}

dist <- function(x, method = "euclidean", diag = FALSE, upper = FALSE, p = 2) {
    cl <- match.call()
    methods <- c("euclidean", "maximum", "manhattan", "canberra", "binary", "minkowski")
    method <- .Internal(C_multivar_match(method, methods, "dist"))
    .Internal(C_multivar_dist(as.matrix(x), method, diag, upper,
                              if (method == "minkowski") p else NULL, cl))
}

as.dist <- function(m, diag = FALSE, upper = FALSE) UseMethod("as.dist")
as.dist.default <- function(m, diag = FALSE, upper = FALSE) {
    if (!inherits(m, "dist")) {
        cl <- match.call()
        m <- as.matrix(m)
        if (nrow(m) != ncol(m)) warning("non-square matrix")
        rn <- rownames(m)
        if (is.null(rn)) rn <- colnames(m)
        m <- structure(m[lower.tri(m)], Labels = rn, Size = nrow(m),
                       call = cl, class = "dist")
    }
    if (is.null(attr(m, "Diag")) || !missing(diag)) attr(m, "Diag") <- diag
    if (is.null(attr(m, "Upper")) || !missing(upper)) attr(m, "Upper") <- upper
    m
}
as.matrix.dist <- function(x, ...) .Internal(C_multivar_dist_matrix(x))
print.dist <- function(x, diag = NULL, upper = NULL, digits = getOption("digits"),
                       justify = "none", right = TRUE, ...) {
    .Internal(C_multivar_print_dist(x, diag, upper, digits, right))
    invisible(x)
}

hclust <- function(d, method = "complete", members = NULL) {
    cl <- match.call()
    if (method == "ward") {
        message('The "ward" method has been renamed to "ward.D"; note new "ward.D2"')
        method <- "ward.D"
    }
    methods <- c("ward.D", "single", "complete", "average", "mcquitty", "median",
                 "centroid", "ward.D2")
    method <- .Internal(C_multivar_match(method, methods, "hclust"))
    .Internal(C_multivar_hclust(d, method, members, cl))
}
print.hclust <- function(x, ...) {
    cat("\nCall:\n")
    print(x$call)
    cat("\nCluster method   :", x$method, "\n")
    if (!is.null(x$dist.method)) cat("Distance         :", x$dist.method, "\n")
    cat("Number of objects:", length(x$order), "\n\n")
    invisible(x)
}
cutree <- function(tree, k = NULL, h = NULL) .Internal(C_multivar_cutree(tree, k, h))
cmdscale <- function(d, k = 2, eig = FALSE, add = FALSE, x.ret = FALSE,
                     list. = eig || add || x.ret) {
    if (is.null(attr(d, "Size"))) {
        d <- as.matrix(d)
        if (nrow(d) != ncol(d)) stop("distances must be result of 'dist' or a square matrix")
    }
    .Internal(C_multivar_cmdscale(d, k, eig, add, x.ret, list.))
}

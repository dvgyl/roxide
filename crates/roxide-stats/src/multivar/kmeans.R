kmeans <- function(x, centers, iter.max = 10L, nstart = 1L,
                   algorithm = c("Hartigan-Wong", "Lloyd", "Forgy", "MacQueen"),
                   trace = FALSE) {
    x <- as.matrix(x)
    methods <- c("Hartigan-Wong", "Lloyd", "Forgy", "MacQueen")
    algorithm <- .Internal(C_multivar_match(algorithm, methods, "kmeans"))
    if (length(centers) != 1L) {
        centers <- as.matrix(centers)
        return(.Internal(C_multivar_kmeans(x, centers, iter.max, algorithm, trace)))
    }
    k <- as.integer(centers)
    nstart <- as.integer(nstart)
    if (is.na(k) || k < 0L) stop("invalid 'size' argument")
    if (is.na(nstart)) stop("missing value where TRUE/FALSE needed")
    if (nstart > 100000L) stop("too many starts")
    nr <- nrow(x)
    if (k > nr) stop("cannot take a sample larger than the population when 'replace = FALSE'")
    if (nstart <= 1L) {
        centers <- x[sample.int(nr, k), , drop = FALSE]
        if (length(.Internal(C_multivar_unique_rows(centers))) == k) {
            return(.Internal(C_multivar_kmeans(x, centers, iter.max, algorithm, trace)))
        }
    }
    distinct <- .Internal(C_multivar_unique_rows(x))
    if (length(distinct) < k) stop("more cluster centers than distinct data points.")
    best <- NULL
    for (i in seq_len(max(1L, nstart))) {
        centers <- x[distinct[sample.int(length(distinct), k)], , drop = FALSE]
        fit <- .Internal(C_multivar_kmeans(x, centers, iter.max, algorithm, trace))
        if (is.null(best) || fit$tot.withinss < best$tot.withinss) best <- fit
    }
    best
}

fitted <- function(object, ...) UseMethod("fitted")
fitted.default <- function(object, ...) {
    value <- object$fitted.values
    if (is.null(value)) value <- object$fitted
    napredict(object$na.action, value)
}
fitted.kmeans <- function(object, method = c("centers", "classes"), ...) {
    methods <- c("centers", "classes")
    method <- .Internal(C_multivar_match(method, methods, "fitted.kmeans"))
    if (method == "classes") return(object$cluster)
    object$centers[object$cluster, , drop = FALSE]
}
print.kmeans <- function(x, ..., digits = getOption("digits")) {
    cat("K-means clustering with ", length(x$size), " clusters of sizes ",
        paste(x$size, collapse = ", "), "\n\nCluster means:\n", sep = "")
    .Internal(C_multivar_print_matrix(x$centers, digits))
    cat("\nClustering vector:\n")
    .Internal(C_multivar_print_named(x$cluster, digits))
    cat("\nWithin cluster sum of squares by cluster:\n")
    print(x$withinss, digits = digits, ...)
    cat(sprintf(" (between_SS / total_SS = %5.1f %%)\n", 100 * x$betweenss / x$totss))
    cat("\nAvailable components:\n\n")
    .Internal(C_multivar_print_fields(names(x)))
    invisible(x)
}

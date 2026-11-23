var <- function(x, y = NULL, na.rm = FALSE, use) {
    if (missing(use)) use <- if (na.rm) "na.or.complete" else "everything"
    .Internal(C_var(x, y, use, "pearson"))
}
sd <- function(x, na.rm = FALSE) .Internal(C_sd(x, na.rm))
cov <- function(x, y = NULL, use = "everything", method = c("pearson", "kendall", "spearman")) .Internal(C_cov(x, y, use, method))
cor <- function(x, y = NULL, use = "everything", method = c("pearson", "kendall", "spearman")) .Internal(C_cor(x, y, use, method))
cov2cor <- function(V) .Internal(C_cov2cor(V))

median <- function(x, na.rm = FALSE, ...) UseMethod("median")
median.default <- function(x, na.rm = FALSE, ...) .Internal(C_median(x, na.rm))

quantile <- function(x, ...) UseMethod("quantile")
quantile.default <- function(x, probs = seq(0, 1, 0.25), na.rm = FALSE, names = TRUE,
                             type = 7, digits = 7, fuzz = if (type == 7L) 0 else 4 * 2.220446049250313e-16, ...)
    .Internal(C_quantile(x, probs, na.rm, names, type, digits, fuzz))

IQR <- function(x, na.rm = FALSE, type = 7) {
    q <- quantile(x, c(0.25, 0.75), na.rm = na.rm, names = FALSE, type = type)
    q[2L] - q[1L]
}
mad <- function(x, center = median(x), constant = 1.4826, na.rm = FALSE, low = FALSE, high = FALSE) {
    if (na.rm) x <- x[!is.na(x)]
    constant * .Internal(C_mad(abs(x - center), low, high))
}
fivenum <- function(x, na.rm = TRUE) .Internal(C_fivenum(x, na.rm))

weighted.mean <- function(x, w, ...) UseMethod("weighted.mean")
weighted.mean.default <- function(x, w, ..., na.rm = FALSE) {
    if (missing(w)) .Internal(C_weighted_mean(x, NULL, na.rm, TRUE))
    else .Internal(C_weighted_mean(x, w, na.rm, FALSE))
}

setNames <- function(object = nm, nm) {
    names(object) <- nm
    object
}

Pair <- function(x, y) structure(cbind(x, y), class = "Pair")

.htest_frame <- function(call, env, pass = FALSE) {
    args <- as.list(call)[-1L]
    args <- args[!is.na(.model_match(names(args), c("formula", "data", "subset", "na.action")))]
    if (pass && !any("na.action" == names(args))) args$na.action <- quote(stats::na.pass)
    eval(.Internal(C_htest_frame_call(args)), env)
}

.htest_groups <- function(frame) {
    g <- factor(frame[[2L]])
    labels <- levels(g)
    if (length(labels) != 2L) stop("grouping factor must have exactly 2 levels")
    list(frame[[1L]][!is.na(g) & as.integer(g) == 1L],
         frame[[1L]][!is.na(g) & as.integer(g) == 2L], labels)
}

.htest_group_formula <- function(formula, call, env, test, dots) {
    if (!inherits(formula, "formula") || length(formula) != 3L)
        stop("'formula' missing or incorrect")
    if (any("paired" == names(dots)) && (test == "t" || test == "wilcox"))
        stop("cannot use 'paired' in formula method")
    frame <- .htest_frame(call, env, test == "t" || test == "wilcox")
    labels <- names(frame)
    fun <- list(t = t.test.default, wilcox = wilcox.test.default,
                var = var.test.default, ks = ks.test.default)[[test]]
    if (length(frame) == 1L && (test == "t" || test == "wilcox") && (identical(formula[[3L]], 1) || identical(formula[[3L]], 1L))) {
        x <- frame[[1L]]
        result <- if (inherits(x, "Pair")) do.call(fun, c(list(x[,1L], x[,2L], paired = TRUE), dots)) else
            do.call(fun, c(list(x), dots))
        result$data.name <- labels[[1L]]
        return(result)
    }
    if (length(frame) != 2L) stop("'formula' missing or incorrect")
    samples <- .htest_groups(frame)
    result <- do.call(fun, c(samples[1L:2L], dots))
    result$data.name <- paste(labels[[1L]], "by", labels[[2L]])
    group <- samples[[3L]]
    if (test == "t") {
        names(result$estimate) <- paste("mean in group", group)
        names(result$null.value) <- paste("difference in means between group", group[[1L]], "and group", group[[2L]])
    }
    if (test == "ks") {
        if (result$alternative != "two-sided")
            result$alternative <- paste("the CDF of", group[[1L]], "lies",
                                         if (names(result$statistic) == "D^-") "below" else "above",
                                         "that of", group[[2L]])
        result$response <- labels[[1L]]
        result$groups <- group
    }
    result
}
t.test.formula <- function(formula, data, subset, na.action = na.pass, ...) {
    if (missing(formula)) stop("'formula' missing or incorrect")
    .htest_group_formula(formula, match.call(expand.dots = FALSE), parent.frame(), "t", list(...))
}
wilcox.test.formula <- function(formula, data, subset, na.action = na.pass, ...) {
    if (missing(formula)) stop("'formula' missing or incorrect")
    .htest_group_formula(formula, match.call(expand.dots = FALSE), parent.frame(), "wilcox", list(...))
}
var.test.formula <- function(formula, data, subset, na.action, ...) {
    if (missing(formula)) stop("'formula' missing or incorrect")
    .htest_group_formula(formula, match.call(expand.dots = FALSE), parent.frame(), "var", list(...))
}
ks.test.formula <- function(formula, data, subset, na.action, ...) {
    if (missing(formula)) stop("'formula' missing or incorrect")
    .htest_group_formula(formula, match.call(expand.dots = FALSE), parent.frame(), "ks", list(...))
}
cor.test.formula <- function(formula, data, subset, na.action, ...) {
    if (missing(formula) || !inherits(formula, "formula") || length(formula) != 2L)
        stop("'formula' missing or incorrect")
    frame <- .htest_frame(match.call(expand.dots = FALSE), parent.frame())
    if (length(frame) != 2L) stop("invalid formula")
    out <- cor.test.default(frame[[1L]], frame[[2L]], ...)
    out$data.name <- paste(names(frame), collapse = " and ")
    out
}

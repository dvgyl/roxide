formula <- function(x, ...) UseMethod("formula")
formula.default <- function(x, env = parent.frame(), ...) {
    f <- attr(x, "formula", exact = TRUE)
    if (!is.null(f)) return(f)
    if (is.list(x) && !is.null(x$formula)) return(formula(x$formula, env = env))
    if (is.null(x)) return(structure(list(), class = "formula", .Environment = env))
    if (typeof(x) == "language") return(.Internal(C_formula_call(x, env)))
    stop("invalid formula")
}
formula.formula <- function(x, ...) x
formula.terms <- function(x, ...) .Internal(C_formula_call(x, environment(x)))
formula.character <- function(x, env = parent.frame(), ...) {
    parsed <- parse(text = x, keep.source = FALSE)
    if (length(parsed) == 0L) stop("invalid formula")
    if (length(parsed) > 1L) warning("Using formula(x) is deprecated when x is a character vector of length > 1.")
    .Internal(C_formula_call(parsed[[1L]], env))
}
formula.data.frame <- function(x, env = parent.frame(), ...) {
    .Internal(C_formula_names(names(x), env))
}
as.formula <- function(object, env = parent.frame()) {
    if (inherits(object, "formula")) object else formula(object, env = env)
}
print.formula <- function(x, showEnv = !identical(environment(x), globalenv()), ...) {
    plain <- x
    attributes(plain) <- NULL
    cat(deparse(plain), sep = "\n")
    if (showEnv && !is.null(environment(x))) print(environment(x))
    invisible(x)
}
update <- function(object, ...) UseMethod("update")
update.formula <- function(old, new, ...) {
    f <- .Internal(C_update_formula(old, as.formula(new)))
    formula(terms(f, simplify = TRUE, allowDotAsName = TRUE))
}

terms <- function(x, ...) UseMethod("terms")
terms.default <- function(x, ...) {
    t <- attr(x, "terms", exact = TRUE)
    if (is.null(t) && is.list(x)) t <- x$terms
    if (is.null(t)) stop("no terms component nor attribute")
    t
}
terms.terms <- function(x, ...) x
terms.formula <- function(x, specials = NULL, abb = NULL, data = NULL,
                          neg.out = TRUE, keep.order = FALSE, simplify = FALSE,
                          ..., allowDotAsName = FALSE) {
    if (!is.null(data) && !is.list(data) && !is.environment(data)) stop("'data' argument is of the wrong type")
    columns <- if (is.environment(data)) NULL else names(data)
    if (!length(columns)) columns <- NULL
    .Internal(C_termsform(x, columns, specials, keep.order, simplify, allowDotAsName))
}
delete.response <- function(termobj) .Internal(C_delete_response(termobj))
drop.terms <- function(termobj, dropx = NULL, keep.response = FALSE) {
    f <- .Internal(C_drop_terms(termobj, dropx, keep.response))
    sp <- attr(termobj, "specials")
    out <- terms(f, specials = names(sp), keep.order = TRUE, allowDotAsName = TRUE)
    pred <- attr(termobj, "predvars")
    old <- as.list(attr(termobj, "variables"))[-1L]
    vars <- as.list(attr(out, "variables"))[-1L]
    selected <- .model_match(.Internal(C_model_labels(vars)), .Internal(C_model_labels(old)))
    if (!is.null(pred)) attr(out, "predvars") <- .model_call("list", as.list(pred)[selected + 1L])
    classes <- attr(termobj, "dataClasses")
    if (!is.null(classes)) attr(out, "dataClasses") <- classes[selected]
    out
}
reformulate <- function(termlabels, response = NULL, intercept = TRUE, env = parent.frame()) {
    if (!is.character(termlabels)) stop("'termlabels' must be a character vector")
    rhs <- if (length(termlabels)) paste(termlabels, collapse = " + ") else "1"
    if (!intercept) rhs <- paste(rhs, "- 1")
    lhs <- if (is.null(response)) "" else if (is.character(response)) response else deparse1(response)
    formula.character(paste(lhs, "~", rhs), env = env)
}
I <- function(x) {
    prior <- attr(x, "class", exact = TRUE)
    class(x) <- c("AsIs", prior[prior != "AsIs"])
    x
}
offset <- function(object) {
    if (!is.numeric(object)) stop("'offset' must be numeric")
    object
}

.model_call <- function(head, args) .Internal(C_model_call(head, args))
.model_match <- function(x, table) .Internal(C_model_match(x, table))
.model_rows <- function(x) {
    if (is.data.frame(x)) return(length(.Internal(C_model_rownames(x))))
    dims <- dim(x)
    if (is.null(dims)) length(x) else dims[[1L]]
}
.model_select_rows <- function(x, index) {
    dims <- dim(x)
    if (is.null(dims)) return(x[index])
    if (length(dims) == 2L) return(x[index, , drop = FALSE])
    dn <- dimnames(x)
    dim(x) <- c(dims[[1L]], prod(dims[-1L]))
    out <- x[index, , drop = FALSE]
    dim(out) <- c(nrow(out), dims[-1L])
    if (!is.null(dn)) {
        dn[1L] <- list(dn[[1L]][index])
        dimnames(out) <- dn
    }
    out
}
.model_slice_frame <- function(x, selection) {
    rows <- .Internal(C_model_rownames(x))
    index <- seq_along(rows)
    names(index) <- as.character(rows)
    index <- index[selection]
    new_rows <- .Internal(C_model_unique_rows(rows[index]))
    result <- lapply(as.list(x), function(column) .model_select_rows(column, index))
    attributes(result) <- attributes(x)
    attr(result, "row.names") <- new_rows
    result
}
.model_class <- function(x) {
    if (is.logical(x)) return("logical")
    if (inherits(x, "ordered")) return("ordered")
    if (is.factor(x)) return("factor")
    if (is.character(x)) return("character")
    if (is.matrix(x) && is.numeric(x)) return(paste0("nmatrix.", ncol(x)))
    if (is.numeric(x)) return("numeric")
    "other"
}
.model_factor <- function(x) {
    if (is.factor(x)) return(x)
    if (is.logical(x)) return(factor(x, levels = c(FALSE, TRUE)))
    factor(x)
}

complete.cases <- function(...) .Internal(C_complete_cases(list(...)))
na.omit <- function(object, ...) UseMethod("na.omit")
na.exclude <- function(object, ...) UseMethod("na.exclude")
na.fail <- function(object, ...) UseMethod("na.fail")
na.pass <- function(object, ...) object
na.action <- function(object, ...) UseMethod("na.action")
na.action.default <- function(object, ...) {
    a <- attr(object, "na.action", exact = TRUE)
    if (is.null(a) && is.list(object)) a <- object$na.action
    a
}
na.fail.default <- function(object, ...) {
    if (any(!complete.cases(object))) stop("missing values in object")
    object
}
.model_omit <- function(object, kind, frame) {
    if (frame) {
        complete <- rep(TRUE, .model_rows(object))
        for (column in as.list(object)) {
            if (is.atomic(column) && typeof(column) != "raw") complete <- complete & complete.cases(column)
        }
    } else {
        if (is.null(object) || !is.atomic(object) || typeof(object) == "raw" || length(dim(object)) > 2L) return(object)
        complete <- complete.cases(object)
    }
    if (all(complete)) return(object)
    gone <- seq_along(complete)[!complete]
    rows <- if (frame) as.character(.Internal(C_model_rownames(object))) else if (is.matrix(object)) rownames(object) else names(object)
    if (!is.null(rows)) names(gone) <- rows[gone]
    class(gone) <- kind
    result <- if (frame) .model_slice_frame(object, complete) else .model_select_rows(object, complete)
    attr(result, "na.action") <- gone
    result
}
na.omit.default <- function(object, ...) .model_omit(object, "omit", FALSE)
na.exclude.default <- function(object, ...) .model_omit(object, "exclude", FALSE)
na.omit.data.frame <- function(object, ...) .model_omit(object, "omit", TRUE)
na.exclude.data.frame <- function(object, ...) .model_omit(object, "exclude", TRUE)
naresid <- function(omit, x, ...) UseMethod("naresid")
napredict <- function(omit, x, ...) UseMethod("napredict")
naprint <- function(x, ...) UseMethod("naprint")
naresid.default <- function(omit, x, ...) x
napredict.default <- function(omit, x, ...) x
naresid.omit <- function(omit, x, ...) x
napredict.omit <- function(omit, x, ...) x
naresid.exclude <- function(omit, x, ...) {
    if (!length(omit)) return(x)
    n <- .model_rows(x) + length(omit)
    index <- rep(NA_integer_, n)
    index[-as.integer(omit)] <- seq_len(.model_rows(x))
    dims <- dim(x)
    rows <- if (is.null(dims)) names(x) else dimnames(x)[[1L]]
    out <- .model_select_rows(x, index)
    if (!is.null(rows)) {
        rows <- rows[index]
        if (!is.null(names(omit))) rows[as.integer(omit)] <- names(omit)
        if (is.null(dims)) names(out) <- rows
        else {
            dn <- dimnames(out)
            dn[1L] <- list(rows)
            dimnames(out) <- dn
        }
    }
    out
}
napredict.exclude <- function(omit, x, ...) naresid.exclude(omit, x, ...)
naprint.default <- function(x, ...) ""
naprint.omit <- function(x, ...) {
    if (length(x) == 1L) "1 observation deleted due to missingness"
    else paste(length(x), "observations deleted due to missingness")
}
naprint.exclude <- naprint.omit

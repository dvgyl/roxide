model.frame <- function(formula, ...) UseMethod("model.frame")
model.frame.lm <- function(formula, ...) {
    if (!is.null(formula$model)) return(formula$model)
    model.frame.default(terms(formula), ...)
}
model.frame.default <- function(formula, data, subset, na.action,
                                drop.unused.levels = FALSE, xlev = NULL, ...) {
    caller <- parent.frame()
    if (!inherits(formula, "formula")) {
        if (is.list(formula) && !is.null(formula$model)) return(formula$model)
        formula <- as.formula(formula, env = caller)
    }
    env <- environment(formula)
    if (is.null(env)) env <- caller
    if (missing(data)) data <- env
    if (!is.null(data) && !is.list(data) && !is.environment(data)) {
        stop("'data' must be a data.frame, environment, or list")
    }
    tt <- if (inherits(formula, "terms")) formula else terms(formula, data = data)
    vars <- attr(tt, "variables", exact = TRUE)
    vars_list <- as.list(vars)[-1L]
    var_names <- .Internal(C_model_labels(vars_list))
    pred <- attr(tt, "predvars", exact = TRUE)
    values <- eval(if (is.null(pred)) vars else pred, data, env)
    if (is.null(pred)) {
        if (length(vars_list)) {
            scope <- new.env(parent = caller)
            assign("makepredictcall", makepredictcall, envir = scope)
            assign(".model_formula_env", env, envir = scope)
            for (j in seq_along(vars_list)) {
                assign(".model_variable", values[[j]], envir = scope)
                assign(".model_expression", vars_list[[j]], envir = scope)
                vars_list[[j]] <- eval(quote(makepredictcall(.model_variable, .model_expression)), scope)
            }
        }
        attr(tt, "predvars") <- .model_call("list", vars_list)
    }
    extras <- eval(substitute(list(...)), data, env)
    extra_names <- names(extras)
    names(values) <- var_names
    for (j in seq_along(extras)) {
        value <- extras[[j]]
        if (!is.null(value)) values[paste0("(", extra_names[[j]], ")")] <- list(value)
    }
    rows <- .Internal(C_model_rownames(data))
    if (length(values)) {
        n <- .model_rows(values[[1L]])
        for (j in seq_along(values)) {
            value <- values[[j]]
            if (is.null(value) || !is.atomic(value)) {
                stop(paste0("invalid type (", typeof(value), ") for variable '", names(values)[[j]], "'"))
            }
            if (.model_rows(value) != n) stop(paste0("variable lengths differ (found for '", names(values)[[j]], "')"))
        }
        if (length(rows) != n) {
            rows <- names(values[[1L]])
            if (is.null(rows)) rows <- seq_len(n)
        }
    }
    if (is.null(rows)) rows <- integer()
    frame <- structure(values, class = "data.frame", row.names = rows)
    if (!missing(subset)) frame <- .model_slice_frame(frame, eval(substitute(subset), data, env))
    if (missing(na.action)) {
        na.action <- attr(data, "na.action")
        if (is.null(na.action)) na.action <- getOption("na.action", "na.omit")
    }
    if (is.character(na.action)) na.action <- get(na.action, envir = caller)
    if (!is.null(na.action)) {
        attr(frame, "terms") <- tt
        frame <- na.action(frame)
    }
    for (j in seq_along(frame)) {
        name <- names(frame)[[j]]
        x <- frame[[j]]
        target <- xlev[[name]]
        if (!is.null(target) && is.character(x)) x <- factor(x)
        if (is.factor(x)) {
            if (!is.null(target)) {
                observed <- levels(factor(x))
                new <- observed[is.na(.model_match(observed, target))]
                if (length(new)) stop(paste0("factor ", name, " has new levels ", paste(new, collapse = ", ")))
                frame[[j]] <- factor(x, levels = target, ordered = inherits(x, "ordered"))
            } else if (drop.unused.levels) {
                reduced <- factor(x, ordered = inherits(x, "ordered"))
                if (!identical(levels(x), levels(reduced))) frame[[j]] <- reduced
            }
        }
    }
    attr(tt, "dataClasses") <- setNames(vapply(frame, .model_class, ""), names(frame))
    attr(frame, "terms") <- tt
    frame
}

model.response <- function(data, type = "any") {
    response <- attr(attr(data, "terms"), "response")
    if (!length(data) || is.null(response) || response != 1L) return(NULL)
    response <- data[[1L]]
    if (type == "numeric" && is.factor(response)) {
        warning('using type = "numeric" with a factor response will be ignored')
    } else if (type == "double" || type == "numeric") {
        if (type == "double" || !is.integer(response)) {
            if (is.matrix(response)) response <- matrix(as.double(response), nrow(response), ncol(response), dimnames = dimnames(response))
            else response <- as.double(response)
        }
    }
    rows <- as.character(.Internal(C_model_rownames(data)))
    if (is.matrix(response)) dimnames(response) <- list(rows, colnames(response))
    else names(response) <- rows
    response
}
model.weights <- function(x) x[["(weights)"]]
model.offset <- function(x) {
    value <- x[["(offset)"]]
    for (i in attr(attr(x, "terms"), "offset")) {
        if (is.null(value)) value <- x[[i]]
        else value <- value + x[[i]]
    }
    if (!is.null(value) && !is.numeric(value)) stop("'offset' must be numeric")
    value
}
model.extract <- function(frame, component) {
    if (component == "response") return(model.response(frame))
    if (component == "offset") return(model.offset(frame))
    frame[[paste0("(", component, ")")]]
}
get_all_vars <- function(formula, data = NULL, ...) {
    caller <- parent.frame()
    if (!inherits(formula, "formula")) formula <- as.formula(formula, env = caller)
    if (is.null(data)) data <- environment(formula)
    tt <- terms(formula, data = data)
    vars <- .Internal(C_model_symbols(tt))
    scope <- .model_data_env(data, environment(tt))
    values <- lapply(vars, function(name) get(name, envir = scope))
    values <- setNames(values, vars)
    extras <- eval(substitute(list(...)), data, environment(tt))
    values <- c(values, extras)
    rows <- .Internal(C_model_rownames(data))
    if (length(values)) {
        n <- .model_rows(values[[1L]])
        for (j in seq_along(values)) {
            if (.model_rows(values[[j]]) != n) stop(paste0("variable lengths differ (found for '", names(values)[[j]], "')"))
        }
        if (length(rows) != n) rows <- seq_len(n)
    }
    if (is.null(rows)) rows <- integer()
    structure(values, class = "data.frame", row.names = rows)
}
.model_data_env <- function(data, env) {
    if (is.environment(data)) return(data)
    result <- new.env(parent = env)
    for (name in names(data)) assign(name, data[[name]], envir = result)
    result
}

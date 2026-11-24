assignInMyNamespace <- function(x, value) {
    env <- parent.frame()
    pkg <- packageName(env)
    if (is.null(pkg)) stop("Cannot find the calling namespace.")
    assignInNamespace(x, value, ns = pkg)
}

packageName <- function(env = parent.frame()) {
    if (!is.environment(env)) stop("'env' must be an environment")
    while (!identical(env, emptyenv())) {
        if (identical(env, asNamespace("base"))) return("base")
        if (exists(".packageName", envir = env, inherits = FALSE))
            return(get(".packageName", envir = env, inherits = FALSE))
        env <- parent.env(env)
    }
    NULL
}

isS3stdGeneric <- function(f) {
    if (is.character(f)) {
        if (length(f) != 1L || !exists(f, mode = "function", envir = parent.frame())) return(FALSE)
        f <- get(f, mode = "function", envir = parent.frame())
    }
    if (typeof(f) != "closure") return(FALSE)
    expr <- body(f)
    while (is.call(expr) && identical(expr[[1L]], as.name("{")) && length(expr) > 1L)
        expr <- expr[[2L]]
    if (!is.call(expr) || !identical(expr[[1L]], as.name("UseMethod")) || length(expr) < 2L)
        return(FALSE)
    generic <- eval(expr[[2L]], envir = environment(f))
    if (!is.character(generic) || length(generic) != 1L) return(FALSE)
    setNames(TRUE, generic)
}

.rox_s3_primitive_generics <- c(
    "anyNA", "as.character", "as.complex", "as.double", "as.environment", "as.integer",
    "as.logical", "as.call", "as.numeric", "as.raw", "c", "dim", "dim<-", "dimnames",
    "dimnames<-", "is.array", "is.finite", "is.infinite", "is.matrix", "is.na", "is.nan",
    "is.numeric", "length", "length<-", "levels<-", "names", "names<-", "rep", "seq.int", "xtfrm",
    "abs", "sign", "sqrt", "ceiling", "floor", "trunc", "cummax", "cummin", "cumprod", "cumsum",
    "exp", "expm1", "log", "log10", "log2", "log1p", "cos", "cosh", "sin", "sinh", "tan", "tanh",
    "acos", "acosh", "asin", "asinh", "atan", "atanh", "cospi", "sinpi", "tanpi",
    "gamma", "lgamma", "digamma", "trigamma", "max", "min", "range", "prod", "sum", "any", "all",
    "Arg", "Conj", "Im", "Mod", "Re", "+", "-", "*", "/", "^", "%%", "%/%", "==",
    "!=", "<", "<=", ">=", ">", "&", "|", "!", "[", "[[", "$", "[<-", "[[<-", "$<-")

.rox_uses_s3_dispatch <- function(expr) {
    !missing(expr) && is.call(expr) && (identical(expr[[1L]], as.name("UseMethod")) ||
                     any(vapply(as.list(expr), .rox_uses_s3_dispatch, TRUE)))
}

.rox_s3_generic <- function(name, envir) {
    if (name %in% c("Math", "Ops", "Summary", "Complex", "matrixOps")) return(TRUE)
    if (!exists(name, envir = envir, mode = "function")) return(FALSE)
    fun <- get(name, envir = envir, mode = "function")
    if (is.primitive(fun)) return(name %in% .rox_s3_primitive_generics)
    if (name %in% .rox_s3_primitive_generics &&
        identical(fun, get0(name, envir = baseenv(), inherits = FALSE))) return(TRUE)
    .rox_uses_s3_dispatch(body(fun))
}

isS3method <- function(method, f, class, envir = parent.frame()) {
    if (missing(method)) {
        if (missing(f) || missing(class) || !.rox_s3_generic(f, envir)) return(FALSE)
        return(!is.null(getS3method(f, class, optional = TRUE, envir = envir)))
    }
    if (!is.character(method) || length(method) != 1L || is.na(method)) return(FALSE)
    dots <- gregexpr(".", method, fixed = TRUE)[[1L]]
    for (i in rev(dots[dots > 1L & dots < nchar(method)])) {
        generic <- substr(method, 1L, i - 1L)
        if (!.rox_s3_generic(generic, envir)) next
        if (exists(method, envir = envir, mode = "function")) return(TRUE)
        if (!is.null(getS3method(generic, substring(method, i + 1L), TRUE, envir))) return(TRUE)
    }
    FALSE
}

apropos <- function(what, where = FALSE, ignore.case = TRUE, dot_internals = FALSE, mode = "any",
                    dot_prefix = dot_internals) {
    what <- as.character(what)
    if (length(what) != 1L || is.na(what)) stop("Invalid 'what' argument.")
    result <- character()
    for (i in seq_along(search())) {
        env <- as.environment(i)
        names <- ls(envir = env, all.names = TRUE)
        if (!dot_prefix) names <- names[!startsWith(names, ".__")]
        names <- grep(what, names, value = TRUE, ignore.case = ignore.case)
        if (mode != "any") names <- names[vapply(names, exists, TRUE, envir = env,
                                                inherits = FALSE, mode = mode)]
        if (where && length(names)) names <- setNames(names, rep(as.character(i), length(names)))
        result <- c(result, names)
    }
    sort(result)
}

find <- function(what, mode = "any", numeric = FALSE, simple.words = TRUE) {
    paths <- search()
    hits <- if (simple.words) which(vapply(seq_along(paths), function(i)
        exists(what, envir = as.environment(i), mode = mode, inherits = FALSE), TRUE)) else
        sort(unique(as.integer(names(apropos(what, where = TRUE, ignore.case = FALSE, mode = mode)))))
    if (numeric) setNames(as.integer(hits), paths[hits]) else paths[hits]
}

getAnywhere <- function(x) {
    spelling <- substitute(x)
    text <- tryCatch(if (is.character(x)) x else NULL, error = function(e) NULL)
    x <- if (is.null(text)) as.character(spelling) else text
    if (length(x) != 1L || is.na(x)) stop("Invalid binding name.")
    objs <- list()
    where <- character()
    visible <- logical()
    collect <- function(label, env, shown) {
        if (exists(x, envir = env, inherits = FALSE)) {
            objs <<- c(objs, setNames(list(get(x, envir = env, inherits = FALSE)),
                                     if (shown) label else ""))
            where <<- c(where, label)
            visible <<- c(visible, shown)
        }
    }
    for (label in search()) collect(label, as.environment(label), TRUE)
    for (pkg in loadedNamespaces()) collect(paste0("namespace:", pkg), asNamespace(pkg), FALSE)
    structure(list(name = x, objs = objs, where = where, visible = visible, dups = duplicated(objs)),
              class = "getAnywhere")
}

methods <- function(generic.function, class, all.names = FALSE, dropPath = FALSE) {
    caller <- parent.frame()
    by.class <- missing(generic.function)
    if (by.class && missing(class)) stop("Specify a generic function or class.")
    generic <- if (by.class) NULL else if (is.character(generic.function)) generic.function else
        as.character(substitute(generic.function))
    class <- if (missing(class)) NULL else class
    if (!by.class && !exists(generic, mode = "function", envir = caller))
        stop("no function '", generic, "' is visible")
    rows <- list()
    collect <- function(env, from, visible) {
        candidates <- ls(envir = env, all.names = TRUE)
        if (!is.null(generic)) candidates <- candidates[startsWith(candidates, paste0(generic, "."))]
        if (!is.null(class)) candidates <- candidates[endsWith(candidates, paste0(".", class))]
        for (name in candidates) {
            if (!is.null(rows[[name]]) ||
                !exists(name, mode = "function", envir = env, inherits = FALSE)) next
            method.generic <- if (is.null(generic)) substr(name, 1L, nchar(name) - nchar(class) - 1L) else generic
            if (!.rox_s3_generic(method.generic, caller)) next
            rows[[name]] <<- list(visible = visible, from = from, generic = method.generic)
        }
    }
    for (from in search()) collect(as.environment(from), sub("^package:", "", from), TRUE)
    for (pkg in loadedNamespaces()) {
        ns <- asNamespace(pkg)
        table <- get0(".__S3MethodsTable__.", envir = ns, inherits = FALSE)
        if (is.environment(table)) collect(table, "registered S3method", FALSE)
    }
    names <- sort(as.character(names(rows)))
    rows <- rows[names]
    info <- data.frame(visible = vapply(rows, "[[", TRUE, "visible"),
                       from = vapply(rows, "[[", "", "from"),
                       generic = vapply(rows, "[[", "", "generic"),
                       isS4 = rep(FALSE, length(rows)), row.names = names)
    structure(names, info = info, byclass = by.class, class = "MethodsFunction")
}

print.MethodsFunction <- function(x, ...) {
    shown <- ifelse(attr(x, "info")$visible, as.character(x), paste0(as.character(x), "*"))
    if (length(shown)) print(noquote(shown), ...) else cat("no methods found\n")
    invisible(x)
}

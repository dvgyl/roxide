.ref_has <- function(key, keys) length(keys) > 0L && any(key == keys)

.ref_meta <- function(def) attr(def, ".rx_ref_meta", exact = TRUE)

# Define an environment-backed S4 class and return its generator.
setRefClass <- function(Class, fields = character(), contains = character(),
                        methods = list(), where = topenv(parent.frame()),
                        inheritPackage = FALSE, ...) {
    meta <- .ref_class_meta(Class, fields, contains, methods, where, inheritPackage)
    setClass(Class, contains = meta$contains, where = where, ...)
    .ref_finish_definition(getClass(Class, where = where), meta)
    invisible(.ref_generator(meta))
}

# Get the generator for a reference class visible from where.
getRefClass <- function(Class, where = topenv(parent.frame())) {
    def <- getClass(Class, where = where)
    meta <- .ref_meta(def)
    if (is.null(meta)) stop(sprintf("class “%s” is defined but is not a reference class", Class), call. = FALSE)
    .ref_generator(meta)
}

.ref_generator_methods <- function(meta, entries) {
    if (!length(entries)) return(.ref_method_names(meta))
    if (!is.null(names(entries))) return(invisible(.ref_add_methods(meta, entries)))
    keys <- entries[[1L]]
    result <- list()
    for (key in keys) {
        found <- .ref_find_method(meta, key)
        if (!is.null(found)) result[[length(result) + 1L]] <- .ref_method_definition(found, key)
    }
    if (!length(result)) {
        warning(paste("Methods not found:",
                      paste(sprintf("“%s”", .ref_method_names(meta)), collapse = ", ")))
        return(NULL)
    }
    if (length(keys) == 1L) result[[1L]] else result
}

.ref_method_definition <- function(found, key) {
    def <- found$fun
    attr(def, "mayCall") <- character()
    attr(def, "name") <- key
    attr(def, "refClassName") <- if (is.null(found$owner)) "envRefClass" else found$owner$name
    attr(def, "superClassMethod") <- ""
    .ref_tag(def, structure("refMethodDef", package = "methods"))
}

.ref_accessor_pair <- function(key) {
    names <- list(FIELD = as.name(key))
    getter <- eval(substitute(function() FIELD, names), .ref_namespace)
    setter <- eval(substitute(function(value) {
        FIELD <<- value
        invisible(value)
    }, names), .ref_namespace)
    list(getter, setter)
}

.ref_accessors <- function(meta, fields) {
    if (!length(fields)) stop("replacing substrings in a non-character object", call. = FALSE)
    result <- list()
    for (key in fields) {
        .ref_check_field(meta, key)
        suffix <- paste0(toupper(substr(key, 1L, 1L)), substring(key, 2L))
        pair <- .ref_accessor_pair(key)
        names(pair) <- paste0(c("get", "set"), suffix)
        result <- c(result, pair)
    }
    .ref_add_methods(meta, result)
    invisible(result)
}

.ref_lock <- function(meta, fields) {
    if (!length(fields)) return(meta$locks)
    for (key in fields) {
        .ref_check_field(meta, key)
        spec <- meta$fields[[key]]
        if (is.function(spec))
            stop(sprintf("field ‘%s’ of class “%s” has a non-default binding and cannot be locked",
                         key, meta$name), call. = FALSE)
        if (spec == "ANY") stop("use of NULL environment is defunct", call. = FALSE)
    }
    meta$locks <- .ref_unique(c(meta$locks, fields))
    .ref_invalidate()
    invisible(meta$methods)
}

.ref_construct <- function(...) new(.ref_definition(meta), ...)

.ref_generator_member <- function(x, name) {
    name <- as.character(name)
    meta <- .ref_meta(x)
    if (name == "fields") return(function() .ref_field_types(meta))
    if (name == "methods") return(function(...) .ref_generator_methods(meta, list(...)))
    if (name == "help") return(function(topic) {
        if (missing(topic)) .ref_help(meta) else .ref_help(meta, substitute(topic))
    })
    if (name == "accessors") return(function(fields) .ref_accessors(meta, fields))
    if (name == "lock") return(function(...) .ref_lock(meta, c(...)))
    if (name == "def") return(.ref_definition(meta))
    if (name == "className") return(meta$name)
    stop(sprintf("‘%s’ is not a valid field or method name for reference class “refGeneratorSlot”",
                 name), call. = FALSE)
}

.ref_help <- function(meta, topic) {
    if (missing(topic)) {
        cat("Usage:  $help(topic) where topic is the name of a method (quoted or not)\n")
        cat(sprintf("The definition of class %s follows.\n", meta$name))
        show(.ref_definition(meta))
        return(invisible(NULL))
    }
    topic <- as.character(topic)
    found <- .ref_find_method(meta, topic)
    if (is.null(found)) {
        cat(sprintf("topic ‘%s’ is not a method name in class “%s”\n", topic, meta$name))
        cat("The class definition follows\n")
        stop("unused argument (def)", call. = FALSE)
    }
    .ref_method_help(topic, found$fun)
}

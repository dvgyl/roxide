.ref_has <- function(key, keys) length(keys) > 0L && any(key == keys)

.ref_meta <- function(def) attr(def, ".rx_ref_meta", exact = TRUE)

.ref_named <- function(entries, label) {
    keys <- names(entries)
    if (length(entries) && (is.null(keys) || any(is.na(keys) | !nzchar(keys)) ||
                            length(.ref_unique(keys)) != length(keys))) {
        stop(paste(label, "must have distinct nonempty names"), call. = FALSE)
    }
}

.ref_fields <- function(fields, where) {
    if (!is.list(fields) && !is.character(fields))
        stop("'fields' must be a character vector or list", call. = FALSE)
    if (is.null(names(fields)) && length(fields) && is.character(fields)) {
        keys <- fields
        fields <- rep("ANY", length(keys))
        names(fields) <- keys
    }
    fields <- as.list(fields)
    keys <- names(fields)
    if (length(fields) && is.null(keys)) stop("No field names supplied", call. = FALSE)
    if (length(keys) && length(.ref_unique(keys)) < length(keys)) {
        stop(paste0("All field names must be distinct in:\n(",
                    paste(sprintf("‘%s’", keys), collapse = ", "), ")"), call. = FALSE)
    }
    .ref_named(fields, "fields")
    for (key in names(fields)) {
        spec <- fields[[key]]
        if (typeof(spec) == "closure") next
        if (!is.character(spec) || length(spec) != 1L || is.na(spec)) {
            stop(sprintf(paste0("field ‘%s’ was supplied as an object of class “%s”; ",
                                "must be a class name or a binding function"),
                         key, class(spec)[[1L]]), call. = FALSE)
        }
        if (is.null(getClassDef(spec, where))) {
            stop(sprintf("class “%s” for field ‘%s’ is not defined", spec, key), call. = FALSE)
        }
    }
    fields
}

.ref_add_methods <- function(meta, methods) {
    if (!is.list(methods)) stop("'methods' must be a list", call. = FALSE)
    .ref_named(methods, "methods")
    for (key in names(methods)) {
        def <- methods[[key]]
        if (typeof(def) != "closure") {
            stop(sprintf("method ‘%s’ must be a function", key), call. = FALSE)
        }
    }
    for (key in names(methods)) assign(key, methods[[key]], envir = meta$methods)
    invisible(meta$methods)
}

.ref_prepare_definition <- function(def, meta) {
    fieldClasses <- as.list(.ref_field_types(meta))
    fieldPrototypes <- new.env(parent = emptyenv())
    for (key in names(meta$fields)) assign(key, meta$fields[[key]], envir = fieldPrototypes)
    .ref_store_definition(def, structure("refClassRepresentation", package = "methods"),
        list(fieldClasses = fieldClasses, fieldPrototypes = fieldPrototypes,
             refMethods = meta$methods, refSuperClasses = meta$superclasses, .rx_ref_meta = meta))
}

.ref_class_name <- function(meta) structure(meta$name, package = meta$package)

.ref_definition <- function(meta) {
    def <- getClass(.ref_class_name(meta), where = meta$where)
    if (is.null(.ref_meta(def))) meta$def else def
}

# Define an environment-backed S4 class and return its generator.
setRefClass <- function(Class, fields = character(), contains = character(),
                        methods = list(), where = topenv(parent.frame()),
                        inheritPackage = FALSE, ...) {
    fields <- .ref_fields(fields, where)
    parents <- list()
    for (parent in contains) {
        info <- .ref_meta(getClass(parent, where = where))
        if (!is.null(info)) parents[[length(parents) + 1L]] <- info
    }

    inherited <- list()
    locks <- character()
    superclasses <- character()
    for (parent in parents) {
        for (key in names(parent$fields)) {
            if (!.ref_has(key, names(inherited))) inherited[key] <- parent$fields[key]
        }
        locks <- .ref_unique(c(locks, parent$locks))
        superclasses <- c(superclasses, parent$name, parent$superclasses)
    }
    for (key in names(fields)) inherited[key] <- fields[key]

    meta <- new.env(parent = emptyenv())
    meta$name <- Class
    meta$where <- where
    meta$package <- getPackageName(where)
    meta$parentEnv <- where
    if (inheritPackage) {
        for (parent in parents) {
            if (parent$package != meta$package) {
                meta$parentEnv <- parent$parentEnv
                break
            }
        }
    }
    meta$fields <- inherited
    meta$methods <- new.env(parent = emptyenv())
    meta$parents <- parents
    meta$superclasses <- .ref_unique(c(superclasses, "envRefClass"))
    meta$locks <- locks
    .ref_add_methods(meta, methods)

    if (!length(parents) && !.ref_has("envRefClass", contains)) {
        contains <- c(contains, "envRefClass")
    }
    setClass(Class, contains = contains, where = where, ...)
    def <- getClass(Class, where = where)
    meta$def <- .ref_prepare_definition(def, meta)
    invisible(.ref_generator(meta))
}

# Get the generator for a reference class visible from where.
getRefClass <- function(Class, where = topenv(parent.frame())) {
    def <- getClass(Class, where = where)
    meta <- .ref_meta(def)
    if (is.null(meta)) stop(sprintf("class “%s” is defined but is not a reference class", Class), call. = FALSE)
    .ref_generator(meta)
}

.ref_field_types <- function(meta) {
    keys <- names(meta$fields)
    result <- character(length(keys))
    names(result) <- keys
    for (key in keys) {
        spec <- meta$fields[[key]]
        result[[key]] <- if (is.function(spec)) "activeBindingFunction" else spec
    }
    result
}

.ref_method_names <- function(meta) {
    keys <- ls(meta$methods, all.names = TRUE)
    for (parent in meta$parents) keys <- c(keys, .ref_method_names(parent))
    sort(.ref_unique(c(keys, names(.ref_base_methods))))
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
    invisible(meta$methods)
}

.ref_generator <- function(meta) {
    make <- function(...) new(.ref_definition(meta), ...)
    generator <- .s4_new("refGeneratorSlot", .ref_namespace)
    attr(generator, ".rx_ref_meta") <- meta
    attr(make, "generator") <- generator
    attr(make, "className") <- .ref_class_name(meta)
    attr(make, "package") <- meta$package
    attr(make, ".rx_ref_meta") <- meta
    .ref_tag(make, structure("refObjectGenerator", package = "methods"))
}

.ref_generator_member <- function(x, name) {
    name <- as.character(name)
    meta <- .ref_meta(x)
    if (name == "new") return(function(...) x(...))
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

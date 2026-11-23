.ref_check_field <- function(meta, name) {
    if (!.ref_has(name, names(meta$fields)))
        stop(sprintf("‘%s’ is not a field in class “%s”", name, meta$name), call. = FALSE)
}

.ref_install_field <- function(data, key, spec, locked, meta) {
    force(key)
    force(spec)
    force(locked)
    if (is.function(spec)) {
        environment(spec) <- data
        makeActiveBinding(key, spec, data)
    } else {
        initial <- if (spec == "ANY") {
            new("uninitializedField", field = key, className = spec)
        } else {
            target <- getClass(spec, where = meta$where)
            prototype <- target@prototype
            if (target@virtual) prototype else if (isS4(prototype)) new(target)
            else .s4_new(target, meta$where)
        }
        if (spec == "ANY") {
            assign(key, initial, envir = data)
        } else {
            backing <- paste0(".->", key)
            assign(backing, initial, envir = data)
            binding <- function(value) {
                if (missing(value)) return(get(backing, envir = data, inherits = FALSE))
                if (locked && bindingIsLocked(backing, data)) {
                    stop(sprintf("invalid replacement: reference class field ‘%s’ is read-only",
                                 key), call. = FALSE)
                }
                if (!is(value, spec)) {
                    stop(sprintf(paste0("invalid assignment for reference class field ‘%s’, ",
                                        "should be from class “%s” or a subclass (was class “%s”)"),
                                 key, spec, class(value)[[1L]]), call. = FALSE)
                }
                assign(backing, value, envir = data)
                if (locked) lockBinding(backing, data)
                invisible(value)
            }
            makeActiveBinding(key, binding, data)
        }
    }
}

.ref_instance_meta <- function(object) .ref_meta(object@.xData$.refClassDef)

.ref_find_method <- function(meta, name, own = TRUE, fallback = TRUE) {
    if (own && exists(name, envir = meta$methods, inherits = FALSE)) {
        return(list(owner = meta, fun = get(name, envir = meta$methods, inherits = FALSE)))
    }
    for (parent in meta$parents) {
        found <- .ref_find_method(parent, name, fallback = FALSE)
        if (!is.null(found)) return(found)
    }
    if (fallback && .ref_has(name, names(.ref_base_methods))) {
        return(list(owner = NULL, fun = .ref_base_methods[[name]]))
    }
    NULL
}

.ref_bind_method <- function(object, name, found) {
    force(object)
    force(name)
    force(found)
    if (is.null(found$owner)) {
        scope <- new.env(parent = .ref_namespace)
        scope$.self <- object
        scope$.refClassDef <- object@.xData$.refClassDef
    } else {
        scope <- new.env(parent = object@.xData)
    }
    scope$callSuper <- function(...) {
        owner <- found$owner
        if (is.null(owner)) return(.ref_base_methods$callSuper())
        nextMethod <- .ref_find_method(owner, name, own = FALSE)
        if (is.null(nextMethod) && name == "initialize")
            return(.ref_initialize_fields(object, list(...)))
        if (is.null(nextMethod)) {
            stop(sprintf(paste0("a call to superClass() is in the method ‘%s’ but there is ",
                                "no superclass definition of this method for class “%s”"),
                         name, owner$name), call. = FALSE)
        }
        .ref_bind_method(object, name, nextMethod)(...)
    }
    fun <- found$fun
    environment(fun) <- scope
    fun
}

.ref_method <- function(object, name, meta) {
    found <- .ref_find_method(meta, name)
    if (!is.null(found)) return(.ref_bind_method(object, name, found))
    stop(sprintf("‘%s’ is not a valid field or method name for reference class “%s”",
                 name, meta$name), call. = FALSE)
}

.ref_member <- function(x, name) {
    name <- as.character(name)
    if (identical(class(x)[[1L]], "refGeneratorSlot"))
        return(.ref_generator_member(x, name))
    data <- x@.xData
    if (exists(name, envir = data, inherits = FALSE)) {
        return(get(name, envir = data, inherits = FALSE))
    }
    method <- .ref_method(x, name, .ref_instance_meta(x))
    assign(name, method, envir = data)
    method
}

.ref_replace <- function(x, name, value) {
    name <- as.character(name)
    .ref_check_field(.ref_instance_meta(x), name)
    assign(name, value, envir = x@.xData)
    x
}

.ref_method_binding <- function() .ref_member(object, key)

.ref_seed <- function(object, meta) {
    data <- object@.xData
    methods <- new.env(parent = meta$parentEnv)
    parent.env(data) <- methods
    data$.self <- object
    data$.refClassDef <- .ref_definition(meta)
    keys <- names(meta$fields)
    for (key in keys)
        .ref_install_field(data, key, meta$fields[[key]], .ref_has(key, meta$locks), meta)
    .ref_install_methods(methods, .ref_method_names(meta), .ref_method_binding, object)
    lockBinding(".self", data)
    lockBinding(".refClassDef", data)
    invisible(object)
}

.ref_initialize_fields <- function(object, args) {
    meta <- .ref_instance_meta(object)
    keys <- names(args)
    if (is.null(keys)) keys <- rep("", length(args))
    for (i in seq_along(args)) {
        if (nzchar(keys[[i]])) next
        value <- args[[i]]
        if (is(value, "envRefClass")) {
            other <- .ref_instance_meta(value)
            for (key in names(other$fields)) {
                if (.ref_has(key, names(meta$fields)))
                    assign(key, value$field(key), envir = object@.xData)
            }
        } else {
            warning(sprintf(paste0("unnamed arguments to $new() must be objects from a ",
                                   "reference class; got an object of class “%s”"),
                            class(value)[[1L]]))
        }
    }
    for (i in seq_along(args)) {
        key <- keys[[i]]
        if (!nzchar(key)) next
        .ref_check_field(meta, key)
        assign(key, args[[i]], envir = object@.xData)
    }
    object
}

.ref_copy <- function(object, shallow) {
    meta <- .ref_instance_meta(object)
    result <- new(.ref_definition(meta))
    for (key in names(meta$fields)) {
        value <- object$field(key)
        if (!shallow && is(value, "envRefClass")) value <- value$copy(FALSE)
        assign(key, value, envir = result@.xData)
    }
    result
}

.ref_show <- function(object) {
    meta <- .ref_instance_meta(object)
    cat(sprintf('Reference class object of class "%s"\n', meta$name))
    for (key in names(meta$fields)) {
        cat(sprintf('Field "%s":\n', key))
        print(get(key, envir = object@.xData, inherits = FALSE))
    }
    invisible(NULL)
}

.ref_export <- function(object, Class) {
    if (identical(class(object)[[1L]], Class[[1L]])) return(object)
    if (!is(object, Class)) return(as(object, Class))
    target <- getRefClass(Class)
    meta <- .ref_meta(target)
    result <- target$new()
    for (key in names(meta$fields)) {
        value <- object$field(key)
        required <- .ref_field_types(meta)[[key]]
        if (!is(value, required))
            stop(sprintf(paste0("the class of field ‘%s’ in the object is not compatible ",
                                "with the desired class “%s” in the target"), key, required),
                 call. = FALSE)
        assign(key, value, envir = result@.xData)
    }
    result
}

.ref_import <- function(object, value, Class) {
    target <- .ref_instance_meta(object)
    if (!.ref_has(Class[[1L]], c(target$name, target$superclasses))) {
        stop(sprintf("“%s” is not one of the reference super classes for this object",
                     Class[[1L]]), call. = FALSE)
    }
    if (!is(value, Class)) value <- as(value, Class)
    source <- .ref_meta(getClass(Class))
    for (key in names(source$fields)) {
        if (.ref_has(key, names(target$fields)))
            assign(key, value$field(key), envir = object@.xData)
    }
    invisible(object)
}

.ref_base_methods <- list(
    ".objectPackage" = function() .ref_instance_meta(.self)$package,
    ".objectParent" = function() .ref_instance_meta(.self)$parentEnv,
    initFields = function(...) .ref_initialize_fields(.self, list(...)),
    callSuper = function(...) {
        stop("direct calls to callSuper() are invalid:  should only be called from another method",
             call. = FALSE)
    },
    usingMethods = function(...) {
        meta <- .ref_instance_meta(.self)
        for (key in c(...)) {
            if (!is.null(.ref_find_method(meta, key))) .ref_member(.self, key)
        }
        NULL
    },
    copy = function(shallow = FALSE) .ref_copy(.self, shallow),
    field = function(name, value) {
        if (missing(value)) return(get(name, envir = .self@.xData))
        if (!.ref_has(name, names(.ref_instance_meta(.self)$fields)))
            stop(sprintf("‘%s’ is not a field in this class", name), call. = FALSE)
        invisible(assign(name, value, envir = .self@.xData))
    },
    getClass = function() .refClassDef,
    getRefClass = function() .ref_generator(.ref_instance_meta(.self)),
    show = function() .ref_show(.self),
    export = function(Class) .ref_export(.self, Class),
    import = function(value, Class = class(value)) .ref_import(.self, value, Class),
    trace = function(...) stop("reference method tracing is unavailable"),
    untrace = function(...) stop("reference method tracing is unavailable")
)

.ref_check_field <- function(meta, name) {
    if (!.ref_has(name, names(meta$fields)))
        stop(sprintf("‘%s’ is not a field in class “%s”", name, meta$name), call. = FALSE)
}

.ref_instance_meta <- function(object) attr(object@.xData$.refClassDef, ".rx_ref_meta", exact = TRUE)

.ref_find_method <- function(meta, name) .ref_lookup_method(meta, name)

.ref_super_binding <- function(...) {
    nextMethod <- .ref_super(.self, name, owner)
    if (is.null(nextMethod)) return(.ref_initialize_fields(.self, list(...)))
    nextMethod(...)
}

.ref_method_binding <- function() .ref_member(object, key)
.ref_field_binding <- function(...) .ref_field_value(...)

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
    getClass = function(...) if (...length()) methods::getClass(...) else .refClassDef,
    getRefClass = function(...) if (...length()) methods::getRefClass(...) else .ref_generator(.ref_instance_meta(.self)),
    show = function() .ref_show(.self),
    export = function(Class) .ref_export(.self, Class),
    import = function(value, Class = class(value)) .ref_import(.self, value, Class),
    trace = function(...) stop("reference method tracing is unavailable"),
    untrace = function(...) stop("reference method tracing is unavailable")
)

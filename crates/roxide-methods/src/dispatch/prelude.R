signature <- function(...) {
    entries <- list(...)
    result <- character(length(entries))
    for (i in seq_along(entries)) {
        value <- entries[[i]]
        if (!is.character(value) || length(value) != 1L) {
            stop(sprintf("bad class specified for element %d (should be a single character string)", i),
                 call. = FALSE)
        }
        result[[i]] <- value
    }
    names(result) <- names(entries)
    result
}

setGeneric <- function(name, def = NULL, group = list(), valueClass = character(),
                       where = topenv(parent.frame()), package = NULL,
                       signature = NULL, useAsDefault = NULL,
                       genericFunction = NULL, simpleInheritanceOnly = NULL) {
    .rx_setGeneric(name, def, where, package, signature, useAsDefault,
                   genericFunction, group, valueClass, simpleInheritanceOnly)
}

getGeneric <- function(f, mustFind = FALSE, where = topenv(parent.frame()), package = "") {
    .rx_generic(f, where, package, mustFind)
}

isGeneric <- function(f, where = topenv(parent.frame()), fdef = NULL, getName = FALSE) {
    .rx_isGeneric(f, where, fdef, getName)
}

removeGeneric <- function(f, where = topenv(parent.frame())) .rx_removeGeneric(f, where)

setMethod <- function(f, signature = character(), definition,
                      where = topenv(parent.frame()), valueClass = NULL, sealed = FALSE) {
    .rx_setMethod(f, signature, definition, where, valueClass, sealed)
}

removeMethod <- function(f, signature = character(), where = topenv(parent.frame())) {
    .rx_removeMethod(f, signature, where)
}

selectMethod <- function(f, signature, optional = FALSE, useInherited = TRUE,
                         mlist = NULL, fdef = NULL, verbose = FALSE, doCache = FALSE) {
    .rx_selectMethod(f, signature, topenv(parent.frame()), optional, useInherited, fdef, doCache, mlist)
}

getMethod <- function(f, signature = character(), where = topenv(parent.frame()),
                      optional = FALSE, ...) {
    .rx_selectMethod(f, signature, where, optional, FALSE, NULL, FALSE)
}

existsMethod <- function(f, signature = character(), where = topenv(parent.frame())) {
    !is.null(getMethod(f, signature, where, optional = TRUE))
}

hasMethod <- function(f, signature = character(), where = topenv(parent.frame())) {
    !is.null(.rx_selectMethod(f, signature, where, TRUE, TRUE, NULL, FALSE))
}

findMethods <- function(f, where, classes = NULL,
                        inherited = FALSE, package = "", ...) {
    .rx_findMethods(f, if (missing(where)) NULL else .rx_metaEnvironment(where),
                    classes, inherited, package, topenv(parent.frame()))
}

showMethods <- function(f = character(), where = topenv(parent.frame()), classes = NULL,
                        includeDefs = FALSE, inherited = !includeDefs, showEmpty = TRUE,
                        printTo = stdout(), fdef = NULL) {
    .rx_showMethods(f, where, classes, includeDefs, inherited, showEmpty, printTo, fdef)
}

getGroup <- function(fdef, recursive = FALSE) .rx_getGroup(fdef, recursive)

getGroupMembers <- function(group, recursive = FALSE, character = FALSE) {
    .rx_getGroupMembers(group, recursive, missing(character) || character)
}

isGroup <- function(f, where = topenv(parent.frame()), fdef) {
    .rx_isGroup(if (missing(fdef)) f else fdef, .rx_metaEnvironment(where))
}

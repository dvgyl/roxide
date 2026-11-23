cacheMetaData <- function(where, attach = TRUE, searchWhere = .rx_metaEnvironment(where),
                          doCheck = TRUE) {
    invisible(.rx_cacheMetaData(.rx_metaEnvironment(where), attach, .rx_metaEnvironment(searchWhere)))
}

cacheGenericsMetaData <- function(f, fdef = getGeneric(f, where = where), attach = TRUE,
                                  where = topenv(parent.frame()),
                                  package = packageSlot(fdef), methods) {
    .rx_cacheGeneric(.rx_metaEnvironment(where), f, fdef, attach, package,
                     if (missing(methods)) NULL else methods)
}

getClasses <- function(where = parent.frame(), inherits = missing(where)) {
    .rx_getClasses(.rx_metaEnvironment(where), inherits)
}

getGenerics <- function(where, searchForm = FALSE) {
    .rx_getGenerics(if (missing(where)) NULL else .rx_metaEnvironment(where), searchForm)
}

findMethodSignatures <- function(..., target = TRUE, methods = findMethods(...)) {
    .rx_findMethodSignatures(methods, target)
}

S3Part <- function(object, strictS3 = FALSE, S3Class) {
    .rx_S3Part(object, strictS3, if (missing(S3Class)) NULL else S3Class)
}

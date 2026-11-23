representation <- function(...) list(...)

prototype <- function(...) .s4_prototype(list(...))

getPackageName <- function(where = topenv(parent.frame())) .s4_package(where)

setClass <- function(Class, representation = list(), prototype = NULL,
                     contains = character(), validity = NULL, access = list(),
                     where = topenv(parent.frame()), version = NULL, sealed = FALSE,
                     package = getPackageName(where), slots = list()) {
    definition <- .s4_define(Class, representation, prototype, contains, validity,
                             access, where, sealed, package, slots)
    generator <- function(...) new(Class, ...)
    invisible(.s4_generator(generator, definition))
}

getClass <- function(Class, .Force = FALSE, where = topenv(parent.frame())) {
    .s4_get(Class, where, NULL, TRUE, if (.Force) 2L else 1L)
}

getClassDef <- function(Class, where = topenv(parent.frame()), package = NULL,
                        inherits = TRUE) .s4_get(Class, where, package, inherits, 0L)

isClass <- function(Class, formal = TRUE, where = topenv(parent.frame())) {
    !is.null(getClassDef(Class, where))
}

removeClass <- function(Class, where = topenv(parent.frame())) .s4_remove(Class, where)

isVirtualClass <- function(Class, where = topenv(parent.frame())) {
    definition <- getClassDef(Class, where)
    is.null(definition) || definition@virtual
}

getSlots <- function(x) .s4_slots(x, topenv(parent.frame()), FALSE)
slotNames <- function(x) .s4_slots(x, topenv(parent.frame()), TRUE)

slot <- function(object, name, check = TRUE) .s4_slot(object, name)
`slot<-` <- function(object, name, check = TRUE, value) {
    .s4_slot_set(object, name, check, value)
}

new <- function(Class, ...) {
    where <- parent.frame()
    object <- .s4_new(Class, topenv(where))
    entry <- .s4_initializer(where)
    entry(object, ...)
}

initialize <- function(.Object, ...) .s4_initialize(.Object, list(...), parent.frame())

setValidity <- function(Class, method, where = topenv(parent.frame())) {
    invisible(.s4_validity_set(Class, method, where))
}

getValidity <- function(ClassDef) ClassDef@validity

validObject <- function(object, test = FALSE, complete = FALSE) {
    .s4_valid(object, test, complete, parent.frame())
}

setClassUnion <- function(name, members = character(), where = topenv(parent.frame())) {
    invisible(.s4_union(name, members, where))
}

setOldClass <- function(Classes, prototype = NULL, where = topenv(parent.frame()),
                        test = FALSE, S4Class) {
    invisible(.s4_old(Classes, prototype, where))
}

setIs <- function(class1, class2, test = NULL, coerce = NULL, replace = NULL,
                  where = topenv(parent.frame()), classDef = getClassDef(class1, where),
                  extensionObject = NULL, doComplete = TRUE) {
    invisible(.s4_set_is(class1, class2, test, coerce, replace, where))
}

is <- function(object, class2) .s4_is(object, if (missing(class2)) NULL else class2, parent.frame())

extends <- function(class1, class2, fullInfo = FALSE, maybe = TRUE) {
    .s4_extends(class1, if (missing(class2)) NULL else class2, fullInfo, maybe,
                topenv(parent.frame()))
}

as <- function(object, Class, strict = TRUE, ext = NULL) {
    .s4_as(object, Class, strict, ext, parent.frame())
}

`as<-` <- function(object, Class, value) .s4_as_set(object, Class, value, parent.frame())

coerce <- function(from, to, strict = TRUE) .s4_as(from, to, strict, NULL, parent.frame())

setAs <- function(from, to, def, replace = NULL, where = topenv(parent.frame())) {
    invisible(.s4_set_as(from, to, def, replace, where))
}

.s4_true <- function(object) TRUE
.s4_edge_coerce <- function(from, strict = TRUE) as(from, .s4_target, strict)
.s4_edge_replace <- function(from, to, value) `as<-`(from, .s4_target, value)

.s4_bootstrap(environment())

.ref_namespace <- environment()

setClass("refClass", contains = c("refObject", "VIRTUAL"), where = .ref_namespace)
setClass(".environment", contains = "environment", where = .ref_namespace)
setClass("envRefClass", contains = c(".environment", "refClass"), where = .ref_namespace)
setClass("refGeneratorSlot", contains = "envRefClass", where = .ref_namespace)
setClass("refObjectGenerator", contains = c("classGeneratorFunction", "refClass"),
         slots = c(generator = "refGeneratorSlot"), where = .ref_namespace)
setClass("refClassRepresentation", contains = "classRepresentation",
         slots = c(fieldClasses = "list", fieldPrototypes = "environment",
                   refMethods = "environment", refSuperClasses = "character"),
         where = .ref_namespace)
setClass("refMethodDef", contains = "function",
         slots = c(mayCall = "character", name = "character", refClassName = "character",
                   superClassMethod = "character"), where = .ref_namespace)
setClass("uninitializedField", slots = c(field = "character", className = "character"),
         where = .ref_namespace)

.ref_initialize_s4 <- initialize
# Install the fields before the reference class initializer runs.
initialize <- function(.Object, ...) {
    meta <- .ref_object_meta(.Object)
    if (!is.null(meta)) {
        init <- .ref_seed(.Object, meta)
        if (is.null(init)) {
            .ref_initialize_fields(.Object, list(...))
        } else {
            init(...)
        }
        return(.Object)
    }
    .ref_initialize_s4(.Object, ...)
}

.ref_register_access(.ref_member, .ref_replace, .ref_generator_access)
setMethod("show", "envRefClass", function(object) object$show(), where = .ref_namespace)

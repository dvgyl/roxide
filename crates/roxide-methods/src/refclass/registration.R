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
    if (isS4(.Object) && is(.Object, "envRefClass")) {
        def <- getClass(class(.Object), where = topenv(parent.frame()))
        meta <- .ref_meta(def)
        if (!is.null(meta)) {
            .ref_seed(.Object, meta)
            init <- .ref_find_method(meta, "initialize")
            if (is.null(init)) {
                .Object$initFields(...)
            } else {
                .ref_bind_method(.Object, "initialize", init)(...)
            }
            return(.Object)
        }
    }
    .ref_initialize_s4(.Object, ...)
}

.ref_register_access(.ref_member, .ref_replace, .ref_generator_member)
setMethod("show", "envRefClass", function(object) object$show(), where = .ref_namespace)

dyn.load <- function(x, local = TRUE, now = TRUE, ...)
    .Internal(rox_dyn_load(path.expand(x), local, now))

dyn.unload <- function(x)
    .Internal(rox_dyn_unload(path.expand(x)))

is.loaded <- function(symbol, PACKAGE = "", type = "")
    .Internal(rox_is_loaded(symbol, PACKAGE, type))

getNativeSymbolInfo <- function(name, PACKAGE = "", unlist = TRUE, withRegistrationInfo = FALSE) {
    if (is.list(name)) {
        name <- vapply(name, function(x) {
            value <- as.character(x)
            if (length(value) != 1L) stop("invalid 'name' argument")
            value
        }, "", USE.NAMES = FALSE)
    }
    if (inherits(PACKAGE, "DLLInfoReference")) {
        for (dll in getLoadedDLLs()) {
            if (identical(dll[["info"]], PACKAGE)) {
                PACKAGE <- dll
                break
            }
        }
        if (inherits(PACKAGE, "DLLInfoReference")) stop("DLLInfoReference is not loaded")
    }
    if (inherits(PACKAGE, "DLLInfo")) PACKAGE <- PACKAGE[["name"]]
    .Internal(rox_native_symbol_info(name, PACKAGE, unlist, withRegistrationInfo))
}

getLoadedDLLs <- function()
    .Internal(rox_loaded_dlls())

getDLLRegisteredRoutines <- function(dll, addNames = TRUE)
    UseMethod("getDLLRegisteredRoutines")

getDLLRegisteredRoutines.DLLInfo <- function(dll, addNames = TRUE)
    .Internal(rox_registered_routines(dll, addNames))

getDLLRegisteredRoutines.character <- function(dll, addNames = TRUE) {
    if (length(dll) != 1L || is.na(dll)) stop("'dll' must be a single DLL name or path")
    loaded <- getLoadedDLLs()
    info <- unclass(loaded)[[dll]]
    if (is.null(info)) {
        for (entry in loaded) {
            if (entry[["path"]] == dll) {
                info <- entry
                break
            }
        }
    }
    if (is.null(info)) stop(sprintf("No DLL currently loaded with name or path '%s'", dll))
    getDLLRegisteredRoutines.DLLInfo(info, addNames)
}

.dynLibs <- (function() {
    dlls <- structure(list(), class = "DLLInfoList")
    function(new) {
        if (missing(new)) return(dlls)
        dlls <<- structure(new, class = "DLLInfoList")
        invisible(dlls)
    }
})()

library.dynam <- function(chname, package, lib.loc, verbose = getOption("verbose"),
                          file.ext = .Platform$dynlib.ext, ...) {
    if (missing(chname)) return(.dynLibs())
    path <- normalizePath(file.path(find.package(package, lib.loc), "libs", paste0(chname, file.ext)),
                          mustWork = FALSE)
    dlls <- .dynLibs()
    for (dll in dlls) {
        if (dll[["path"]] == path) return(invisible(dll))
    }
    if (isTRUE(verbose)) cat("Loading DLL ", path, "\n", sep = "")
    dll <- dyn.load(path, ...)
    .dynLibs(c(unclass(dlls), list(dll)))
    invisible(dll)
}

library.dynam.unload <- function(chname, libpath, verbose = getOption("verbose"),
                                 file.ext = .Platform$dynlib.ext) {
    path <- normalizePath(file.path(libpath, "libs", paste0(chname, file.ext)), mustWork = FALSE)
    dlls <- .dynLibs()
    paths <- vapply(unclass(dlls), function(dll) dll[["path"]], "")
    index <- match(path, paths)
    if (isTRUE(verbose)) cat("Unloading DLL ", path, "\n", sep = "")
    dyn.unload(path)
    if (is.na(index)) return(invisible(NULL))
    .dynLibs(unclass(dlls)[-index])
    invisible(dlls[[index]])
}

`[[.DLLInfo` <- function(x, i, ...) unclass(x)[[i, exact = TRUE]]

`$.DLLInfo` <- function(x, name) {
    field <- x[[name]]
    if (!is.null(field)) return(field)
    getNativeSymbolInfo(name, x)
}

`$.DLLInfoList` <- function(x, name) unclass(x)[[name, exact = FALSE]]

print.DLLInfo <- function(x, ...) {
    cat("DLL name: ", x[["name"]], "\n", sep = "")
    path <- x[["path"]]
    if (nchar(path) + 10L > getOption("width", 80L))
        cat("Filename:\n      ", path, "\n", sep = "")
    else
        cat("Filename: ", path, "\n", sep = "")
    cat("Dynamic lookup: ", x[["dynamicLookup"]], "\n", sep = "")
    invisible(x)
}

print.DLLInfoList <- function(x, ...) {
    if (length(x)) {
        dlls <- unclass(x)
        print(data.frame(Filename = vapply(dlls, function(dll) dll[["path"]], ""),
                         Dynamic.Lookup = vapply(dlls, function(dll) dll[["dynamicLookup"]], TRUE),
                         row.names = names(dlls)), ...)
    }
    invisible(x)
}

print.NativeSymbolInfo <- function(x, ...) {
    print(unclass(x), ...)
    cat("attr(,\"class\")\n")
    print(class(x), ...)
    invisible(x)
}

print.DLLRegisteredRoutines <- function(x, ...) {
    groups <- unclass(x)
    rows <- max(c(0L, lengths(groups)))
    columns <- list()
    for (kind in names(groups)) {
        routines <- groups[[kind]]
        n <- length(routines)
        if (n) {
            columns[[kind]] <- c(vapply(unclass(routines), function(s) s$name, ""), rep("", rows - n))
            columns[[paste0(kind, ".numParameters")]] <-
                c(vapply(unclass(routines), function(s) as.character(s$numParameters), ""), rep("", rows - n))
        }
    }
    if (rows) print(do.call(data.frame, c(columns, list(check.names = FALSE))), ...)
    else cat("data frame with 0 columns and 0 rows\n")
    invisible(x)
}

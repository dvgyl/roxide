args <- commandArgs(trailingOnly = TRUE)
stopifnot(identical(commandArgs()[[1L]], args[[3L]]))
path <- args[[1L]]

loaded <- withVisible(dyn.load(path))
dll <- loaded$value
stopifnot(!loaded$visible, inherits(dll, "DLLInfo"), identical(dll$path, path),
          identical(getLoadedDLLs()$native, dll), !dll$dynamicLookup)
stopifnot(is.loaded("increment", "native", "Call"),
          is.loaded("increment_buffer", "native", "C"),
          is.loaded("increment_fortran", "native", "Fortran"),
          is.loaded("tagged_argument", "native", "External"),
          !is.loaded("increment", "native", "C"))

routines <- getDLLRegisteredRoutines(dll)
stopifnot(identical(routines, getDLLRegisteredRoutines("native")),
          identical(names(routines$.Call), "increment"),
          is.null(names(getDLLRegisteredRoutines(dll, FALSE)$.Call)))
info <- getNativeSymbolInfo(c("increment", "tagged_argument"), dll, withRegistrationInfo = TRUE)
stopifnot(identical(names(info), c("increment", "tagged_argument")),
          inherits(info$increment, "NativeSymbolInfo"))

stopifnot(identical(.Call("increment", 41L, PACKAGE = "native"), 42L),
          identical(.Call(info$increment, 41L), 42L),
          identical(.Call(dll$increment$address, 41L), 42L),
          identical(.External("tagged_argument", argument = 42L, PACKAGE = "native"), 42L),
          identical(.External2("external_context", argument = 42L, PACKAGE = "native"), 42L),
          identical(.C("increment_buffer", value = 41L, PACKAGE = "native"), list(value = 42L)),
          identical(.Fortran("increment_fortran", value = 41L, PACKAGE = "native"), list(value = 42L)))

unloaded <- withVisible(dyn.unload(path))
stopifnot(!unloaded$visible, is.null(unloaded$value), !is.loaded("increment", "native"),
          is.null(getLoadedDLLs()$native))

message <- tryCatch(dyn.load(args[[2L]]), error = conditionMessage)
stopifnot(grepl("foreign R dependency '@rpath/libR.dylib'", message, fixed = TRUE))
cat("Native calls and foreign dependency check passed.\n")

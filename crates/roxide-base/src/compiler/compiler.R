.options <- new.env(parent = emptyenv())
.options$values <- list(
    optimize = 2L,
    suppressAll = TRUE,
    suppressUndefined = c(".Generic", ".Method", ".Random.seed", ".self"),
    suppressNoSuperAssignVar = FALSE
)

.checkOptions <- function(optimize, level) {
    if (optimize < 2L && level > 2L)
        stop(paste0("invalid compiler options: optimize(==", optimize,
                    ")<2 and jitEnabled(==", level, ")>2"))
}

enableJIT <- function(level) {
    level <- as.integer(level)
    if (identical(level, 3L)) .checkOptions(.options$values$optimize, level)
    .Internal(rox_compiler_jit(level))
}

compilePKGS <- function(enable) .Internal(rox_compiler_pkgs(as.logical(enable)))

getCompilerOption <- function(name, options = NULL) {
    if (length(name) != 1L) stop("expected one compiler option name")
    if (name %in% names(options)) return(options[[name]])
    values <- .options$values
    if (!(name %in% names(values))) stop(paste0("object '", name, "' not found"))
    values[[name]]
}

setCompilerOptions <- function(...) {
    changes <- list(...)
    keys <- names(changes)
    values <- .options$values
    previous <- list()
    for (name in keys) {
        if (!(name %in% names(values)))
            stop(paste0("'", name, "' is not a valid compiler option"))
        value <- changes[[name]]
        if (name == "optimize") {
            value <- as.integer(value)
            if (length(value) != 1L || value < 0L || value > 3L) next
            .checkOptions(value, enableJIT(-1L))
        } else if (!(is.logical(value) && length(value) == 1L && !is.na(value)) &&
                   !(name == "suppressUndefined" && is.character(value))) {
            next
        }
        previous <- c(previous, .options$values[name])
        values[[name]] <- value
    }
    .options$values <- values
    invisible(previous)
}

cmpfun <- function(f, options = NULL) {
    force(options)
    .Internal(rox_compiler_cmpfun(f))
}

compile <- function(e, env = .GlobalEnv, options = NULL) {
    force(options)
    .Internal(rox_compiler_compile(e, env))
}

cmpfile <- function(infile, outfile, ascii = FALSE, env = .GlobalEnv,
                    verbose = FALSE, options = NULL) {
    if (!is.environment(env)) stop("argument is not an environment")
    if (missing(outfile)) {
        outfile <- if (grepl("[.][Rr]$", infile)) sub("[.][Rr]$", ".Rc", infile)
                   else paste0(infile, ".Rc")
    }
    exprs <- parse(file = infile, keep.source = FALSE)
    if (length(exprs) == 0L) {
        warning("empty input file; no output written")
        return(invisible(NULL))
    }
    force(options)
    invisible(.Internal(rox_compiler_write(exprs, outfile, ascii, verbose)))
}

loadcmp <- function(file, envir = .GlobalEnv, chdir = FALSE) {
    if (chdir) {
        directory <- dirname(file)
        file <- basename(file)
        old <- setwd(directory)
        on.exit(setwd(old))
    }
    invisible(.Internal(rox_compiler_load(file, envir)))
}

disassemble <- function(code) {
    text <- .Internal(rox_compiler_disassemble(code))
    cat(text)
    invisible(text)
}

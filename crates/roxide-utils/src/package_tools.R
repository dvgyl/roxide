install.packages <- function(pkgs, lib, repos = getOption("repos"), contriburl = contrib.url(repos, type),
                             method, available = NULL, destdir = NULL, dependencies = NA,
                             type = getOption("pkgType"), configure.args = getOption("configure.args"),
                             configure.vars = getOption("configure.vars"), clean = FALSE,
                             Ncpus = getOption("Ncpus", 1L), verbose = getOption("verbose", FALSE),
                             libs_only = FALSE, INSTALL_opts, quiet = FALSE, keep_outputs = FALSE, ...) {
    if (!is.null(repos)) stop("Repository installation is unavailable in offline mode. Set repos = NULL for local packages.")
    if (missing(pkgs) || !length(pkgs)) return(invisible(NULL))
    if (missing(lib)) lib <- .libPaths()[1L]
    if (length(lib) == 1L) lib <- rep(lib, length(pkgs))
    if (length(lib) != length(pkgs)) stop("'lib' must have length one or match 'pkgs'")
    executable <- commandArgs()[[1L]]
    opts <- if (missing(INSTALL_opts)) character() else INSTALL_opts
    if (clean) opts <- c(opts, "--clean")
    if (libs_only) opts <- c(opts, "--libs-only")
    for (i in seq_along(pkgs)) {
        pkg <- normalizePath(pkgs[[i]], mustWork = TRUE)
        dest <- normalizePath(lib[[i]], mustWork = TRUE)
        status <- system2(executable, shQuote(c("CMD", "INSTALL", "-l", dest, opts, pkg)),
                          stdout = if (quiet) FALSE else "", stderr = if (quiet) FALSE else "", timeout = 120)
        if (length(status) && status != 0L)
            warning("Installation failed for '", pkgs[[i]], "' with status ", status)
    }
    invisible(NULL)
}

package.skeleton <- function(name = "anRpackage", list = character(), environment = .GlobalEnv,
                             path = ".", force = FALSE, code_files = character(), encoding = "unknown") {
    if (length(name) != 1L || is.na(name) ||
        !grepl("^[A-Za-z][A-Za-z0-9.]+$", name) || endsWith(name, "."))
        stop("Malformed package name.")
    if (missing(list) && !length(code_files)) list <- ls(envir = environment, all.names = TRUE)
    if (!is.character(list)) stop("'list' must be a character vector")
    list <- unique(list)
    absent <- list[!vapply(list, exists, TRUE, envir = environment, inherits = FALSE)]
    if (length(absent)) stop("Objects not found: ", paste(absent, collapse = ", "))
    if (!length(list) && !length(code_files)) stop("No objects or code files to include.")
    if (any(!file.exists(code_files) | dir.exists(code_files))) stop("Cannot read a code file.")
    code_names <- basename(code_files)
    if (anyDuplicated(code_names) || (length(list) && "objects.R" %in% code_names))
        stop("Code file names must be unique.")
    root <- file.path(path, name)
    if (file.exists(root) && !force) stop("Package directory already exists.")
    for (dir in c(root, file.path(root, "R"), file.path(root, "man")))
        if (!dir.exists(dir) && !dir.create(dir)) stop("Cannot create '", dir, "'.")
    description <- c(paste("Package:", name), "Type: Package", paste("Title:", name),
                     "Version: 0.1.0", "Author: Add your name",
                     "Maintainer: Add your name <maintainer@example.org>",
                     "Description: Describe the package.", "License: Specify the license")
    if (encoding != "unknown") description <- c(description, paste("Encoding:", encoding))
    writeLines(description, file.path(root, "DESCRIPTION"))
    exports <- if (length(list)) paste0("export(", paste(encodeString(list, quote = '"'),
                                                           collapse = ", "), ")") else character()
    if (length(code_files)) exports <- c(exports, 'exportPattern("^[[:alpha:]]")')
    writeLines(exports, file.path(root, "NAMESPACE"))
    if (length(list)) invisible(dump(list, file.path(root, "R", "objects.R"), envir = environment))
    if (length(code_files) && !all(file.copy(code_files, file.path(root, "R"), overwrite = force)))
        stop("Cannot copy the code files.")
    writeLines(c("Complete DESCRIPTION and add documentation in man/.",
                 "Delete this file before distributing the package."),
               file.path(root, "Read-and-delete-me"))
    invisible(root)
}

globalVariables <- function(names, package, add = TRUE) {
    env <- if (missing(package)) topenv(parent.frame()) else
        if (is.environment(package)) package else asNamespace(package)
    old <- if (exists(".__global__", envir = env, inherits = FALSE))
        get(".__global__", envir = env, inherits = FALSE) else character()
    if (missing(names)) return(invisible(old))
    if (!is.character(names)) stop("the global variables list must be a character vector")
    if (environmentIsLocked(env)) stop("The namespace for package \"", getNamespaceName(env),
        "\" is locked; no changes in the global variables list may be made.")
    result <- if (add) unique(c(old, names)) else names
    assign(".__global__", result, envir = env)
    invisible(result)
}
hasName <- function(x, name) name %in% names(x)

packageVersion <- function(pkg, lib.loc = NULL) {
    if (exists("packageVersion", envir = baseenv(), inherits = FALSE))
        return(base::packageVersion(pkg, lib.loc))
    version <- packageDescription(pkg, lib.loc, fields = "Version")
    if (is.na(version)) stop("there is no package called ‘", pkg, "’")
    structure(list(as.integer(strsplit(version, "[.-]")[[1L]])),
              class = c("package_version", "numeric_version"))
}
compareVersion <- function(a, b) {
    if (is.na(a)) return(if (is.na(b)) 0L else -1L)
    if (is.na(b)) return(1L)
    x <- as.integer(strsplit(a, "[.-]")[[1L]])
    y <- as.integer(strsplit(b, "[.-]")[[1L]])
    for (i in seq_len(min(length(x), length(y)))) {
        if (x[[i]] > y[[i]]) return(1L)
        if (x[[i]] < y[[i]]) return(-1L)
    }
    if (length(x) > length(y)) 1L else if (length(x) < length(y)) -1L else 0L
}

modifyList <- function(x, val, keep.null = FALSE) {
    if (!is.list(x) || !is.list(val)) stop("is.list(x) and is.list(val) must be TRUE")
    for (nm in names(val)) {
        if (is.na(nm) || !nzchar(nm)) next
        y <- val[[nm]]
        if (is.list(x[[nm]]) && is.list(y)) y <- modifyList(x[[nm]], y, keep.null = keep.null)
        if (keep.null) x[nm] <- list(y) else x[[nm]] <- y
    }
    x
}
.rox_package_env <- function(ns, pkg, lib.loc) {
    if (is.environment(ns)) ns else
        if (is.null(ns)) loadNamespace(pkg, lib.loc = lib.loc) else loadNamespace(ns)
}
getFromNamespace <- function(x, ns, pos = -1, envir = as.environment(pos)) {
    ns <- if (missing(ns)) sub("^package:", "", environmentName(envir)) else ns
    ns <- .rox_package_env(ns, NULL, NULL)
    get(x, envir = ns, inherits = FALSE)
}
assignInNamespace <- function(x, value, ns, pos = -1, envir = as.environment(pos)) {
    ns <- if (missing(ns)) sub("^package:", "", environmentName(envir)) else ns
    ns <- .rox_package_env(ns, NULL, NULL)
    locked <- bindingIsLocked(x, ns)
    if (locked) {
        unlockBinding(x, ns)
        on.exit(lockBinding(x, ns))
    }
    assign(x, value, envir = ns)
    invisible(NULL)
}
getS3method <- function(f, class, optional = FALSE, envir = parent.frame()) {
    if (!exists(f, mode = "function", envir = envir)) {
        if (optional) return(NULL)
        stop("no function '", f, "' could be found")
    }
    .Internal(C_rox_get_s3_method(f, class, optional, envir))
}

person <- function(given = NULL, family = NULL, middle = NULL, email = NULL, role = NULL,
                   comment = NULL, first = NULL, last = NULL) {
    if (!is.null(first)) given <- c(first, given)
    if (!is.null(last)) family <- c(family, last)
    if (!is.null(middle)) {
        warning("It is recommended to use ‘given’ instead of ‘middle’.")
        given <- c(given, middle)
    }
    fields <- list(given = given, family = family, role = role, email = email, comment = comment)
    if (all(vapply(fields, is.null, TRUE))) return(structure(list(), class = "person"))
    size <- vapply(fields, function(x) if (is.list(x)) length(x) else 1L, 0L)
    n <- max(size)
    out <- lapply(seq_len(n), function(i) lapply(fields, function(x) {
        if (is.list(x) && length(x)) x[[(i - 1L) %% length(x) + 1L]] else x
    }))
    structure(out, class = "person")
}
`$.person` <- function(x, name) {
    fields <- lapply(unclass(x), function(y) y[[name]])
    if (length(fields) == 1L) fields[[1L]] else fields
}
`[.person` <- function(x, i, ...) structure(unclass(x)[i], class = "person")
c.person <- function(..., recursive = FALSE) structure(
    unlist(lapply(list(...), unclass), recursive = FALSE, use.names = FALSE), class = "person")
format.person <- function(x, include = c("given", "family", "email", "role", "comment"),
                          braces = list(email = c("<", ">"), role = c("[", "]"), comment = c("(", ")")),
                          collapse = list(given = " ", family = " ", role = ", ", comment = ", "), ...) {
    vapply(unclass(x), function(p) {
        fields <- vapply(include, function(nm) {
            value <- p[[nm]]
            if (!length(value)) return("")
            if (identical(nm, "comment") && !is.null(names(value))) {
                named <- nzchar(names(value))
                value[named] <- paste0(names(value)[named], ": ", value[named])
            }
            sep <- collapse[[nm]]
            value <- paste(value, collapse = if (is.null(sep)) " " else sep)
            brace <- braces[[nm]]
            if (length(brace) == 2L) value <- paste0(brace[[1L]], value, brace[[2L]])
            value
        }, "")
        paste(fields[nzchar(fields)], collapse = " ")
    }, "", USE.NAMES = FALSE)
}
print.person <- function(x, ...) {
    writeLines(format(x, ...))
    invisible(x)
}
as.person <- function(x) UseMethod("as.person")
as.person.person <- function(x) x
as.person.default <- function(x) as.person.character(as.character(x))
as.person.character <- function(x) {
    read <- function(s) {
        field <- function(pattern) {
            m <- regexec(pattern, s, perl = TRUE)[[1L]]
            if (length(m) < 2L) return(NULL)
            n <- attr(m, "match.length")
            substr(s, m[[2L]], m[[2L]] + n[[2L]] - 1L)
        }
        email <- field("<([^>]*)>")
        role <- field("\\[([^]]*)\\]")
        comment <- field("\\(([^)]*)\\)")
        name <- trimws(sub("[<\\[(].*$", "", s, perl = TRUE))
        parts <- strsplit(name, "[[:space:]]+")[[1L]]
        family <- if (length(parts) > 1L) tail(parts, 1L) else NULL
        given <- if (length(parts) > 1L) head(parts, -1L) else parts
        role <- if (length(role)) trimws(strsplit(role, ",", fixed = TRUE)[[1L]]) else NULL
        unclass(person(given, family, email = email, role = role, comment = comment))[[1L]]
    }
    structure(lapply(x, read), class = "person")
}

packageDescription <- function(pkg, lib.loc = NULL, fields = NULL, drop = TRUE, encoding = "") {
    path <- if (exists("find.package", envir = baseenv(), inherits = FALSE))
        find.package(pkg, lib.loc = lib.loc, quiet = TRUE) else character()
    file <- if (length(path)) file.path(path[[1L]], "DESCRIPTION") else ""
    if (nzchar(file) && file.exists(file)) {
        dcf <- read.dcf(file)
        out <- as.list(dcf[1L, ])
    } else if (is.null(lib.loc) && pkg %in% c("base", "compiler", "datasets", "graphics", "grDevices",
                                              "grid", "methods", "parallel", "splines", "stats", "stats4", "tcltk",
                                              "tools", "utils")) {
        out <- list(Package = pkg, Version = "4.6.2", Priority = "base")
    } else {
        warning("no package '", pkg, "' was found")
        return(NA)
    }
    if (!is.null(fields)) {
        vals <- lapply(fields, function(field) if (is.null(out[[field]])) NA_character_ else out[[field]])
        if (drop && length(vals) == 1L) return(vals[[1L]])
        names(vals) <- fields
        out <- vals
    }
    structure(out, class = "packageDescription", fields = fields, file = file)
}
installed.packages <- function(lib.loc = NULL, priority = NULL, noCache = FALSE, fields = NULL,
                               subarch = .Platform$r_arch, ...) {
    defaults <- c("Package", "LibPath", "Version", "Priority", "Depends", "Imports", "LinkingTo",
                  "Suggests", "Enhances", "License", "License_is_FOSS", "License_restricts_use",
                  "OS_type", "MD5sum", "NeedsCompilation", "Built", "Published")
    columns <- unique(c(defaults, fields))
    include.base <- is.null(lib.loc) || .Library %in% lib.loc
    if (is.null(lib.loc)) lib.loc <- if (exists(".libPaths", envir = baseenv())) .libPaths() else .Library
    records <- list()
    for (lib in lib.loc) {
        paths <- list.dirs(lib, full.names = TRUE, recursive = FALSE)
        for (path in paths[file.exists(file.path(paths, "DESCRIPTION"))]) {
            desc <- read.dcf(file.path(path, "DESCRIPTION"))
            value <- as.list(desc[1L, ])
            value$LibPath <- lib
            if (!is.null(value$Package)) records[[length(records) + 1L]] <- value
        }
    }
    if (include.base) for (pkg in c("base", "datasets", "graphics", "grDevices", "grid", "methods",
                                   "stats", "tools", "utils")) {
        if (!any(vapply(records, function(r) identical(r$Package, pkg), TRUE)))
            records[[length(records) + 1L]] <- list(Package = pkg, Version = "4.6.2",
                                                     LibPath = .Library, Priority = "base")
    }
    if (!is.null(priority)) records <- Filter(function(r) r$Priority %in% priority, records)
    out <- matrix(NA_character_, nrow = length(records), ncol = length(columns))
    for (i in seq_along(records)) out[i, ] <- vapply(columns, function(nm) {
        x <- records[[i]][[nm]]
        if (is.null(x)) NA_character_ else x
    }, "")
    dimnames(out) <- list(out[, 1L], columns)
    out
}
URLencode <- function(URL, reserved = FALSE, repeated = FALSE)
    .Internal(C_rox_url_encode(as.character(URL), reserved, repeated))
file_test <- function(op, x, y) {
    if (op == "-f") return(file.exists(x) & !dir.exists(x))
    if (op == "-d") return(dir.exists(x))
    if (op == "-r") return(file.access(x, 4L) == 0L)
    if (op == "-w") return(file.access(x, 2L) == 0L)
    if (op == "-x") return(file.access(x, 1L) == 0L)
    if (op == "-s") return(!is.na(file.size(x)) & file.size(x) > 0L)
    if (op %in% c("-nt", "-ot")) {
        a <- file.mtime(x)
        b <- file.mtime(y)
        return(!is.na(a) & !is.na(b) & if (op == "-nt") a > b else a < b)
    }
    stop("test '", op, "' is not available")
}
glob2rx <- function(pattern, trim.head = FALSE, trim.tail = TRUE) {
    convert <- function(x) {
        if (is.na(x)) x <- "NA"
        chars <- strsplit(x, "", fixed = TRUE)[[1L]]
        out <- vapply(chars, function(ch) {
            if (ch == "*") ".*" else if (ch == "?") "." else
                if (ch %in% c(".", "[", "{", "(", ")", "^", "$", "\\")) paste0("\\", ch) else ch
        }, "")
        pattern <- paste(out, collapse = "")
        if (identical(pattern, ".*") && trim.tail) return("^")
        if (trim.tail && endsWith(pattern, ".*")) pattern <- substr(pattern, 1L, nchar(pattern) - 2L)
        else pattern <- paste0(pattern, "$")
        if (trim.head && startsWith(pattern, ".*") && !identical(pattern, ".*")) {
            substring(pattern, 3L)
        } else paste0("^", pattern)
    }
    vapply(as.character(pattern), convert, "", USE.NAMES = FALSE)
}
combn <- function(x, m, FUN = NULL, simplify = TRUE, ...) {
    if (length(x) == 1L && is.numeric(x) && x > 0L && trunc(x) == x) x <- seq_len(x)
    if (length(m) != 1L || !is.numeric(m) || is.na(m) || m < 0) stop("invalid 'm' argument")
    m <- as.integer(m)
    n <- length(x)
    if (n < m) stop("n < m")
    count <- choose(n, m)
    ii <- seq_len(m)
    result <- vector("list", count)
    for (j in seq_len(count)) {
        item <- x[ii]
        result[j] <- list(if (is.null(FUN)) item else FUN(item, ...))
        if (j == count) break
        p <- m
        while (p > 0L && ii[[p]] == n - m + p) p <- p - 1L
        end <- seq.int(p, m)
        ii[end] <- ii[[p]] + seq_along(end)
    }
    if (!simplify) return(result)
    first <- result[[1L]]
    size <- length(first)
    d <- dim(first)
    if (is.null(d)) d <- size
    if (is.null(FUN) && is.factor(x)) {
        result <- structure(unlist(lapply(result, as.integer), use.names = FALSE),
                            levels = levels(x), class = class(x))
    } else if (size == 0L) result <- first
    else result <- unlist(result, recursive = FALSE, use.names = FALSE)
    dim(result) <- c(d, count)
    result
}

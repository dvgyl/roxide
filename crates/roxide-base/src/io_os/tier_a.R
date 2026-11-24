open <- function(con, ...) UseMethod("open")
open.connection <- function(con, open = "r", blocking = TRUE, ...)
    .Internal(rox_io_open(con, open, blocking))

url <- function(description, open = "", blocking = TRUE, encoding = getOption("encoding"),
                method = getOption("url.method", default = "default"), headers = NULL) {
    method <- match.arg(method, c("default", "internal", "libcurl", "wininet"))
    if (length(headers) &&
        (is.null(names(headers)) || anyNA(names(headers)) || any(!nzchar(names(headers))) || anyNA(headers)))
        stop("'headers' must have names and must not be NA")
    if (!is.character(description) || length(description) != 1L || is.na(description))
        stop("invalid 'description' argument")
    if (method == "wininet" && .Platform$OS.type != "windows")
        stop('method = "wininet" is only supported on Windows')
    if (startsWith(description, "file://"))
        return(file(substring(description, 8L), open, blocking, encoding))
    if (!grepl("^(https?|ftps?)://", description))
        stop("URL scheme unsupported by this method")
    if (method == "internal")
        stop(paste0(sub("://.*$", "://", description),
                    ' URLs are not supported by the "internal" method'))
    .Internal(rox_io_url(description, open))
}

system <- function(command, intern = FALSE, ignore.stdout = FALSE, ignore.stderr = FALSE,
                   wait = TRUE, input = NULL, show.output.on.console = TRUE,
                   minimized = FALSE, invisible = TRUE, timeout = 0) {
    .Internal(rox_io_system(command, intern, ignore.stdout, ignore.stderr, wait, input, timeout))
}

path.expand <- function(path) .Internal(rox_path_expand(as.character(path)))
file.remove <- function(...) .Internal(rox_file_remove(as.character(c(...))))
file.copy <- function(from, to, overwrite = recursive, recursive = FALSE, copy.mode = TRUE, copy.date = FALSE)
    .Internal(rox_file_copy(as.character(from), as.character(to), overwrite, recursive))
file.append <- function(file1, file2) .Internal(rox_file_append(as.character(file1), as.character(file2)))
file.access <- function(names, mode = 0L) .Internal(rox_file_access(as.character(names), mode))
file.size <- function(...) file.info(..., extra_cols = FALSE)$size
file.mtime <- function(...) file.info(..., extra_cols = FALSE)$mtime

list.dirs <- function(path = ".", full.names = TRUE, recursive = TRUE) {
    out <- lapply(path, function(dir) {
        if (is.na(dir) || !dir.exists(dir)) return(character())
        entries <- list.files(dir, all.files = TRUE, full.names = FALSE, recursive = recursive,
                              include.dirs = TRUE, no.. = TRUE)
        entries <- entries[dir.exists(file.path(dir, entries))]
        if (recursive) entries <- c("", entries)
        if (!full.names) return(entries)
        out <- file.path(dir, entries)
        out[!nzchar(entries)] <- dir
        out
    })
    if (!length(out)) character() else sort(as.character(unlist(out, use.names = FALSE)))
}
Sys.glob <- function(paths, dirmark = FALSE) {
    expand <- function(path) {
        if (is.na(path) || !nzchar(path)) return(character())
        if (!grepl("[*?\\[]", path)) return(path[file.exists(path)])
        parent <- dirname(path)
        dirs <- if (identical(parent, ".")) "." else expand(parent)
        pattern <- basename(path)
        pattern <- gsub("([.()+^$${}|])", "\\\\\\1", pattern, perl = TRUE)
        pattern <- gsub("*", ".*", pattern, fixed = TRUE)
        pattern <- gsub("?", ".", pattern, fixed = TRUE)
        matches <- lapply(dirs, function(dir) {
            if (!dir.exists(dir)) return(character())
            names <- list.files(dir, paste0("^", pattern, "$"), all.files = startsWith(basename(path), "."),
                                no.. = TRUE)
            if (identical(dir, ".") && !startsWith(path, "./")) names else file.path(dir, names)
        })
        sort(as.character(unlist(matches, use.names = FALSE)))
    }
    out <- as.character(unlist(lapply(path.expand(paths), expand), use.names = FALSE))
    if (dirmark) {
        dirs <- dir.exists(out) & !endsWith(out, "/")
        out[dirs] <- paste0(out[dirs], "/")
    }
    out
}

Sys.time <- function() structure(.Internal(Sys.time()), class = c("POSIXct", "POSIXt"))
Sys.Date <- function() as.Date(Sys.time(), tz = Sys.timezone())
Sys.sleep <- function(time) .Internal(rox_sys_sleep(time))
Sys.getpid <- function() .Internal(rox_sys_pid())
Sys.which <- function(names) .Internal(rox_sys_which(names))
Sys.getlocale <- function(category = "LC_ALL") {
    if (!(category %in% c("LC_ALL","LC_COLLATE","LC_CTYPE","LC_MONETARY","LC_NUMERIC","LC_TIME",
                          "LC_MESSAGES","LC_PAPER","LC_MEASUREMENT"))) stop("invalid 'category' argument")
    if (identical(category, "LC_NUMERIC")) return("C")
    .Internal(rox_sys_locale())[[1L]]
}
Sys.setlocale <- function(category = "LC_ALL", locale = "") {
    current <- Sys.getlocale(category)
    if (locale != "" && locale != current) {
        warning(paste0("OS reports request to set locale to \"", locale, "\" cannot be honored"))
        return("")
    }
    current
}
.sys.timezone <- NA_character_
lockBinding(".sys.timezone", baseenv())

Sys.timezone <- function(location = TRUE) {
    if (!location) warning("Sys.timezone(location = FALSE) is defunct and ignored")
    zone <- Sys.getenv("TZ")
    if (nzchar(zone)) return(zone)
    zone <- .sys.timezone
    if (is.na(zone)) {
        zone <- .Internal(rox_sys_timezone())
        if (bindingIsLocked(".sys.timezone", baseenv()))
            unlockBinding(".sys.timezone", baseenv())
        assign(".sys.timezone", zone, envir = baseenv())
        lockBinding(".sys.timezone", baseenv())
    }
    zone
}
l10n_info <- function() {
    locale <- .Internal(rox_sys_locale())
    latin <- grepl("8859[-_]?1$|latin1$", locale[[1L]], ignore.case = TRUE)
    list(MBCS = locale[[2L]], `UTF-8` = locale[[2L]], `Latin-1` = latin,
         codeset = if (locale[[2L]]) "UTF-8" else if (latin) "ISO-8859-1" else "ASCII")
}
capabilities <- function(what = NULL) {
    ans <- c(jpeg = FALSE, png = FALSE, tiff = FALSE, tcltk = FALSE, X11 = FALSE,
             aqua = FALSE, `http/ftp` = FALSE, sockets = FALSE, libxml = FALSE, fifo = FALSE,
             cledit = FALSE, iconv = TRUE, NLS = FALSE, Rprof = FALSE, profmem = FALSE,
             cairo = FALSE, ICU = FALSE, long.double = FALSE, libcurl = FALSE)
    if (is.null(what)) ans else ans[match(what, names(ans), nomatch = 0L)]
}
isatty <- function(con) .Internal(rox_isatty(con))
interactive <- function() .Internal(rox_interactive())
readline <- function(prompt = "") .Internal(rox_readline(as.character(prompt)))

date <- function() format(Sys.time(), "%a %b %e %H:%M:%S %Y", tz = Sys.timezone())
proc.time <- function() structure(.Internal(rox_proc_time()),
    names = c("user.self","sys.self","elapsed","user.child","sys.child"), class = "proc_time")
system.time <- function(expr, gcFirst = TRUE) {
    if (gcFirst) {
        if (exists("gc", mode = "function", envir = baseenv(), inherits = FALSE)) gc()
        else .Internal(rox_time_safepoint())
    }
    before <- proc.time()
    force(expr)
    structure((proc.time() - before)[1L:5L], class = "proc_time")
}
print.proc_time <- function(x, ...) {
    print.default(x[1L:3L], ...)
    invisible(x)
}

quit <- function(save = "default", status = 0L, runLast = TRUE) {
    if (!(save %in% c("default", "no", "yes", "ask"))) stop("unrecognized value of 'save'")
    if (runLast && exists(".Last", envir = globalenv(), mode = "function", inherits = FALSE))
        get(".Last", envir = globalenv(), mode = "function", inherits = FALSE)()
    .Internal(rox_quit(status, runLast))
}
q <- quit

source <- function(file, local = FALSE, echo = verbose, print.eval = echo, exprs,
                   spaced = useFile, verbose = getOption("verbose", FALSE), prompt.echo = getOption("prompt", "> "),
                   max.deparse.length = 150L, width.cutoff = 60L, deparseCtrl = "showAttributes",
                   chdir = FALSE, encoding = getOption("encoding", "native.enc"), continue.echo = getOption("continue", "+ "),
                   skip.echo = 0L, keep.source = getOption("keep.source")) {
    envir <- if (is.environment(local)) local else if (isTRUE(local)) parent.frame() else
        if (identical(local, FALSE)) globalenv() else stop("'local' must be TRUE, FALSE or an environment")
    useFile <- missing(exprs)
    if (useFile) {
        exprs <- parse(file = file, keep.source = keep.source, encoding = encoding)
        if (chdir && is.character(file)) {
            old <- setwd(dirname(file))
            on.exit(setwd(old))
        }
    }
    last <- list(value = NULL, visible = FALSE)
    for (i in seq_along(exprs)) {
        if (echo) {
            if (spaced) cat("\n")
            lines <- deparse(exprs[[i]], width.cutoff = width.cutoff)
            cat(prompt.echo, lines[[1L]], "\n", sep = "")
            for (line in lines[-1L]) cat(continue.echo, line, "\n", sep = "")
        }
        ei <- if (is.expression(exprs)) exprs[i] else exprs[[i]]
        last <- withVisible(eval(ei, envir))
        if (print.eval && last$visible) print(last$value)
    }
    invisible(last)
}
sys.source <- function(file, envir = baseenv(), chdir = FALSE, keep.source = getOption("keep.source"),
                       keep.parse.data = getOption("keep.parse.data"), toplevel.env = envir) {
    source(file, local = envir, chdir = chdir, keep.source = keep.source)
    invisible(NULL)
}
sink <- function(file = NULL, append = FALSE, type = c("output", "message"), split = FALSE)
    .Internal(rox_sink(file, append, match.arg(type) == "message", split))
sink.number <- function(type = c("output", "message"))
    .Internal(rox_sink_number(match.arg(type) == "message"))

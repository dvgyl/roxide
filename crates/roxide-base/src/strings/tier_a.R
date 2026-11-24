gettext <- function(..., domain = NULL, trim = TRUE) {
    ans <- c(...)
    if (is.null(ans)) character() else as.character(ans)
}
gettextf <- function(fmt, ..., domain = NULL) sprintf(gettext(fmt, domain = domain), ...)
ngettext <- function(n, msg1, msg2, domain = NULL) {
    if (length(n) != 1L || is.na(n)) stop("invalid 'n' argument")
    if (n == 1L) msg1 else msg2
}

.rox_quote <- function(x, q, double) {
    if (!length(x)) return(character())
    pair <- if (is.character(q) && length(q) == 4L) {
        if (double) q[3L:4L] else q[1L:2L]
    } else if (identical(q, FALSE)) rep(if (double) "\"" else "'", 2L)
    else if (identical(q, "TeX")) {
        if (double) c("``", "''") else c("`", "'")
    } else if (double) c("“", "”") else c("‘", "’")
    paste0(pair[[1L]], x, pair[[2L]])
}
sQuote <- function(x, q = getOption("useFancyQuotes", TRUE)) .rox_quote(x, q, FALSE)
dQuote <- function(x, q = getOption("useFancyQuotes", TRUE)) .rox_quote(x, q, TRUE)

shQuote <- function(string, type = if (.Platform$OS.type == "windows") "cmd" else "sh") {
    if (!length(string)) return(character())
    opts <- c("sh", "csh", "cmd", "cmd2")
    if (length(type) != 1L || !(type %in% opts)) stop("invalid 'type' argument")
    if (identical(type, "cmd2")) return(gsub("([&|<>()@^])", "^\\1", string, perl = TRUE))
    if (identical(type, "cmd")) {
        x <- gsub('(\\\\*)"', '\\1\\1\\\\"', string, perl = TRUE)
        x <- sub('(\\\\+)$', '\\1\\1', x, perl = TRUE)
        return(paste0('"', x, '"'))
    }
    if (!any(grepl("'", string, fixed = TRUE))) return(paste0("'", string, "'"))
    if (identical(type, "csh")) return(paste0("'", gsub("'", "'\\\\''", string, fixed = TRUE), "'"))
    paste0('"', gsub('([\\\`$"\\\\])', '\\\\\\1', string, perl = TRUE), '"')
}

strrep <- function(x, times) {
    x <- as.character(x)
    times <- as.integer(times)
    if (any(times < 0L, na.rm = TRUE)) stop("invalid 'times' value")
    if (!length(x) || !length(times)) return(character())
    n <- max(length(x), length(times))
    x <- rep_len(x, n)
    times <- rep_len(times, n)
    vapply(seq_len(n), function(i) {
        if (is.na(x[[i]]) || is.na(times[[i]])) NA_character_
        else paste(rep(x[[i]], times[[i]]), collapse = "")
    }, "")
}
strtrim <- function(x, width) {
    x <- as.character(x)
    width <- as.integer(width)
    if (!length(width) || any(is.na(width)) || any(width < 0L)) stop("invalid 'width' argument")
    if (!length(x)) return(x)
    width <- rep_len(width, length(x))
    vapply(seq_along(x), function(i) {
        xi <- x[[i]]
        if (is.na(xi)) return(NA_character_)
        if (nchar(xi, type = "width") <= width[[i]]) return(xi)
        count <- nchar(xi, type = "chars")
        lo <- 0L
        hi <- count
        while (lo < hi) {
            mid <- ceiling((lo + hi) / 2)
            if (nchar(substr(xi, 1L, mid), type = "width") <= width[[i]]) lo <- mid
            else hi <- mid - 1L
        }
        substr(xi, 1L, lo)
    }, "")
}
casefold <- function(x, upper = FALSE) if (upper) toupper(x) else tolower(x)
utf8ToInt <- function(x) .Internal(rox_utf8_to_int(x))
intToUtf8 <- function(x, multiple = FALSE, allow_surrogate_pairs = FALSE)
    .Internal(rox_int_to_utf8(x, multiple, allow_surrogate_pairs))
iconv <- function(x, from = "", to = "", sub = NA, mark = TRUE, toRaw = FALSE)
    .Internal(rox_iconv(x, from, to, sub, mark, toRaw))
make.unique <- function(names, sep = ".") .Internal(rox_make_unique(names, sep))
strtoi <- function(x, base = 0L) .Internal(rox_strtoi(x, base))

as.hexmode <- function(x) {
    if (is.integer(x)) {
        y <- x
    } else if (is.character(x)) {
        y <- strtoi(x, 16L)
        if (anyNA(y) || any(y < 0L))
            stop("'x' cannot be coerced to class \"hexmode\"")
    } else if (is.double(x)) {
        y <- as.integer(x)
        if (!all(is.na(x) | (!is.na(y) & x == y)))
            stop("'x' cannot be coerced to class \"hexmode\"")
    } else {
        stop("'x' cannot be coerced to class \"hexmode\"")
    }
    class(y) <- "hexmode"
    y
}

format.hexmode <- function(x, width = NULL, upper.case = FALSE, ...) {
    values <- as.integer(x)
    conversion <- if (upper.case) "X" else "x"
    if (is.null(width)) {
        fields <- sprintf(paste0("%", conversion), values)
        width <- max(c(0L, nchar(fields[!is.na(x)])))
    }
    y <- sprintf(paste0("%0", width, conversion), values)
    y[is.na(x)] <- NA_character_
    attrs <- attributes(x)
    attrs <- attrs[names(attrs) %in% c("names", "dim", "dimnames")]
    if (length(attrs)) attributes(y) <- attrs
    y
}

as.character.hexmode <- function(x, ...) as.vector(format.hexmode(x, width = 0L))
as.list.function <- function(x, ...) c(as.list(formals(x)), list(body(x)))

encodeString <- function(x, width = 0L, quote = "", na.encode = TRUE, justify = "left") {
    x <- .Internal(rox_encode_string(as.character(x), quote))
    if (na.encode) x[is.na(x)] <- if (nzchar(quote)) "NA" else "<NA>"
    if (is.null(width)) width <- max(c(0L, nchar(x, type = "width", keepNA = TRUE)), na.rm = TRUE)
    if (!(justify %in% c("left", "right", "centre", "none"))) stop("invalid 'justify' argument")
    if (!length(width) || is.na(width) || width < 0L) stop("invalid 'width' argument")
    if (identical(justify, "none") || !width || !length(x)) return(x)
    n <- width - nchar(x, type = "width")
    n[which(n < 0L)] <- 0L
    left <- if (identical(justify, "right")) n else if (identical(justify, "centre")) n %/% 2L else 0L
    y <- paste0(strrep(" ", left), x, strrep(" ", n - left))
    y[is.na(x)] <- NA_character_
    names(y) <- names(x)
    y
}

prettyNum <- function(x, big.mark = "", big.interval = 3L, small.mark = "", small.interval = 5L,
                      decimal.mark = getOption("OutDec", "."), input.d.mark = decimal.mark,
                      preserve.width = c("common", "individual", "none"), zero.print = NULL,
                      drop0trailing = FALSE, is.cmplx = NA, ...) {
    if (!is.character(x)) {
        format_value <- function(value)
            format(value, big.mark = big.mark, big.interval = big.interval,
                   decimal.mark = decimal.mark, drop0trailing = drop0trailing, ...)
        if (is.numeric(x) || is.complex(x) || is.logical(x)) {
            y <- vapply(x, format_value, "", USE.NAMES = FALSE)
            names(y) <- names(x)
            return(y)
        }
        return(format_value(x))
    }
    pad <- nchar(x)
    y <- trimws(x)
    if (nzchar(big.mark)) {
        pat <- sprintf("(?<=\\d)(?=(?:\\d{%d})+(?:\\D|$))", big.interval)
        whole <- sub("[.,].*$", "", y)
        tail <- substring(y, nchar(whole) + 1L)
        y <- paste0(gsub(pat, big.mark, whole, perl = TRUE), tail)
    }
    if (!identical(input.d.mark, decimal.mark)) y <- sub(input.d.mark, decimal.mark, y, fixed = TRUE)
    if (drop0trailing) {
        y <- sub("(\\.[0-9]*?[1-9])0+($|[eE])", "\\1\\2", y, perl = TRUE)
        y <- sub("\\.0+($|[eE])", "\\1", y, perl = TRUE)
    }
    if (!is.null(zero.print)) y[which(!is.na(x) & suppressWarnings(as.double(x) == 0))] <- zero.print
    y[is.na(x)] <- NA_character_
    if (length(preserve.width) > 1L) preserve.width <- preserve.width[[1L]]
    if (length(y) && any(!is.na(y))) {
        if (identical(preserve.width, "common")) {
            width <- max(c(pad, nchar(y)), na.rm = TRUE)
            y <- encodeString(y, width = width, justify = "right", na.encode = FALSE)
        }
        if (identical(preserve.width, "individual")) {
            spaces <- pad - nchar(y)
            spaces[which(spaces < 0L)] <- 0L
            y <- paste0(strrep(" ", spaces), y)
            y[is.na(x)] <- NA_character_
        }
    }
    names(y) <- names(x)
    y
}

formatC <- function(x, digits = NULL, width = NULL, format = NULL, flag = "", mode = NULL,
                    big.mark = "", big.interval = 3L, small.mark = "", small.interval = 5L,
                    decimal.mark = getOption("OutDec", "."), preserve.width = "individual",
                    zero.print = NULL, drop0trailing = FALSE) {
    if (!length(x)) return(character())
    if (!is.null(mode)) storage.mode(x) <- mode
    if (is.character(x)) return(format(x, width = width))
    if (is.null(format)) format <- if (is.integer(x)) "d" else "g"
    default.digits <- is.null(digits)
    if (default.digits) digits <- if (format == "d") 0L else 4L
    if (is.null(width)) width <- if (format == "d" || default.digits || format == "fg") 1L else digits + 1L
    if (format == "d") {
        y <- sprintf(paste0("%", flag, width, "d"), as.integer(x))
    } else if (format == "fg") {
        y <- vapply(x, function(v) format.default(v, trim = TRUE, digits = digits, width = abs(width),
                                                   scientific = FALSE), "", USE.NAMES = FALSE)
    } else {
        y <- sprintf(paste0("%", flag, width, ".", digits, format), x)
    }
    if (is.numeric(x) && any(!is.finite(x))) {
        nf <- !is.finite(x)
        special <- trimws(as.character(x[nf]))
        special[is.na(x[nf]) & !is.nan(x[nf])] <- "NA"
        y[nf] <- encodeString(special, width = max(abs(width), nchar(special)),
                              justify = if (width < 0L || grepl("-", flag, fixed = TRUE)) "left" else "right")
    }
    y <- prettyNum(y, big.mark = big.mark, big.interval = big.interval, small.mark = small.mark,
                   small.interval = small.interval, decimal.mark = decimal.mark, input.d.mark = ".",
                   preserve.width = preserve.width, zero.print = zero.print, drop0trailing = drop0trailing)
    names(y) <- names(x)
    y
}

toString <- function(x, ...) UseMethod("toString")
toString.default <- function(x, width = NULL, ...) {
    text <- paste(as.character(x), collapse = ", ")
    if (!is.null(width) && width > 0L && nchar(text, type = "width") > width)
        text <- paste0(strtrim(text, max(0L, width - 4L)), "....")
    text
}

strwrap <- function(x, width = 0.9 * getOption("width"), indent = 0L, exdent = 0L,
                    prefix = "", simplify = TRUE, initial = prefix) {
    if (width <= 0L || indent < 0L || exdent < 0L) stop("invalid width or indentation")
    wrap <- function(text) {
        if (is.na(text)) return("")
        paragraphs <- strsplit(text, "\n[[:blank:]]*\n")[[1L]]
        lines <- character()
        for (p in paragraphs) {
            words <- strsplit(trimws(p), "[[:space:]]+")[[1L]]
            if (length(lines)) lines <- c(lines, prefix)
            line <- paste0(if (length(lines)) prefix else initial, strrep(" ", indent))
            first <- TRUE
            for (word in words) {
                next.line <- paste0(line, if (first) "" else " ", word)
                if (!first && nchar(next.line, type = "width") >= width) {
                    lines <- c(lines, line)
                    line <- paste0(prefix, strrep(" ", exdent), word)
                } else line <- next.line
                first <- FALSE
            }
            if (!first) lines <- c(lines, line)
        }
        lines
    }
    out <- lapply(as.character(x), wrap)
    if (!simplify) return(out)
    if (!length(out)) character() else as.character(unlist(out, use.names = FALSE))
}

`substr<-` <- function(x, start, stop, value) {
    if (!is.character(x)) stop("replacing substrings in a non-character object")
    if (!length(value)) stop("invalid value")
    if (!length(x)) return(x)
    n <- length(x)
    start <- rep_len(as.integer(start), n)
    stop <- rep_len(as.integer(stop), n)
    value <- rep_len(as.character(value), n)
    out <- x
    for (i in seq_len(n)) {
        if (is.na(x[[i]]) || is.na(start[[i]]) || is.na(stop[[i]]) || is.na(value[[i]])) {
            out[[i]] <- NA_character_
        } else {
            begin <- max(1L, start[[i]])
            end <- min(nchar(x[[i]]), stop[[i]], begin + nchar(value[[i]]) - 1L)
            if (begin <= end) out[[i]] <- paste0(substr(x[[i]], 1L, begin - 1L),
                substr(value[[i]], 1L, end - begin + 1L), substring(x[[i]], end + 1L))
        }
    }
    out
}

regexec <- function(pattern, text, ignore.case = FALSE, perl = FALSE, fixed = FALSE, useBytes = FALSE) {
    m <- regexpr(pattern, text, ignore.case = ignore.case, perl = !fixed, fixed = fixed, useBytes = useBytes)
    start <- attr(m, "capture.start")
    len <- attr(m, "capture.length")
    nm <- attr(m, "capture.names")
    lapply(seq_along(m), function(i) {
        ans <- m[[i]]
        sizes <- attr(m, "match.length")[[i]]
        if (!is.na(ans) && ans >= 0L && !is.null(start)) {
            ans <- c(ans, start[i, ])
            sizes <- c(sizes, len[i, ])
            if (perl && any(nzchar(nm))) names(ans) <- c("", nm)
        }
        attr(ans, "match.length") <- sizes
        if (!is.na(m[[i]])) {
            attr(ans, "index.type") <- attr(m, "index.type")
            attr(ans, "useBytes") <- attr(m, "useBytes")
        }
        ans
    })
}

`regmatches<-` <- function(x, m, invert = FALSE, value) {
    if (length(x) != length(m)) stop("‘x’ and ‘m’ must have the same length")
    if (!length(value)) stop("replacement has length zero")
    if (is.na(invert)) stop("invalid 'invert' argument")
    vector.matches <- !is.list(m)
    take <- 0L
    for (i in seq_along(x)) {
        if (is.na(x[[i]])) next
        if (vector.matches) {
            pos <- m[[i]]
            size <- attr(m, "match.length")[[i]]
        } else {
            pos <- m[[i]]
            size <- attr(pos, "match.length")
        }
        ix <- which(pos >= 0L)
        pos <- pos[ix]
        size <- size[ix]
        if (!length(pos) && !invert) next
        take <- take + 1L
        replacement <- if (vector.matches && !invert) value[[(take - 1L) %% length(value) + 1L]]
            else value[[(i - 1L) %% length(value) + 1L]]
        replacement <- rep_len(as.character(replacement), length(pos) + as.integer(invert))
        out <- character()
        from <- 1L
        for (j in seq_along(pos)) {
            start <- pos[[j]]
            if (start < from) stop("overlapping matches")
            end <- start + size[[j]] - 1L
            out <- if (invert) c(out, replacement[[j]], substr(x[[i]], start, end)) else
                c(out, substr(x[[i]], from, start - 1L), replacement[[j]])
            from <- end + 1L
        }
        x[[i]] <- paste0(c(out, if (invert) replacement[[length(replacement)]] else substring(x[[i]], from)), collapse = "")
    }
    x
}

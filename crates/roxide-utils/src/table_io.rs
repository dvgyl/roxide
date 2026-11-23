//! Text table readers, writers, and type conversion for the utils package.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod convert;
mod read;
mod write;

use roxide_base::scan::r_strings;
use roxide_eval::{
    ArgEval, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{RLogical, RSession, SEXP_NIL, SexpType, ValueApi};

/// Register table I/O and type conversion in utils.
pub const MODULE: Module = Module {
    prims: &[
        internal(
            "C_rox_typeconvert",
            6,
            convert::type_convert,
            Visibility::On,
        ),
        internal("C_rox_countfields", 6, read::count_fields, Visibility::On),
        internal("C_rox_readtable", 26, read::read_table, Visibility::On),
        internal("C_rox_writetable", 12, write::write_table, Visibility::Off),
    ],
    prelude: r##"
type.convert <- function(x, ...) UseMethod("type.convert")
type.convert.default <- function(x, na.strings = "NA", as.is, dec = ".",
                                 numerals = c("allow.loss", "warn.loss", "no.loss"),
                                 tryLogical = TRUE, ...) {
    if (missing(as.is)) {
        warning("'as.is' should be specified by the caller; using TRUE")
        as.is <- TRUE
    }
    result <- .Internal(C_rox_typeconvert(as.character(x), na.strings, as.is, dec, numerals, tryLogical))
    if (!is.null(dim(x))) {
        dim(result) <- dim(x)
        dimnames(result) <- dimnames(x)
    }
    result
}
type.convert.list <- function(x, ...) lapply(x, type.convert, ...)
type.convert.data.frame <- function(x, ...) {
    for (i in seq_along(x)) x[[i]] <- type.convert(x[[i]], ...)
    x
}
count.fields <- function(file, sep = "", quote = "\"'", skip = 0L,
                         blank.lines.skip = TRUE, comment.char = "#")
    .Internal(C_rox_countfields(file, sep, quote, skip, blank.lines.skip, comment.char))
read.table <- function(file, header = FALSE, sep = "", quote = "\"'", dec = ".",
                       numerals = c("allow.loss", "warn.loss", "no.loss"),
                       row.names, col.names, as.is = !stringsAsFactors, tryLogical = TRUE,
                       na.strings = "NA", colClasses = NA, nrows = -1L, skip = 0L,
                       check.names = TRUE, fill = !blank.lines.skip, strip.white = FALSE,
                       blank.lines.skip = TRUE, comment.char = "#", allowEscapes = FALSE,
                       flush = FALSE, stringsAsFactors = FALSE, fileEncoding = "",
                       encoding = "unknown", text, skipNul = FALSE) {
    if (missing(text) || !missing(file)) text <- NULL
    if (missing(file)) file <- ""
    if (missing(col.names)) col.names <- NULL
    row.mode <- if (missing(row.names)) 0L else 1L
    if (row.mode == 0L) row.names <- NULL
    auto.header <- missing(header)
    if (!is.logical(header)) stop("invalid argument type")
    if (fileEncoding != "") encoding <- fileEncoding
    .Internal(C_rox_readtable(file, header, auto.header, sep, quote, dec, numerals,
                              row.mode, row.names, col.names, as.is, tryLogical, na.strings,
                              colClasses, nrows, skip, check.names, fill, strip.white,
                              blank.lines.skip, comment.char, allowEscapes, flush,
                              encoding, text, skipNul))
}
read.csv <- function(file, header = TRUE, sep = ",", quote = "\"", dec = ".",
                     fill = TRUE, comment.char = "", ...)
    read.table(file = file, header = header, sep = sep, quote = quote, dec = dec,
               fill = fill, comment.char = comment.char, ...)
read.csv2 <- function(file, header = TRUE, sep = ";", quote = "\"", dec = ",",
                      fill = TRUE, comment.char = "", ...)
    read.table(file = file, header = header, sep = sep, quote = quote, dec = dec,
               fill = fill, comment.char = comment.char, ...)
read.delim <- function(file, header = TRUE, sep = "\t", quote = "\"", dec = ".",
                       fill = TRUE, comment.char = "", ...)
    read.table(file = file, header = header, sep = sep, quote = quote, dec = dec,
               fill = fill, comment.char = comment.char, ...)
read.delim2 <- function(file, header = TRUE, sep = "\t", quote = "\"", dec = ",",
                        fill = TRUE, comment.char = "", ...)
    read.table(file = file, header = header, sep = sep, quote = quote, dec = dec,
               fill = fill, comment.char = comment.char, ...)
write.table <- function(x, file = "", append = FALSE, quote = TRUE, sep = " ", eol = "\n",
                        na = "NA", dec = ".", row.names = TRUE, col.names = TRUE,
                        qmethod = c("escape", "double"), fileEncoding = "")
    .Internal(C_rox_writetable(x, file, append, quote, sep, eol, na, dec,
                               row.names, col.names, qmethod, fileEncoding))
write.csv <- function(x, file = "", ..., row.names = TRUE) {
    .write.csv(x, file, row.names, ",", ".", ...)
}
write.csv2 <- function(x, file = "", ..., row.names = TRUE) {
    .write.csv(x, file, row.names, ";", ",", ...)
}
.write.csv <- function(x, file, row.names, separator, decimal, ..., append, sep, dec,
                       qmethod, col.names) {
    if (!missing(append)) warning("attempt to set 'append' ignored")
    if (!missing(sep)) warning("attempt to set 'sep' ignored")
    if (!missing(dec)) warning("attempt to set 'dec' ignored")
    if (!missing(qmethod)) warning("attempt to set 'qmethod' ignored")
    if (!missing(col.names)) warning("attempt to set 'col.names' ignored")
    write.table(x, file, ..., row.names = row.names,
                col.names = if (is.logical(row.names) && !row.names) TRUE else NA,
                sep = separator, dec = decimal, qmethod = "double")
}
"##,
};

const fn internal(name: &'static str, n: usize, fun: PrimFn, visibility: Visibility) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(n),
        dispatch: Dispatch::None,
        visibility,
        fun,
    }
}

fn attr(s: &mut RSession, value: SexpId, name: &str) -> EvalResult {
    let sym = s.intern(name)?;
    Ok(s.attr(value, sym))
}

fn set(s: &mut RSession, value: SexpId, name: &str, attr: SexpId) -> EvalResult<()> {
    let sym = s.intern(name)?;
    Ok(s.set_attrib(value, sym, attr)?)
}

fn string(ctx: &mut Ctx, call: &Call, value: SexpId, name: &str) -> EvalResult<String> {
    if ctx.session().type_of(value)? == SexpType::Str
        && let [Some(text)] = r_strings(ctx, value)?.as_mut_slice()
    {
        return Ok(std::mem::take(text));
    }
    Err(ctx.stop(call.err_call(), format!("invalid '{name}' argument")))
}

fn flag(ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<bool>> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
    Ok(ctx
        .session()
        .lgl(value)?
        .iter()
        .next()
        .and_then(RLogical::get))
}

fn limit(ctx: &mut Ctx, value: SexpId) -> EvalResult<usize> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Real)?;
    Ok(ctx
        .session()
        .real(value)?
        .iter()
        .next()
        .unwrap_or_default()
        .max(0.0) as usize)
}

fn choice<'a>(
    ctx: &mut Ctx,
    call: &Call,
    value: SexpId,
    choices: &'a [&str],
) -> EvalResult<&'a str> {
    let strings = r_strings(ctx, value)?;
    let selected = match strings.as_slice() {
        [] => choices.first().copied(),
        [Some(text)] => choices.iter().copied().find(|&c| c == text).or_else(|| {
            let mut matches = choices
                .iter()
                .copied()
                .filter(|c| !text.is_empty() && c.starts_with(text));
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        }),
        all if all
            .iter()
            .map(Option::as_deref)
            .eq(choices.iter().copied().map(Some)) =>
        {
            choices.first().copied()
        }
        _ => None,
    };
    selected.ok_or_else(|| {
        ctx.stop(
            call.err_call(),
            format!(
                "'arg' should be one of {}",
                choices
                    .iter()
                    .map(|c| format!("“{c}”"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
    })
}

fn has_class(ctx: &mut Ctx, value: SexpId, class: &str) -> EvalResult<bool> {
    let classes = attr(ctx.session_mut(), value, "class")?;
    Ok(classes != SEXP_NIL
        && r_strings(ctx, classes)?
            .iter()
            .any(|c| c.as_deref() == Some(class)))
}

//! Parse Rd documentation, render plain-text help, and handle paths.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod parser;
mod paths;
mod render;
mod tree;

use roxide_eval::{
    ArgEval, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{BASE_ENV, CharId, Encoding, NA_REAL, RLogical, SEXP_NIL, SexpType, ValueApi};
use std::{borrow::Cow, path::Path};

/// Register Rd and file path operations in the tools namespace.
pub const MODULE: Module = Module {
    prims: &[
        internal("rox_rd_parse", 8, parse_rd),
        internal("rox_rd_text", 7, render::rd_to_text),
        internal("rox_rd_character", 1, tree::as_character),
        internal("rox_tools_ext", 3, paths::extensions),
        internal("rox_tools_non_ascii", 1, paths::non_ascii),
    ],
    prelude: r#"
parse_Rd <- function(file, srcfile = NULL, encoding = "unknown", verbose = FALSE,
                     fragment = FALSE, warningCalls = TRUE, macros = TRUE,
                     permissive = FALSE, keep_empty_sections = TRUE)
    .Internal(rox_rd_parse(file, srcfile, encoding, fragment, warningCalls, macros,
                           permissive, keep_empty_sections))
as.character.Rd <- function(x, ...) .Internal(rox_rd_character(x))
.rd_base_as_character <- as.character
assign("as.character", function(x, ...) {
    if (inherits(x, "Rd")) as.character.Rd(x, ...)
    else .rd_base_as_character(x, ...)
}, envir = baseenv())
print.Rd <- function(x, ...) {
    cat(as.character.Rd(x), sep = "")
    invisible(x)
}
.rd_options <- list(width = 80L, minIndent = 10L, extraIndent = 4L,
                    sectionIndent = 5L, sectionExtra = 2L, itemBullet = "• ",
                    enumFormat = function(n) paste0(n, ". "), descStyle = "linebreak",
                    showURLs = FALSE, code_quote = TRUE, underline_titles = TRUE)
Rd2txt_options <- function(...) {
    supplied <- list(...)
    if (!length(supplied)) return(.rd_options)
    if (is.null(names(supplied)) && length(supplied) == 1L && is.list(supplied[[1L]]))
        supplied <- supplied[[1L]]
    keys <- names(supplied)
    if (is.null(keys)) keys <- character()
    old <- .rd_options[keys]
    updated <- .rd_options
    for (key in keys) if (nzchar(key)) updated[key] <- supplied[key]
    .rd_options <<- updated
    invisible(old)
}
Rd2txt <- function(Rd, out = "", package = "", defines = "unix", stages = "render",
                   outputEncoding = "", fragment = FALSE, options, ...) {
    if (missing(options)) options <- list()
    defaults <- Rd2txt_options()
    for (key in names(options)) defaults[key] <- options[key]
    options <- defaults
    if (!inherits(Rd, "Rd")) Rd <- parse_Rd(Rd, fragment = fragment, ...)
    env <- new.env(parent = globalenv())
    evaluate <- function(code, results) {
        expression <- parse(text = code)
        if (results == "verbatim") return(paste(capture.output(eval(expression, env)), collapse = "\n"))
        value <- eval(expression, env)
        if (results == "hide") return("")
        paste(as.character(value), collapse = " ")
    }
    lines <- .Internal(rox_rd_text(Rd, fragment, options, defines, stages, outputEncoding, evaluate))
    encoded <- outputEncoding == "latin1" || outputEncoding == "ISO-8859-1" || outputEncoding == "ASCII"
    if (is.character(out)) {
        if (identical(out, "")) {
            if (length(lines)) writeLines(lines, useBytes = encoded)
        } else {
            con <- file(out, "wt")
            on.exit(close(con))
            writeLines(lines, con, useBytes = encoded)
        }
    } else {
        writeLines(lines, out, useBytes = encoded)
        out <- .Internal(rox_io_description(out))
    }
    invisible(out)
}
file_ext <- function(x) .Internal(rox_tools_ext(as.character(x), FALSE, TRUE))
file_path_sans_ext <- function(x, compression = FALSE)
    .Internal(rox_tools_ext(as.character(x), compression, FALSE))
showNonASCII <- function(x) {
    if (!is.character(x)) x <- as.character(x)
    invisible(x[.Internal(rox_tools_non_ascii(x))])
}
showNonASCIIfile <- function(file) showNonASCII(readLines(file))
file_path_as_absolute <- function(x) {
    if (length(x) != 1L) stop("'x' must be a character string")
    if (!is.character(x)) stop("invalid 'path' argument")
    if (!file.exists(x)) stop(paste0("file '", x, "' does not exist"))
    normalizePath(x, winslash = "/", mustWork = TRUE)
}
"#,
};

pub(super) const fn internal(name: &'static str, count: usize, fun: PrimFn) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(count),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
        fun,
    }
}

fn flag(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<bool> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
    let flag = ctx
        .session()
        .lgl(value)?
        .iter()
        .next()
        .and_then(RLogical::get);
    flag.ok_or_else(|| ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"))
}

fn attr(ctx: &mut Ctx, value: SexpId, name: &str) -> EvalResult {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session().attr(value, key))
}

pub(super) fn set_attr(ctx: &mut Ctx, value: SexpId, name: &str, attr: SexpId) -> EvalResult<()> {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session_mut().set_attrib(value, key, attr)?)
}

pub(super) fn chars<'a>(ctx: &mut Ctx, strings: impl IntoIterator<Item = &'a str>) -> EvalResult {
    let ids = strings
        .into_iter()
        .map(|s| ctx.session_mut().mk_char(s.as_bytes(), Encoding::Utf8))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ctx.session_mut().alloc_str_from(&ids)?)
}

fn set_str(ctx: &mut Ctx, value: SexpId, key: &str, text: &str) -> EvalResult<()> {
    let text = ctx.session_mut().scalar_str(text)?;
    ctx.protect(text);
    set_attr(ctx, value, key, text)
}

fn char_text(ctx: &Ctx, id: CharId) -> EvalResult<Option<Cow<'_, str>>> {
    if id == CharId::NA {
        return Ok(None);
    }
    let s = ctx.session();
    let bytes = s.char_bytes(id)?;
    Ok(Some(if s.char_encoding(id)? == Encoding::Latin1 {
        Cow::Owned(bytes.iter().copied().map(char::from).collect())
    } else {
        String::from_utf8_lossy(bytes)
    }))
}

fn strings(ctx: &Ctx, value: SexpId) -> EvalResult<Vec<Option<String>>> {
    ctx.session()
        .str_slice(value)?
        .iter()
        .map(|&id| char_text(ctx, id).map(|text| text.map(Cow::into_owned)))
        .collect()
}

fn first_text(ctx: &Ctx, value: SexpId) -> EvalResult<Option<String>> {
    match ctx.session().str_slice(value)?.first() {
        Some(&id) => char_text(ctx, id).map(|text| text.map(Cow::into_owned)),
        None => Ok(None),
    }
}

fn input(ctx: &mut Ctx, file: SexpId, encoding: SexpId) -> EvalResult<String> {
    ctx.scoped(|ctx| {
        ctx.protect(file);
        ctx.protect(encoding);
        let n = ctx.session_mut().scalar_int(-1)?;
        ctx.protect(n);
        let yes = ctx.session_mut().scalar_lgl(RLogical::TRUE)?;
        ctx.protect(yes);
        let no = ctx.session_mut().scalar_lgl(RLogical::FALSE)?;
        ctx.protect(no);
        let lines = ctx.call_function(
            "readLines",
            &[
                (None, file),
                (None, n),
                (None, yes),
                (None, no),
                (None, encoding),
            ],
            BASE_ENV,
        )?;
        ctx.protect(lines);
        let limit = ctx.host().limits().max_vector_bytes;
        let mut input = String::new();
        for &line in ctx.session().str_slice(lines)? {
            let line = char_text(ctx, line)?.unwrap_or_default();
            if line.len().saturating_add(1) > limit.saturating_sub(input.len()) {
                return Err(ctx.stop(SEXP_NIL, "Rd input size limit exceeded"));
            }
            input.push_str(&line);
            input.push('\n');
        }
        if input.is_empty() {
            input.push('\n');
        }
        Ok(input)
    })
}

fn parse_rd(ctx: &mut Ctx, call: &Call, args: &mut roxide_eval::Args) -> EvalResult {
    let file = args.get(0);
    let mut srcfile = args.get(1);
    let fragment = flag(ctx, call, args.get(3))?;
    let warning_calls = flag(ctx, call, args.get(4))?;
    let macros = args.get(5);
    let permissive = flag(ctx, call, args.get(6))?;
    let keep_empty = flag(ctx, call, args.get(7))?;
    let limit = ctx.host().limits().max_vector_bytes;
    let depth = ctx.host().limits().max_expressions.min(128);
    ctx.scoped(|ctx| {
        let path = ctx.session().type_of(file)? == SexpType::Str;
        let mut name = if path {
            first_text(ctx, file)?.unwrap_or_default()
        } else {
            "<connection>".to_owned()
        };
        if srcfile == SEXP_NIL {
            srcfile = ctx.session_mut().alloc_env(roxide_value::EMPTY_ENV)?;
            ctx.protect(srcfile);
            set_str(ctx, srcfile, "class", "srcfile")?;
            let encoding = first_text(ctx, args.get(2))?.unwrap_or_default();
            let encoding = if encoding == "unknown" { "" } else { &encoding };
            let cwd = ctx.host().cwd().ok();
            let wd = cwd.as_deref().and_then(Path::to_str).unwrap_or("");
            for (key, text) in [
                ("filename", name.as_str()),
                ("wd", wd),
                ("encoding", encoding),
                ("Enc", "UTF-8"),
            ] {
                let key = ctx.session_mut().intern(key)?;
                let text = ctx.session_mut().scalar_str(text)?;
                ctx.session_mut().env_define(srcfile, key, text)?;
            }
            let modified = if path {
                ctx.host()
                    .metadata(Path::new(&name), true)
                    .ok()
                    .and_then(|info| info.modified_us)
            } else {
                None
            };
            let timestamp = ctx
                .session_mut()
                .scalar_real(modified.map_or(NA_REAL, |us| us as f64 / 1e6))?;
            ctx.protect(timestamp);
            let class = chars(ctx, ["POSIXct", "POSIXt"])?;
            ctx.protect(class);
            set_attr(ctx, timestamp, "class", class)?;
            let key = ctx.session_mut().intern("timestamp")?;
            ctx.session_mut().env_define(srcfile, key, timestamp)?;
        } else if ctx.session().type_of(srcfile)? == SexpType::Env {
            let filename = ctx.session_mut().intern("filename")?;
            if let Some(binding) = ctx.session().env_get(srcfile, filename) {
                name = first_text(ctx, binding.value)?.unwrap_or(name);
            }
        }

        let mut parser = parser::Parser::new(limit, depth, permissive, keep_empty);
        let (keep_macros, mut parent) = tree::read_macros(ctx, macros, &mut parser)?;
        let encoding = args.get(2);
        if ctx.session().type_of(macros)? == SexpType::Str {
            for path in strings(ctx, macros)?.into_iter().flatten() {
                let path = ctx.session_mut().scalar_str(&path)?;
                let definitions = input(ctx, path, encoding)?;
                let definitions = parser
                    .parse(&definitions, false)
                    .map_err(|e| ctx.stop(call.err_call(), e))?;
                let imported = tree::build(ctx, &definitions, srcfile, true, parent)?;
                ctx.protect(imported);
                parent = attr(ctx, imported, "macros")?;
                ctx.protect(parent);
            }
        }
        let source = input(ctx, file, encoding)?;
        let document = parser
            .parse(&source, fragment)
            .map_err(|e| ctx.stop(call.err_call(), e))?;
        for (line, message) in &parser.warnings {
            ctx.warn_now(
                if warning_calls {
                    call.err_call()
                } else {
                    SEXP_NIL
                },
                format!("{name}:{line}: {message}"),
            )?;
        }
        tree::build(ctx, &document, srcfile, keep_macros, parent)
    })
}

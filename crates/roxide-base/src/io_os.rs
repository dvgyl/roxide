#![forbid(unsafe_code)]

//! Filesystem functions, OS helpers, and I/O connections.

mod binary;
mod codec;
pub(crate) mod connection;
mod filesystem;
mod lines;
mod system;

use roxide_eval::{
    ArgEval, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, NA_INTEGER, SexpType, ValueApi};

/// Base I/O, connection, filesystem, and OS functions.
pub const MODULE: Module = Module {
    prims: &[
        internal("rox_io_file", connection::file, 6, Visibility::On),
        internal("rox_io_text", connection::text, 4, Visibility::On),
        internal("rox_io_raw", connection::raw, 2, Visibility::On),
        internal("rox_io_raw_value", connection::raw_value, 1, Visibility::On),
        internal(
            "rox_io_text_value",
            connection::text_value,
            1,
            Visibility::On,
        ),
        internal("rox_io_close", connection::close, 1, Visibility::Off),
        internal("rox_io_is_open", connection::is_open, 2, Visibility::On),
        internal("rox_io_std", connection::standard, 1, Visibility::On),
        internal("rox_io_read_lines", lines::read_lines, 6, Visibility::On),
        internal("rox_io_write_lines", lines::write_lines, 4, Visibility::Off),
        internal("rox_io_read_char", lines::read_char, 3, Visibility::On),
        internal(
            "rox_io_write_char",
            lines::write_char,
            5,
            Visibility::SetByFn,
        ),
        internal("rox_io_read_bin", binary::read_bin, 6, Visibility::On),
        internal(
            "rox_io_write_bin",
            binary::write_bin,
            5,
            Visibility::SetByFn,
        ),
        internal("rox_io_tempfile", filesystem::tempfile, 3, Visibility::On),
        internal("rox_io_tempdir", filesystem::tempdir, 1, Visibility::On),
        internal(
            "rox_io_dir_create",
            filesystem::dir_create,
            4,
            Visibility::Off,
        ),
        internal(
            "rox_io_file_create",
            filesystem::file_create,
            2,
            Visibility::On,
        ),
        internal("rox_io_unlink", filesystem::unlink, 3, Visibility::Off),
        internal("rox_io_exists", filesystem::exists, 2, Visibility::On),
        internal("rox_io_file_info", filesystem::file_info, 2, Visibility::On),
        internal("rox_io_rename", filesystem::rename, 2, Visibility::On),
        internal(
            "rox_io_list_files",
            filesystem::list_files,
            9,
            Visibility::On,
        ),
        internal("rox_io_file_path", filesystem::file_path, 2, Visibility::On),
        internal(
            "rox_io_path_parts",
            filesystem::path_parts,
            2,
            Visibility::On,
        ),
        internal("rox_io_normalize", filesystem::normalize, 3, Visibility::On),
        internal("rox_io_getwd", filesystem::getwd, 0, Visibility::On),
        internal("rox_io_setwd", filesystem::setwd, 1, Visibility::Off),
        internal("rox_io_getenv", system::getenv, 3, Visibility::On),
        internal("rox_io_setenv", system::setenv, 1, Visibility::Off),
        internal("rox_io_unsetenv", system::unsetenv, 1, Visibility::Off),
        internal("rox_io_system2", system::system2, 10, Visibility::On),
        internal("rox_io_versions", system::versions, 0, Visibility::On),
    ],
    prelude: r#"
file <- function(description = "", open = "", blocking = TRUE, encoding = getOption("encoding"),
                 raw = FALSE, method = getOption("url.method", "default"))
    .Internal(rox_io_file(description, open, "file", 0L, raw, encoding))
gzfile <- function(description, open = "", encoding = getOption("encoding"), compression = 6)
    .Internal(rox_io_file(description, open, "gzfile", compression, FALSE, encoding))
bzfile <- function(description, open = "", encoding = getOption("encoding"), compression = 9)
    .Internal(rox_io_file(description, open, "bzfile", compression, FALSE, encoding))
xzfile <- function(description, open = "", encoding = getOption("encoding"), compression = 6)
    .Internal(rox_io_file(description, open, "xzfile", compression, FALSE, encoding))
zstdfile <- function(description, open = "", encoding = getOption("encoding"), compression = 9)
    .Internal(rox_io_file(description, open, "zstdfile", compression, FALSE, encoding))
rawConnection <- function(object, open = "r") .Internal(rox_io_raw(object, open))
rawConnectionValue <- function(con) .Internal(rox_io_raw_value(con))
textConnection <- function(object, open = "r", local = FALSE, name = deparse1(substitute(object)),
                           encoding = c("", "bytes", "UTF-8"))
    .Internal(rox_io_text(object, open, if (local) parent.frame() else globalenv(), encoding))
textConnectionValue <- function(con) .Internal(rox_io_text_value(con))
close <- function(con, ...) UseMethod("close")
close.connection <- function(con, ...) .Internal(rox_io_close(con))
isOpen <- function(con, rw = "") .Internal(rox_io_is_open(con, rw))
stdin <- function() .Internal(rox_io_std(0L))
stdout <- function() .Internal(rox_io_std(1L))
stderr <- function() .Internal(rox_io_std(2L))
readLines <- function(con = stdin(), n = -1L, ok = TRUE, warn = TRUE, encoding = "unknown",
                      skipNul = FALSE)
    .Internal(rox_io_read_lines(con, n, ok, warn, encoding, skipNul))
writeLines <- function(text, con = stdout(), sep = "\n", useBytes = FALSE)
    .Internal(rox_io_write_lines(text, con, sep, useBytes))
readBin <- function(con, what, n = 1L, size = NA_integer_, signed = TRUE, endian = "platform")
    .Internal(rox_io_read_bin(con, what, n, size, signed, endian))
writeBin <- function(object, con, size = NA_integer_, endian = "platform", useBytes = FALSE)
    .Internal(rox_io_write_bin(object, con, size, endian, useBytes))
readChar <- function(con, nchars, useBytes = FALSE)
    .Internal(rox_io_read_char(con, nchars, useBytes))
writeChar <- function(object, con, nchars = NULL, eos = "", useBytes = FALSE)
    .Internal(rox_io_write_char(object, con, nchars, eos, useBytes))

tempfile <- function(pattern = "file", tmpdir = tempdir(), fileext = "")
    .Internal(rox_io_tempfile(pattern, tmpdir, fileext))
tempdir <- function(check = FALSE) .Internal(rox_io_tempdir(check))
dir.create <- function(path, showWarnings = TRUE, recursive = FALSE, mode = "0777")
    .Internal(rox_io_dir_create(path, showWarnings, recursive, mode))
file.create <- function(..., showWarnings = TRUE)
    .Internal(rox_io_file_create(c(...), showWarnings))
unlink <- function(x, recursive = FALSE, force = FALSE, expand = TRUE)
    .Internal(rox_io_unlink(x, recursive, expand))
file.exists <- function(...) .Internal(rox_io_exists(c(...), FALSE))
dir.exists <- function(paths) .Internal(rox_io_exists(paths, TRUE))
file.info <- function(..., extra_cols = TRUE) .Internal(rox_io_file_info(c(...), extra_cols))
file.rename <- function(from, to) .Internal(rox_io_rename(from, to))
list.files <- function(path = ".", pattern = NULL, all.files = FALSE, full.names = FALSE,
                       recursive = FALSE, ignore.case = FALSE, include.dirs = FALSE,
                       no.. = FALSE, fixed = FALSE)
    .Internal(rox_io_list_files(path, pattern, all.files, full.names, recursive, ignore.case,
                               include.dirs, no.., fixed))
dir <- list.files
file.path <- function(..., fsep = "/") .Internal(rox_io_file_path(list(...), fsep))
basename <- function(path) .Internal(rox_io_path_parts(path, FALSE))
dirname <- function(path) .Internal(rox_io_path_parts(path, TRUE))
normalizePath <- function(path, winslash = "/", mustWork = NA)
    .Internal(rox_io_normalize(path, winslash, mustWork))
getwd <- function() .Internal(rox_io_getwd())
setwd <- function(dir) .Internal(rox_io_setwd(dir))

Sys.getenv <- function(x = NULL, unset = "", names = NA)
    .Internal(rox_io_getenv(x, unset, names))
Sys.setenv <- function(...) .Internal(rox_io_setenv(list(...)))
Sys.unsetenv <- function(x) .Internal(rox_io_unsetenv(x))
system2 <- function(command, args = character(), stdout = "", stderr = "", stdin = "",
                    input = NULL, env = character(), wait = TRUE, minimized = FALSE,
                    invisible = TRUE, timeout = 0, receive.console.signals = wait)
    .Internal(rox_io_system2(command, args, stdout, stderr, stdin, input, env, wait,
                             timeout, receive.console.signals))
extSoftVersion <- function() .Internal(rox_io_versions())
"#,
};

const fn internal(
    name: &'static str,
    fun: PrimFn,
    count: usize,
    visibility: Visibility,
) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(count),
        dispatch: Dispatch::None,
        visibility,
    }
}

fn attr(ctx: &mut Ctx, value: SexpId, key: &str, attr: SexpId) -> EvalResult<()> {
    let key = ctx.session_mut().intern(key)?;
    Ok(ctx.session_mut().set_attrib(value, key, attr)?)
}

fn strings<B: AsRef<[u8]>>(
    ctx: &mut Ctx,
    values: impl IntoIterator<Item = Option<B>>,
    encoding: Encoding,
) -> EvalResult {
    let ids = values
        .into_iter()
        .map(|s| match s {
            Some(s) => ctx.session_mut().mk_char(s.as_ref(), encoding),
            None => Ok(CharId::NA),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ctx.session_mut().alloc_str_from(&ids)?)
}

fn string(ctx: &mut Ctx, value: impl AsRef<[u8]>) -> EvalResult {
    strings(ctx, [Some(value)], Encoding::Native)
}

#[derive(Clone, Copy)]
enum ByteEncoding {
    Bytes,
    Utf8,
    Ascii,
}

impl ByteEncoding {
    fn native(ctx: &mut Ctx) -> Self {
        if ctx.host().locale().utf8 {
            Self::Utf8
        } else {
            Self::Ascii
        }
    }
}

fn bytes(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<Vec<Option<Vec<u8>>>> {
    translated_bytes(ctx, call, value, ByteEncoding::Bytes)
}

fn translated_bytes(
    ctx: &mut Ctx,
    call: &Call,
    value: SexpId,
    encoding: ByteEncoding,
) -> EvalResult<Vec<Option<Vec<u8>>>> {
    let ids = ctx.session().str_slice(value)?;
    let mut size = ids.len().saturating_mul(size_of::<Option<Vec<u8>>>());
    for &id in ids {
        if id != CharId::NA {
            let data = ctx.session().char_bytes(id)?;
            let mark = ctx.session().char_encoding(id)?;
            let len = match (encoding, mark) {
                (ByteEncoding::Utf8, Encoding::Latin1) => data
                    .len()
                    .saturating_add(data.iter().filter(|b| !b.is_ascii()).count()),
                (ByteEncoding::Ascii, Encoding::Latin1) => data.len().saturating_add(
                    data.iter()
                        .filter(|b| !b.is_ascii())
                        .count()
                        .saturating_mul(3),
                ),
                (ByteEncoding::Ascii, Encoding::Utf8) => std::str::from_utf8(data)
                    .map_or(data.len(), |s| s.chars().map(ascii_width).sum()),
                _ => data.len(),
            };
            size = size.saturating_add(len);
        }
    }
    check_limit(ctx, call, size)?;
    ids_to_bytes(ctx, value, encoding)
}

fn ascii_width(ch: char) -> usize {
    match u32::from(ch) {
        0..=0x7f => 1,
        0x80..=0xffff => 8,
        _ => 12,
    }
}

fn ids_to_bytes(
    ctx: &Ctx,
    value: SexpId,
    encoding: ByteEncoding,
) -> EvalResult<Vec<Option<Vec<u8>>>> {
    let session = ctx.session();
    session
        .str_slice(value)?
        .iter()
        .map(|&id| {
            if id == CharId::NA {
                return Ok(None);
            }
            let data = session.char_bytes(id)?;
            let value = match (encoding, session.char_encoding(id)?) {
                (ByteEncoding::Utf8, Encoding::Latin1) => data
                    .iter()
                    .map(|&b| char::from(b))
                    .collect::<String>()
                    .into_bytes(),
                (ByteEncoding::Ascii, Encoding::Latin1) => {
                    let mut result = Vec::new();
                    for &byte in data {
                        if byte.is_ascii() {
                            result.push(byte);
                        } else {
                            result.extend_from_slice(format!("<{byte:02x}>").as_bytes());
                        }
                    }
                    result
                }
                (ByteEncoding::Ascii, Encoding::Utf8) => match std::str::from_utf8(data) {
                    Ok(text) => {
                        let mut result = Vec::new();
                        for ch in text.chars() {
                            match ascii_width(ch) {
                                1 => result.push(ch as u8),
                                8 => result
                                    .extend_from_slice(format!("<U+{:04X}>", ch as u32).as_bytes()),
                                _ => result
                                    .extend_from_slice(format!("<U+{:08X}>", ch as u32).as_bytes()),
                            }
                        }
                        result
                    }
                    Err(_) => data.to_vec(),
                },
                _ => data.to_vec(),
            };
            Ok(Some(value))
        })
        .collect()
}

fn texts(ctx: &mut Ctx, call: &Call, value: SexpId, arg: &str) -> EvalResult<Vec<Option<String>>> {
    if ctx.session().type_of(value)? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), format!("invalid '{arg}' argument")));
    }
    bytes(ctx, call, value).map(|values| {
        values
            .into_iter()
            .map(|s| s.map(|s| String::from_utf8_lossy(&s).into_owned()))
            .collect()
    })
}

fn text(ctx: &mut Ctx, call: &Call, value: SexpId, arg: &str) -> EvalResult<String> {
    match texts(ctx, call, value, arg)?.as_mut_slice() {
        [Some(s)] => Ok(std::mem::take(s)),
        _ => Err(ctx.stop(call.err_call(), format!("invalid '{arg}' argument"))),
    }
}

fn integers(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<i32>> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Int)?;
    Ok(ctx.session().int(value)?.iter().collect())
}

fn integer(ctx: &mut Ctx, value: SexpId) -> EvalResult<i32> {
    Ok(integers(ctx, value)?.first().copied().unwrap_or(NA_INTEGER))
}

fn logical(ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<bool>> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
    Ok(ctx
        .session()
        .lgl(value)?
        .iter()
        .next()
        .and_then(|x| x.get()))
}

fn flag(ctx: &mut Ctx, call: &Call, value: SexpId, arg: &str) -> EvalResult<bool> {
    logical(ctx, value)?
        .ok_or_else(|| ctx.stop(call.err_call(), format!("invalid '{arg}' argument")))
}

fn io_result<T>(ctx: &mut Ctx, call: &Call, result: std::io::Result<T>) -> EvalResult<T> {
    result.map_err(|e| ctx.stop(call.err_call(), e.to_string()))
}

fn check_limit(ctx: &mut Ctx, call: &Call, size: usize) -> EvalResult<()> {
    if size > ctx.host().limits().max_vector_bytes {
        Err(ctx.stop(call.err_call(), "I/O result exceeds the vector size limit"))
    } else {
        Ok(())
    }
}

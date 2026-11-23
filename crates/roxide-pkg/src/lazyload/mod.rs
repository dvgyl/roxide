//! Read and write lazy-load databases for installed packages.

mod read;
mod write;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use roxide_eval::{Args, Call, Ctx, EvalResult, Module, PrimDef, PrimKind, Visibility};
use roxide_gc::SexpId;
use roxide_host::OpenMode;
use roxide_value::{BASE_ENV, EMPTY_ENV, SEXP_NIL, ValueApi};

use crate::loader::values::{define, field, internal, names, string};

pub use read::load_lazyload_db;
pub(crate) use read::load_namespace_db;
pub use write::write_lazyload_db;
pub(crate) use write::{DataCompression, write_data_db, write_namespace_db};

/// Base functions for lazy-load databases.
pub const MODULE: Module = Module {
    prims: &[
        PrimDef {
            kind: PrimKind::Primitive,
            ..internal("lazyLoadDBfetch", 4, fetch, Visibility::On)
        },
        PrimDef {
            kind: PrimKind::Primitive,
            ..internal("is.primitive", 1, is_primitive, Visibility::On)
        },
        internal("rox_lazy_load", 3, load, Visibility::On),
        internal("rox_lazy_map", 2, map, Visibility::On),
        internal("rox_lazy_fetch", 2, read::force_record, Visibility::On),
        internal(
            "rox_lazy_reference",
            2,
            read::force_reference,
            Visibility::On,
        ),
    ],
    prelude: r#"
lazyLoad <- function(filebase, envir = parent.frame(), filter)
    .Internal(rox_lazy_load(filebase, envir, if (missing(filter)) NULL else filter))
lazyLoadDBexec <- function(filebase, fun, filter) {
    state <- .Internal(rox_lazy_map(filebase, if (missing(filter)) NULL else filter))
    datafile <- paste0(filebase, ".rdb")
    mapfile <- paste0(filebase, ".rdx")
    map <- state$map
    vals <- state$vals
    vars <- names(vals)
    compressed <- map$compressed
    envenv <- state$cache
    env <- new.env(parent = baseenv())
    envhook <- function(key) .Internal(rox_lazy_reference(key, state))
    glue <- function(key) lazyLoadDBfetch(key, datafile, compressed, envhook)
    fun(environment())
}
"#,
};

fn is_primitive(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let kind = ctx.session().type_of(args.get(0))?;
    Ok(
        if matches!(
            kind,
            roxide_value::SexpType::Builtin | roxide_value::SexpType::Special
        ) {
            roxide_value::TRUE_SCALAR
        } else {
            roxide_value::FALSE_SCALAR
        },
    )
}

fn fetch(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let filename = string(ctx, args.get(1))?;
    let filename = Path::new(&filename);
    let appended = suffix(filename, ".rdb");
    let file = if ctx
        .host()
        .metadata(filename, true)
        .is_ok_and(|meta| meta.is_file)
    {
        filename
    } else {
        &appended
    };
    let key = read::Key::parse(ctx, args.get(0))?;
    let mode = read::compression(ctx, args.get(2))?;
    read::fetch_with_callback(ctx, file, key, mode, args.get(3), call)
}

fn load(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let filebase = string(ctx, args.get(0))?;
    read::load_filtered(ctx, Path::new(&filebase), args.get(1), args.get(2))?;
    Ok(SEXP_NIL)
}

fn map(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let filebase = string(ctx, args.get(0))?;
    let (state, vars) = read::open(ctx, Path::new(&filebase))?;
    ctx.protect(state);
    let vars = read::filter(ctx, vars, args.get(1))?;
    let keys = write::named_list(ctx, &vars)?;
    define(ctx, state, "vals", keys)?;
    Ok(state)
}

const S3_TABLE: &str = ".__S3MethodsTable__.";

fn namespace_binding(name: &str) -> bool {
    name != ".__NAMESPACE__."
}

fn suffix(base: &Path, extension: &str) -> PathBuf {
    let mut path = base.as_os_str().to_owned();
    path.push(extension);
    path.into()
}

fn required(ctx: &mut Ctx, value: SexpId, key: &str) -> EvalResult {
    field(ctx, value, key)?
        .ok_or_else(|| ctx.stop(SEXP_NIL, format!("missing lazy-load field '{key}'")))
}

fn named_entries(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<(String, SexpId)>> {
    let values = ctx.session().list_slice(value)?.to_vec();
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let labels = crate::loader::values::attr(ctx, value, "names")?;
    let labels = names(ctx, labels)?;
    if labels.len() != values.len() {
        return Err(ctx.stop(SEXP_NIL, "invalid lazy-load names"));
    }
    let mut seen = std::collections::HashSet::new();
    labels
        .into_iter()
        .zip(values)
        .map(|(name, value)| {
            let Some(name) = name.filter(|name| !name.is_empty() && seen.insert(name.clone()))
            else {
                return Err(ctx.stop(SEXP_NIL, "invalid or duplicate lazy-load name"));
            };
            Ok((name, value))
        })
        .collect()
}

fn limited_read(ctx: &mut Ctx, reader: impl Read, limit: usize) -> EvalResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
    if bytes.len() > limit {
        return Err(ctx.stop(SEXP_NIL, "lazy-load record exceeds the byte limit"));
    }
    Ok(bytes)
}

pub(crate) fn cache_rds(ctx: &mut Ctx, path: &Path, value: SexpId) -> EvalResult<()> {
    let bytes = roxide_base::serialize::serialize_bytes(ctx, value, 3, None)?;
    let file = ctx
        .host()
        .open(path, OpenMode::write_truncate())
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
    let mut gzip = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    gzip.write_all(&bytes)
        .and_then(|()| gzip.finish().map(|_| ()))
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))
}

fn state_value(ctx: &mut Ctx, state: SexpId, name: &str) -> EvalResult {
    let symbol = ctx.session_mut().intern(name)?;
    ctx.session()
        .env_get(state, symbol)
        .map(|binding| binding.value)
        .ok_or_else(|| ctx.stop(SEXP_NIL, "invalid lazy-load state"))
}

fn state_env(ctx: &mut Ctx, map: SexpId, file: &Path) -> EvalResult {
    let state = ctx.session_mut().alloc_env(EMPTY_ENV)?;
    ctx.protect(state);
    let cache = ctx.session_mut().alloc_env(EMPTY_ENV)?;
    define(ctx, state, "cache", cache)?;
    define(ctx, state, "map", map)?;
    let file = ctx.session_mut().scalar_str(&file.to_string_lossy())?;
    define(ctx, state, "file", file)?;
    Ok(state)
}

fn promise(ctx: &mut Ctx, state: SexpId, key: SexpId) -> EvalResult {
    let fun = ctx.session_mut().intern("rox_lazy_fetch")?.as_sexp();
    let internal = ctx.session_mut().intern(".Internal")?.as_sexp();
    let args = ctx.session_mut().cons(state, SEXP_NIL, SEXP_NIL)?;
    let args = ctx.session_mut().cons(key, args, SEXP_NIL)?;
    let fetch = ctx.session_mut().lcons(fun, args, SEXP_NIL)?;
    let arg = ctx.session_mut().cons(fetch, SEXP_NIL, SEXP_NIL)?;
    let expr = ctx.session_mut().lcons(internal, arg, SEXP_NIL)?;
    Ok(ctx.session_mut().alloc_promise(expr, BASE_ENV)?)
}

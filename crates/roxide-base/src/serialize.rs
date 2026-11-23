#![forbid(unsafe_code)]

//! XDR v2 and v3 serialization for raw vectors, RDS files, and R workspaces.

mod files;
mod read;
mod write;

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::{fmt, io};

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind,
    Visibility,
};
use roxide_gc::SexpId;
use roxide_host::Limits;
use roxide_value::{
    BASE_ENV, CharId, PrimId, RLogical, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, ValueApi,
    ValueError,
};

use crate::io_os::connection::{self, Serialization};

use files::Compression;

const REF: u32 = 255;
const NIL: u32 = 254;
const GLOBAL: u32 = 253;
const UNBOUND: u32 = 252;
const MISSING: u32 = 251;
const BASE_NS: u32 = 250;
const NAMESPACE: u32 = 249;
const PACKAGE: u32 = 248;
const EMPTY: u32 = 242;
const BASE: u32 = 241;
const ALTREP: u32 = 238;

const OBJECT: u32 = 1 << 8;
const ATTR: u32 = 1 << 9;
const TAG: u32 = 1 << 10;
const S4: u32 = 1 << 16;
const ASCII: u32 = 1 << 18;
const UTF8: u32 = 1 << 15;
const LATIN1: u32 = 1 << 14;
const BYTES: u32 = 1 << 13;
const LOCKED_BINDING: u32 = 1 << 26;
const ACTIVE_BINDING: u32 = 1 << 27;
const MAX_INLINE_REF: u32 = 0x007f_ffff;

struct CodecError(String);

impl From<ValueError> for CodecError {
    fn from(error: ValueError) -> Self {
        Self(error.to_string())
    }
}

impl From<std::io::Error> for CodecError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

type WireResult<T> = Result<T, CodecError>;

fn invalid(message: impl fmt::Display) -> CodecError {
    CodecError(message.to_string())
}

/// Primitives and R wrappers for serialization and workspace I/O.
pub const MODULE: Module = Module {
    prims: &[
        internal("rox_serialize", serialize, 6, Visibility::On),
        internal("rox_unserialize", unserialize, 2, Visibility::On),
        internal("rox_save_rds", save_rds, 7, Visibility::Off),
        internal("rox_read_rds", read_rds, 2, Visibility::On),
        internal("rox_save", save, 7, Visibility::Off),
        internal("rox_load", load, 3, Visibility::Off),
    ],
    prelude: r#"
serialize <- function(object, connection, ascii = FALSE, xdr = TRUE, version = NULL, refhook = NULL)
    .Internal(rox_serialize(object, connection, ascii, xdr, version, refhook))
unserialize <- function(connection, refhook = NULL)
    .Internal(rox_unserialize(connection, refhook))
saveRDS <- function(object, file = "", ascii = FALSE, version = NULL, compress = TRUE, refhook = NULL)
    .Internal(rox_save_rds(object, file, ascii, version,
                          if (inherits(file, "connection")) NULL else compress,
                          refhook, !missing(compress)))
readRDS <- function(file, refhook = NULL)
    .Internal(rox_read_rds(file, refhook))
save <- function(..., list = character(), file = stop("'file' must be specified"),
                 ascii = FALSE, version = NULL, envir = parent.frame(), compress = !ascii,
                 compression_level = NULL, eval.promises = TRUE, precheck = TRUE)
    .Internal(rox_save(substitute(list(...)), list, file, ascii, version, envir,
                       if (inherits(file, "connection")) list(NULL, NULL, eval.promises)
                       else list(compress, compression_level, eval.promises)))
load <- function(file, envir = parent.frame(), verbose = FALSE)
    .Internal(rox_load(file, envir, verbose))
"#,
};

const fn internal(name: &'static str, fun: PrimFn, n: usize, visibility: Visibility) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(n),
        dispatch: Dispatch::None,
        visibility,
    }
}

fn checked<T>(ctx: &mut Ctx, call: &Call, result: WireResult<T>) -> EvalResult<T> {
    result.map_err(|error| ctx.stop(call.err_call(), error.0))
}

fn limits(ctx: &mut Ctx) -> Limits {
    let expressions = ctx.opts().expressions;
    let mut limits = ctx.host().limits().clone();
    // Pairlist tails use a loop. Bound native stack use for nested objects.
    limits.max_expressions = limits.max_expressions.min(expressions).min(128);
    limits
}

fn flag(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<bool> {
    let result = (|| {
        let (x, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
        let view = ctx.session().lgl(x)?;
        match (view.len() == 1).then(|| view.get(0)) {
            Some(RLogical::TRUE) => Ok(true),
            Some(RLogical::FALSE) => Ok(false),
            _ => Err(invalid("invalid logical argument")),
        }
    })();
    checked(ctx, call, result)
}

fn version(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<u32> {
    if value == SEXP_NIL {
        return Ok(3);
    }
    let result = (|| {
        let (x, _) = ctx.session_mut().coerce(value, SexpType::Int)?;
        let view = ctx.session().int(x)?;
        match (view.len() == 1).then(|| view.get(0)) {
            Some(v @ (2 | 3)) => Ok(v as u32),
            _ => Err(invalid("unsupported serialization version")),
        }
    })();
    checked(ctx, call, result)
}

fn validate_format(ctx: &mut Ctx, call: &Call, ascii: SexpId) -> EvalResult<()> {
    if flag(ctx, call, ascii)? {
        return Err(ctx.stop(call.err_call(), "ASCII serialization is unavailable"));
    }
    Ok(())
}

fn path(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<PathBuf> {
    let result = (|| {
        if let [ch] = ctx.session().str_slice(value)?
            && *ch != CharId::NA
            && let Ok(s) = std::str::from_utf8(ctx.session().char_bytes(*ch)?)
            && !s.is_empty()
        {
            return Ok(PathBuf::from(s));
        }
        Err(invalid("'file' must be a non-empty path"))
    })();
    checked(ctx, call, result)
}

fn compression(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<Compression> {
    if ctx.session().type_of(value)? == SexpType::Str {
        let result = (|| match ctx.session().str_slice(value)? {
            [x] => Compression::from_name(ctx.session().char_bytes(*x)?),
            _ => Err(invalid("invalid 'compress' argument")),
        })();
        return checked(ctx, call, result);
    }
    if flag(ctx, call, value)? {
        Ok(Compression::Gzip)
    } else {
        Ok(Compression::None)
    }
}

/// Persistent reference hook for lazy-load `.rdb`/`.rdx` `PERSISTSXP` encoding/decoding (ADR-0014).
pub trait PersistHook {
    /// Return `Some(STRSXP)` persistent key to emit `PERSISTSXP`, or `None` for normal encoding.
    fn write(&mut self, ctx: &mut Ctx, x: SexpId) -> EvalResult<Option<SexpId>>;
    /// Resolve a `PERSISTSXP` key (`STRSXP`) back into a live R object.
    fn read(&mut self, ctx: &mut Ctx, key: SexpId) -> EvalResult<SexpId>;
}

struct RHook {
    function: SexpId,
    call: Call,
}

impl PersistHook for RHook {
    fn write(&mut self, ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<SexpId>> {
        let key = self.read(ctx, value)?;
        if key == SEXP_NIL {
            return Ok(None);
        }
        if ctx
            .session()
            .str_slice(key)
            .is_ok_and(|chars| !chars.is_empty())
        {
            return Ok(Some(key));
        }
        Err(ctx.stop(
            self.call.err_call(),
            "reference hook must return a non-empty character vector",
        ))
    }

    fn read(&mut self, ctx: &mut Ctx, key: SexpId) -> EvalResult<SexpId> {
        ctx.apply(
            self.function,
            &[(None, key)],
            self.call.err_call(),
            self.call.env,
        )
    }
}

fn encode_object(
    ctx: &mut Ctx,
    call: &Call,
    value: SexpId,
    version: u32,
    hook: SexpId,
) -> EvalResult<Vec<u8>> {
    if hook == SEXP_NIL {
        return encode(ctx, call, value, version, false);
    }
    serialize_bytes(
        ctx,
        value,
        version,
        Some(&mut RHook {
            function: hook,
            call: *call,
        }),
    )
}

fn decode_object(ctx: &mut Ctx, call: &Call, bytes: &[u8], hook: SexpId) -> EvalResult {
    if hook == SEXP_NIL {
        return decode(ctx, call, bytes, false);
    }
    unserialize_bytes(
        ctx,
        bytes,
        Some(&mut RHook {
            function: hook,
            call: *call,
        }),
    )
}

/// Encode `x` into raw XDR bytes (v2 or v3), optionally using `hook` for persistent objects (ADR-0014).
pub fn serialize_bytes(
    ctx: &mut Ctx,
    x: SexpId,
    version: u32,
    hook: Option<&mut dyn PersistHook>,
) -> EvalResult<Vec<u8>> {
    let limits = limits(ctx);
    let registry = Registry::for_write(ctx).map_err(|error| ctx.stop(SEXP_NIL, error.0))?;
    if let Some(hook) = hook {
        return write::encode_hook(ctx, x, version, &limits, &registry, hook);
    }
    let result = write::encode(ctx.session_mut(), x, version, false, &limits, &registry);
    result.map_err(|error| ctx.stop(SEXP_NIL, error.0))
}

/// Decode raw XDR bytes into an R object, optionally resolving `PERSISTSXP` via `hook` (ADR-0014).
pub fn unserialize_bytes(
    ctx: &mut Ctx,
    bytes: &[u8],
    hook: Option<&mut dyn PersistHook>,
) -> EvalResult<SexpId> {
    let limits = limits(ctx);
    let registry = Registry::new(ctx);
    if let Some(hook) = hook {
        return read::decode_hook(ctx, bytes, &limits, &registry, hook);
    }
    let result = read::decode(ctx.session_mut(), bytes, false, &limits, &registry);
    result.map_err(|error| ctx.stop(SEXP_NIL, error.0))
}

struct Registry {
    packages: Vec<(String, SexpId, Option<SexpId>)>,
    primitives: HashMap<PrimId, &'static str>,
}

impl Registry {
    fn new(ctx: &Ctx) -> Self {
        Self {
            packages: ctx
                .package_environments()
                .map(|(name, ns, attached)| (name.to_owned(), ns, attached))
                .collect(),
            primitives: HashMap::new(),
        }
    }

    fn for_write(ctx: &Ctx) -> WireResult<Self> {
        let mut registry = Self::new(ctx);
        let session = ctx.session();
        for env in Self::envs(&registry.packages) {
            for (_, binding) in session.env_entries(env)? {
                if let Ok((id, _)) = session.prim_parts(binding.value)
                    && let Some(name) = ctx.primitive_name(binding.value)
                {
                    registry.primitives.insert(id, name);
                }
            }
        }
        Ok(registry)
    }

    fn envs<'a>(
        packages: &'a [(String, SexpId, Option<SexpId>)],
    ) -> impl Iterator<Item = SexpId> + 'a {
        std::iter::once(BASE_ENV).chain(packages.iter().map(|(_, ns, _)| *ns))
    }
}

fn encode(
    ctx: &mut Ctx,
    call: &Call,
    value: SexpId,
    version: u32,
    workspace: bool,
) -> EvalResult<Vec<u8>> {
    let limits = limits(ctx);
    let result = Registry::for_write(ctx).and_then(|registry| {
        write::encode(
            ctx.session_mut(),
            value,
            version,
            workspace,
            &limits,
            &registry,
        )
    });
    checked(ctx, call, result)
}

fn decode(ctx: &mut Ctx, call: &Call, bytes: &[u8], workspace: bool) -> EvalResult {
    let limits = limits(ctx);
    let registry = Registry::new(ctx);
    let result = read::decode(ctx.session_mut(), bytes, workspace, &limits, &registry);
    checked(ctx, call, result)
}

fn serialize(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    validate_format(ctx, call, args.get(2))?;
    if !flag(ctx, call, args.get(3))? {
        return Err(ctx.stop(
            call.err_call(),
            "native binary serialization is unavailable",
        ));
    }
    let dest = args.get(1);
    let version = version(ctx, call, args.get(4))?;
    let bytes = encode_object(ctx, call, args.get(0), version, args.get(5))?;
    if dest == SEXP_NIL {
        return Ok(ctx.session_mut().alloc_raw_from(&bytes)?);
    }
    connection::write_serialized(ctx, call, dest, Serialization::Stream, &bytes)?;
    Ok(SEXP_NIL)
}

fn unserialize(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let hook = args.get(1);
    let source = args.get(0);
    if connection::is_connection(ctx, source)? {
        return read_object_connection(ctx, call, source, Serialization::Stream, hook);
    }
    let limits = limits(ctx);
    let raw = ctx.session().raw(args.get(0));
    let bytes = match raw.as_ref().ok().and_then(|view| view.as_slice()) {
        Some(bytes) if bytes.len() <= limits.max_decompress_bytes.min(limits.max_vector_bytes) => {
            bytes.to_vec()
        }
        Some(_) => return Err(ctx.stop(call.err_call(), "serialized input exceeds the byte limit")),
        None => return Err(ctx.stop(call.err_call(), "unserialize() requires a raw vector")),
    };
    decode_object(ctx, call, &bytes, hook)
}

fn read_file(ctx: &mut Ctx, call: &Call, path: &Path, workspace: bool) -> EvalResult {
    let result = files::read(ctx.host(), path);
    let bytes = checked(ctx, call, result)?;
    decode(ctx, call, &bytes, workspace)
}

fn read_connection(
    ctx: &mut Ctx,
    call: &Call,
    source: SexpId,
    serialization: Serialization,
) -> EvalResult {
    let limits = limits(ctx);
    let registry = Registry::new(ctx);
    connection::read_serialized(ctx, call, source, serialization, |session, input| {
        read::decode_stream(
            session,
            input,
            serialization == Serialization::Workspace,
            &limits,
            &registry,
        )
        .map_err(|error| io::Error::other(error.0))
    })
}

fn read_object_connection(
    ctx: &mut Ctx,
    call: &Call,
    source: SexpId,
    serialization: Serialization,
    hook: SexpId,
) -> EvalResult {
    if hook == SEXP_NIL {
        return read_connection(ctx, call, source, serialization);
    }
    let limits = limits(ctx);
    let limit = limits.max_decompress_bytes.min(limits.max_vector_bytes);
    let bytes = connection::read_serialized(ctx, call, source, serialization, |_, input| {
        let mut bytes = Vec::new();
        input
            .take((limit as u64).saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(io::Error::other("serialized input exceeds the byte limit"));
        }
        Ok(bytes)
    })?;
    decode_object(ctx, call, &bytes, hook)
}

fn save_rds(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    validate_format(ctx, call, args.get(2))?;
    let dest = args.get(1);
    let version = version(ctx, call, args.get(3))?;
    if connection::is_connection(ctx, dest)? {
        if flag(ctx, call, args.get(6))? {
            ctx.warn(
                call.err_call(),
                "'compress' is ignored unless 'file' is a file name",
            );
        }
        let bytes = encode_object(ctx, call, args.get(0), version, args.get(5))?;
        connection::write_serialized(ctx, call, dest, Serialization::Rds, &bytes)?;
    } else {
        let path = path(ctx, call, dest)?;
        let compress = compression(ctx, call, args.get(4))?;
        let bytes = encode_object(ctx, call, args.get(0), version, args.get(5))?;
        let result = files::write(ctx.host(), &path, &bytes, compress, None);
        checked(ctx, call, result)?;
    }
    Ok(SEXP_NIL)
}

fn read_rds(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let hook = args.get(1);
    let source = args.get(0);
    if connection::is_connection(ctx, source)? {
        return read_object_connection(ctx, call, source, Serialization::Rds, hook);
    }
    let path = path(ctx, call, source)?;
    let result = files::read(ctx.host(), &path);
    let bytes = checked(ctx, call, result)?;
    decode_object(ctx, call, &bytes, hook)
}

fn compression_level(ctx: &mut Ctx, call: &Call, level: SexpId) -> EvalResult<Option<u32>> {
    if level == SEXP_NIL {
        return Ok(None);
    }
    let (x, _) = ctx.session_mut().coerce(level, SexpType::Int)?;
    let view = ctx.session().int(x)?;
    match (view.len() == 1).then(|| view.get(0)) {
        Some(n @ 0..=9) => Ok(Some(n as u32)),
        _ => Err(ctx.stop(call.err_call(), "invalid compression level")),
    }
}

fn save(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let dest = args.get(2);
    validate_format(ctx, call, args.get(3))?;
    let version = version(ctx, call, args.get(4))?;
    let env = args.get(5);
    if ctx.session().type_of(env)? != SexpType::Env {
        return Err(ctx.stop(call.err_call(), "invalid 'envir' argument"));
    }
    let options = args.get(6);
    let file = if connection::is_connection(ctx, dest)? {
        None
    } else {
        let path = path(ctx, call, dest)?;
        let compress = ctx.session().list_get(options, 0)?;
        let compress = compression(ctx, call, compress)?;
        let level = ctx.session().list_get(options, 1)?;
        let level = compression_level(ctx, call, level)?;
        Some((path, compress, level))
    };
    let force = ctx.session().list_get(options, 2)?;
    let force = flag(ctx, call, force)?;
    let result = save_names(ctx.session_mut(), args.get(0), args.get(1));
    let names = checked(ctx, call, result)?;

    let mut values = Vec::with_capacity(names.len());
    for name in names {
        let mut current = env;
        let binding = loop {
            if current == SEXP_NIL {
                let name = ctx.session().sym_name(name)?.to_owned();
                return Err(ctx.stop(call.err_call(), format!("object '{name}' not found")));
            }
            if let Some(binding) = ctx.session().env_get(current, name)
                && binding.value != SEXP_UNBOUND
            {
                break binding;
            }
            current = ctx.session().env_enclos(current)?;
        };
        let value = if binding.active {
            ctx.apply(binding.value, &[], call.err_call(), current)?
        } else if force && ctx.session().type_of(binding.value)? == SexpType::Prom {
            ctx.force(binding.value)?
        } else {
            binding.value
        };
        ctx.protect(value);
        values.push((name, value));
    }
    let mut frame = SEXP_NIL;
    for (name, value) in values.into_iter().rev() {
        frame = ctx.session_mut().cons(value, frame, name.as_sexp())?;
    }
    let bytes = encode(ctx, call, frame, version, true)?;
    if let Some((path, compress, level)) = file {
        let result = files::write(ctx.host(), &path, &bytes, compress, level);
        checked(ctx, call, result)?;
    } else {
        connection::write_serialized(ctx, call, dest, Serialization::Workspace, &bytes)?;
    }
    Ok(SEXP_NIL)
}

fn save_names(
    session: &mut roxide_value::RSession,
    dots: SexpId,
    list: SexpId,
) -> WireResult<Vec<roxide_value::Sym>> {
    let (_, tail, _) = session.cons_parts(dots)?;
    let mut names = Vec::new();
    for (_, value) in session.pairlist_iter(tail) {
        if matches!(value, SEXP_MISSING | SEXP_UNBOUND) {
            return Err(invalid("invalid object name in '...'"));
        }
        if let Ok(name) = session.sym_name_of(value) {
            names.push(name.to_owned());
        } else if let Ok([ch]) = session.str_slice(value)
            && *ch != CharId::NA
        {
            let name = std::str::from_utf8(session.char_bytes(*ch)?)
                .map_err(|_| invalid("invalid object name in '...'"))?;
            names.push(name.to_owned());
        } else {
            return Err(invalid("invalid object name in '...'"));
        }
    }
    let listed = if list == SEXP_NIL {
        &[]
    } else {
        session.str_slice(list)?
    };
    for &ch in listed {
        if ch == CharId::NA {
            return Err(invalid("invalid object name in 'list'"));
        }
        let name = std::str::from_utf8(session.char_bytes(ch)?)
            .map_err(|_| invalid("invalid object name in 'list'"))?;
        names.push(name.to_owned());
    }
    names
        .into_iter()
        .map(|name| Ok(session.intern(&name)?))
        .collect()
}

fn load(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let source = args.get(0);
    let env = args.get(1);
    if ctx.session().type_of(env)? != SexpType::Env {
        return Err(ctx.stop(call.err_call(), "invalid 'envir' argument"));
    }
    let verbose = flag(ctx, call, args.get(2))?;
    let mut frame = if connection::is_connection(ctx, source)? {
        read_connection(ctx, call, source, Serialization::Workspace)?
    } else {
        let path = path(ctx, call, source)?;
        read_file(ctx, call, &path, true)?
    };
    ctx.protect(frame);
    let mut bindings = Vec::new();
    while frame != SEXP_NIL {
        if ctx.session().type_of(frame)? != SexpType::List {
            return Err(ctx.stop(call.err_call(), "invalid workspace binding list"));
        }
        let (value, tail, tag) = ctx.session().cons_parts(frame)?;
        let Ok(name) = ctx.session().sym_name_of(tag).map(str::to_owned) else {
            return Err(ctx.stop(call.err_call(), "invalid workspace binding name"));
        };
        if name.is_empty() {
            return Err(ctx.stop(call.err_call(), "empty workspace binding name"));
        }
        let name = ctx.session_mut().intern(&name)?;
        bindings.push((name, value));
        frame = tail;
    }
    let mut names = Vec::with_capacity(bindings.len());
    if verbose {
        let result = ctx.host().write_out(b"Loading objects:\n").map_err(invalid);
        checked(ctx, call, result)?;
    }
    for (name, value) in bindings {
        ctx.assign(name, value, env)?;
        let text = ctx.session().sym_name(name)?.to_owned();
        if verbose {
            let result = ctx
                .host()
                .write_out(format!("  {text}\n").as_bytes())
                .map_err(invalid);
            checked(ctx, call, result)?;
        }
        let ch = ctx
            .session_mut()
            .mk_char(text.as_bytes(), roxide_value::Encoding::Utf8)?;
        ctx.protect(ch.as_sexp());
        names.push(ch);
    }
    Ok(ctx.session_mut().alloc_str_from(&names)?)
}

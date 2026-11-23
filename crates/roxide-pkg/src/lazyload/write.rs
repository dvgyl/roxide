use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use flate2::write::ZlibEncoder;
use roxide_base::serialize::{PersistHook, serialize_bytes};
use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_host::OpenMode;
use roxide_value::{FALSE_SCALAR, SEXP_NIL, SexpType, TRUE_SCALAR, ValueApi};

use super::{cache_rds, namespace_binding, suffix};
use crate::loader::values::{list, named};

#[derive(Clone, Copy, Default)]
pub(crate) enum DataCompression {
    None,
    #[default]
    Gzip,
    Bzip2,
    Xz,
}

impl DataCompression {
    fn index(self, ctx: &mut Ctx) -> EvalResult {
        match self {
            Self::None => Ok(FALSE_SCALAR),
            Self::Gzip => Ok(TRUE_SCALAR),
            Self::Bzip2 => Ok(ctx.session_mut().scalar_int(2)?),
            Self::Xz => Ok(ctx.session_mut().scalar_int(3)?),
        }
    }

    fn encode(self, bytes: Vec<u8>) -> std::io::Result<Vec<u8>> {
        let mut record = u32::try_from(bytes.len())
            .map_err(std::io::Error::other)?
            .to_be_bytes()
            .to_vec();
        match self {
            Self::None => Ok(bytes),
            Self::Gzip => {
                let mut encoder = ZlibEncoder::new(record, flate2::Compression::default());
                encoder.write_all(&bytes)?;
                encoder.finish()
            }
            Self::Bzip2 => {
                record.push(b'2');
                let mut encoder =
                    bzip2::write::BzEncoder::new(record, bzip2::Compression::default());
                encoder.write_all(&bytes)?;
                encoder.finish()
            }
            Self::Xz => {
                record.push(b'Z');
                let options = lzma_rust2::Lzma2Options::with_preset(0);
                let mut encoder = lzma_rust2::Lzma2Writer::new(record, options);
                encoder.write_all(&bytes)?;
                encoder.finish()
            }
        }
    }
}

/// Write a gzip index and a database of zlib-compressed XDR records.
pub fn write_lazyload_db(ctx: &mut Ctx, filebase: &Path, envir: SexpId) -> EvalResult<()> {
    write_data_db(ctx, filebase, envir, DataCompression::Gzip)
}

pub(crate) fn write_data_db(
    ctx: &mut Ctx,
    filebase: &Path,
    envir: SexpId,
    compression: DataCompression,
) -> EvalResult<()> {
    write_db(ctx, filebase, envir, None, compression)
}

pub(crate) fn write_namespace_db(
    ctx: &mut Ctx,
    filebase: &Path,
    envir: SexpId,
    sysdata: SexpId,
) -> EvalResult<()> {
    write_db(ctx, filebase, envir, Some(sysdata), DataCompression::Gzip)
}

fn write_db(
    ctx: &mut Ctx,
    filebase: &Path,
    envir: SexpId,
    sysdata: Option<SexpId>,
    compression: DataCompression,
) -> EvalResult<()> {
    ctx.scoped(|ctx| {
        ctx.protect(envir);
        let mut file = ctx
            .host()
            .open(&suffix(filebase, ".rdb"), OpenMode::write_truncate())
            .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
        let mut hook = EnvironmentQueue::default();
        let mut variables = Vec::new();
        let mut references = Vec::new();
        let mut offset = 0;
        for (sym, binding) in ctx.session().env_entries(envir)? {
            let name = ctx.session().sym_name(sym)?.to_owned();
            if let Some(sysdata) = sysdata
                && (!namespace_binding(&name)
                    || (!binding.active
                        && ctx
                            .session()
                            .env_get(sysdata, sym)
                            .is_some_and(|data| data.value == binding.value)))
            {
                continue;
            }
            let value = if binding.active {
                ctx.apply(binding.value, &[], SEXP_NIL, envir)?
            } else {
                binding.value
            };
            ctx.protect(value);
            let key = record(ctx, &mut file, value, &mut hook, &mut offset, compression)?;
            ctx.protect(key);
            variables.push((name, key));
        }
        while let Some(&env) = hook.pending.get(references.len()) {
            let desc = describe(ctx, env)?;
            ctx.protect(desc);
            let key = record(ctx, &mut file, desc, &mut hook, &mut offset, compression)?;
            ctx.protect(key);
            references.push((env_key(references.len()), key));
        }
        file.flush()
            .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;

        let variables = named_list(ctx, &variables)?;
        let references = named_list(ctx, &references)?;
        let compressed = compression.index(ctx)?;
        let index = list(ctx, &[variables, references, compressed])?;
        named(ctx, index, &["variables", "references", "compressed"])?;
        cache_rds(ctx, &suffix(filebase, ".rdx"), index)
    })
}

fn env_key(index: usize) -> String {
    format!("env::{}", index + 1)
}

#[derive(Default)]
struct EnvironmentQueue {
    seen: HashMap<SexpId, usize>,
    pending: Vec<SexpId>,
}

impl PersistHook for EnvironmentQueue {
    fn write(&mut self, ctx: &mut Ctx, env: SexpId) -> EvalResult<Option<SexpId>> {
        if ctx.session().type_of(env)? != SexpType::Env {
            return Ok(None);
        }
        let index = *self.seen.entry(env).or_insert_with(|| {
            let index = self.pending.len();
            self.pending.push(env);
            index
        });
        Ok(Some(ctx.session_mut().scalar_str(&env_key(index))?))
    }

    fn read(&mut self, ctx: &mut Ctx, _: SexpId) -> EvalResult {
        Err(ctx.stop(SEXP_NIL, "invalid lazy-load writer hook"))
    }
}

fn describe(ctx: &mut Ctx, env: SexpId) -> EvalResult {
    let mut bindings = Vec::new();
    for (sym, binding) in ctx.session().env_entries(env)? {
        let name = ctx.session().sym_name(sym)?.to_owned();
        let value = if binding.active {
            ctx.apply(binding.value, &[], SEXP_NIL, env)?
        } else {
            binding.value
        };
        ctx.protect(value);
        bindings.push((name, value));
    }
    let bindings = named_list(ctx, &bindings)?;
    let parent = ctx.session().env_enclos(env)?;
    let attributes = ctx.session().attrs(env)?;
    let attributes = if attributes.is_empty() {
        SEXP_NIL
    } else {
        let attributes = attributes
            .into_iter()
            .map(|(sym, value)| Ok((ctx.session().sym_name(sym)?.to_owned(), value)))
            .collect::<EvalResult<Vec<_>>>()?;
        named_list(ctx, &attributes)?
    };
    let s4 = if ctx.session().is_s4(env)? {
        TRUE_SCALAR
    } else {
        FALSE_SCALAR
    };
    let locked = if ctx.session().env_is_locked(env)? {
        TRUE_SCALAR
    } else {
        FALSE_SCALAR
    };
    let desc = list(ctx, &[bindings, parent, attributes, s4, locked])?;
    named(
        ctx,
        desc,
        &["bindings", "enclos", "attributes", "isS4", "locked"],
    )
}

pub(super) fn named_list(ctx: &mut Ctx, entries: &[(String, SexpId)]) -> EvalResult {
    let values = entries.iter().map(|(_, value)| *value).collect::<Vec<_>>();
    let labels = entries
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    let result = list(ctx, &values)?;
    if labels.is_empty() {
        Ok(result)
    } else {
        named(ctx, result, &labels)
    }
}

fn record(
    ctx: &mut Ctx,
    output: &mut impl Write,
    value: SexpId,
    hook: &mut EnvironmentQueue,
    offset: &mut i32,
    compression: DataCompression,
) -> EvalResult {
    let discovered = hook.pending.len();
    let bytes = serialize_bytes(ctx, value, 3, Some(hook))?;
    for &env in &hook.pending[discovered..] {
        ctx.protect(env);
    }
    let payload = compression
        .encode(bytes)
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
    let size = i32::try_from(payload.len())
        .map_err(|_| ctx.stop(SEXP_NIL, "lazy-load record exceeds the size limit"))?;
    let next = offset
        .checked_add(size)
        .ok_or_else(|| ctx.stop(SEXP_NIL, "lazy-load database exceeds the size limit"))?;
    output
        .write_all(&payload)
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
    let key = ctx.session_mut().alloc_int_from(&[*offset, size])?;
    *offset = next;
    Ok(key)
}

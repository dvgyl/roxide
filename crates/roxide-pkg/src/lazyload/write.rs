use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use flate2::{Compression, write::ZlibEncoder};
use roxide_base::serialize::{PersistHook, serialize_bytes};
use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_host::OpenMode;
use roxide_value::{FALSE_SCALAR, SEXP_NIL, TRUE_SCALAR, ValueApi};

use super::{cache_rds, namespace_binding, suffix};
use crate::loader::values::{list, named};

/// Write a gzip index and a database of zlib-compressed XDR records.
pub fn write_lazyload_db(ctx: &mut Ctx, filebase: &Path, envir: SexpId) -> EvalResult<()> {
    write_db(ctx, filebase, envir, false)
}

pub(crate) fn write_namespace_db(ctx: &mut Ctx, filebase: &Path, envir: SexpId) -> EvalResult<()> {
    write_db(ctx, filebase, envir, true)
}

fn write_db(ctx: &mut Ctx, filebase: &Path, envir: SexpId, namespace: bool) -> EvalResult<()> {
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
            if namespace && !namespace_binding(&name) {
                continue;
            }
            let value = if binding.active {
                ctx.apply(binding.value, &[], SEXP_NIL, envir)?
            } else {
                binding.value
            };
            ctx.protect(value);
            let key = record(ctx, &mut file, value, &mut hook, &mut offset)?;
            ctx.protect(key);
            variables.push((name, key));
        }
        while let Some(&env) = hook.pending.get(references.len()) {
            let desc = describe(ctx, env)?;
            ctx.protect(desc);
            let key = record(ctx, &mut file, desc, &mut hook, &mut offset)?;
            ctx.protect(key);
            references.push((env_key(references.len()), key));
        }
        file.flush()
            .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;

        let variables = named_list(ctx, &variables)?;
        let references = named_list(ctx, &references)?;
        let index = list(ctx, &[variables, references, TRUE_SCALAR])?;
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
) -> EvalResult {
    let bytes = serialize_bytes(ctx, value, 3, Some(hook))?;
    let len = i32::try_from(bytes.len())
        .map_err(|_| ctx.stop(SEXP_NIL, "lazy-load record exceeds the size limit"))?;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    let compressed = encoder
        .write_all(&bytes)
        .and_then(|()| encoder.finish())
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
    let size = i32::try_from(compressed.len())
        .ok()
        .and_then(|size| size.checked_add(4))
        .ok_or_else(|| ctx.stop(SEXP_NIL, "lazy-load record exceeds the size limit"))?;
    let next = offset
        .checked_add(size)
        .ok_or_else(|| ctx.stop(SEXP_NIL, "lazy-load database exceeds the size limit"))?;
    output
        .write_all(&len.to_be_bytes())
        .and_then(|()| output.write_all(&compressed))
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
    let key = ctx.session_mut().alloc_int_from(&[*offset, size])?;
    *offset = next;
    Ok(key)
}

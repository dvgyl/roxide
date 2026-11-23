use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use bzip2::read::BzDecoder;
use flate2::read::{MultiGzDecoder, ZlibDecoder};
use lzma_rust2::{Lzma2Reader, XzReader};
use roxide_base::serialize::{PersistHook, unserialize_bytes};
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_host::OpenMode;
use roxide_value::{EMPTY_ENV, SEXP_NIL, SexpType, ValueApi};

use super::{
    limited_read, named_entries, namespace_binding, promise, required, state_env, state_value,
    suffix,
};
use crate::loader::values::{base_call, define, flag, string, strings};

#[derive(Clone, Copy)]
pub(super) struct Key {
    offset: u64,
    size: usize,
}

impl Key {
    pub(super) fn parse(ctx: &mut Ctx, key: SexpId) -> EvalResult<Self> {
        let vector = ctx.session().int(key)?;
        if vector.len() == 2 && vector.get(0) >= 0 && vector.get(1) > 0 {
            Ok(Self {
                offset: vector.get(0) as u64,
                size: vector.get(1) as usize,
            })
        } else {
            Err(ctx.stop(SEXP_NIL, "invalid lazy-load record key"))
        }
    }
}

pub(super) fn compression(ctx: &mut Ctx, value: SexpId) -> EvalResult<u8> {
    if ctx.session().type_of(value)? == SexpType::Lgl && ctx.session().xlength(value)? == 1 {
        return Ok(u8::from(flag(ctx, value)?));
    }
    if ctx.session().type_of(value)? == SexpType::Int {
        let vector = ctx.session().int(value)?;
        if vector.len() == 1 && matches!(vector.get(0), 0..=3) {
            return Ok(vector.get(0) as u8);
        }
    }
    Err(ctx.stop(SEXP_NIL, "invalid lazy-load compression mode"))
}

fn read_record(ctx: &mut Ctx, file: &Path, key: Key, mode: u8) -> EvalResult<Vec<u8>> {
    let limits = ctx.host().limits();
    let limit = limits.max_decompress_bytes.min(limits.max_vector_bytes);
    if key.size > limit {
        return Err(ctx.stop(SEXP_NIL, "lazy-load record exceeds the byte limit"));
    }
    let meta = ctx
        .host()
        .metadata(file, true)
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
    if !meta.is_file || key.offset + key.size as u64 > meta.len {
        return Err(ctx.stop(SEXP_NIL, "lazy-load record lies outside the database"));
    }
    let mut file = ctx
        .host()
        .open(file, OpenMode::read_only())
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
    file.seek(SeekFrom::Start(key.offset))
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
    let mut input = file.take(key.size as u64);
    if mode == 0 {
        let bytes = limited_read(ctx, input, key.size)?;
        if bytes.len() != key.size {
            return Err(ctx.stop(SEXP_NIL, "truncated lazy-load record"));
        }
        return Ok(bytes);
    }
    let mut length = [0; 4];
    input
        .read_exact(&mut length)
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
    let length = u32::from_be_bytes(length) as usize;
    if length > limit {
        return Err(ctx.stop(SEXP_NIL, "lazy-load record exceeds the decompression limit"));
    }
    let marker = if mode == 1 {
        b'1'
    } else {
        let mut marker = [0];
        input
            .read_exact(&mut marker)
            .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
        marker[0]
    };
    let bytes = match (mode, marker) {
        (1..=3, b'1') => limited_read(ctx, ZlibDecoder::new(input), length)?,
        (2..=3, b'2') => limited_read(ctx, BzDecoder::new(input), length)?,
        (3, b'Z') => {
            let mut input = BufReader::new(input);
            let header = input
                .fill_buf()
                .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
            if header.starts_with(b"\xfd7zXZ\0") {
                limited_read(
                    ctx,
                    XzReader::new_mem_limit(input, false, 128 * 1024),
                    length,
                )?
            } else {
                let dictionary = (8 * 1024 * 1024).min(length.max(4096)) as u32;
                let decoder = Lzma2Reader::new_mem_limit(input, dictionary, 128 * 1024, None)
                    .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
                limited_read(ctx, decoder, length)?
            }
        }
        (2..=3, b'0') => limited_read(ctx, input, length)?,
        _ => return Err(ctx.stop(SEXP_NIL, "invalid lazy-load compression marker")),
    };
    if bytes.len() != length {
        return Err(ctx.stop(SEXP_NIL, "incorrect lazy-load record length"));
    }
    Ok(bytes)
}

pub(super) type Entries = Vec<(String, SexpId)>;

pub(super) fn open(ctx: &mut Ctx, filebase: &Path) -> EvalResult<(SexpId, Entries)> {
    let index = suffix(filebase, ".rdx");
    let mut input = BufReader::new(
        ctx.host()
            .open(&index, OpenMode::read_only())
            .map_err(|e| ctx.stop(SEXP_NIL, format!("{}: {e}", index.display())))?,
    );
    let gzip = input
        .fill_buf()
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?
        .starts_with(&[0x1f, 0x8b]);
    let limit = ctx.host().limits().max_decompress_bytes;
    let bytes = if gzip {
        limited_read(ctx, MultiGzDecoder::new(input), limit)?
    } else {
        limited_read(ctx, input, limit)?
    };
    let map = unserialize_bytes(ctx, &bytes, None)?;
    ctx.protect(map);
    let vars = required(ctx, map, "variables")?;
    let vars = named_entries(ctx, vars)?;
    let compressed = required(ctx, map, "compressed")?;
    compression(ctx, compressed)?;
    for (_, key) in &vars {
        Key::parse(ctx, *key)?;
    }
    let state = state_env(ctx, map, &suffix(filebase, ".rdb"))?;
    let references = required(ctx, map, "references")?;
    let references = named_entries(ctx, references)?;
    let table = ctx.session_mut().alloc_env(EMPTY_ENV)?;
    define(ctx, state, "references", table)?;
    for (name, entry) in references {
        define(ctx, table, &name, entry)?;
    }

    Ok((state, vars))
}

pub(super) fn filter(ctx: &mut Ctx, entries: Entries, filter: SexpId) -> EvalResult<Entries> {
    if filter == SEXP_NIL || entries.is_empty() {
        return Ok(entries);
    }
    let labels = strings(ctx, entries.iter().map(|(name, _)| name.as_str()))?;
    ctx.protect(labels);
    let selection = ctx.apply(filter, &[(None, labels)], SEXP_NIL, EMPTY_ENV)?;
    ctx.protect(selection);
    let positions = ctx
        .session_mut()
        .alloc_compact_intseq(1, entries.len(), 1)?;
    let positions = base_call(ctx, "[", &[positions, selection])?;
    Ok(ctx
        .session()
        .int(positions)?
        .iter()
        .map(|position| {
            usize::try_from(position)
                .ok()
                .and_then(|position| position.checked_sub(1))
                .and_then(|index| entries.get(index))
                .cloned()
                .unwrap_or_else(|| ("NA".to_owned(), SEXP_NIL))
        })
        .collect())
}

/// Install lazy promises for each variable in the database at `filebase`.
pub fn load_lazyload_db(ctx: &mut Ctx, filebase: &Path, envir: SexpId) -> EvalResult<()> {
    load_filtered(ctx, filebase, envir, SEXP_NIL)
}

pub(super) fn load_filtered(
    ctx: &mut Ctx,
    filebase: &Path,
    envir: SexpId,
    select: SexpId,
) -> EvalResult<()> {
    load_database(ctx, filebase, envir, select, false)
}

pub(crate) fn load_namespace_db(ctx: &mut Ctx, filebase: &Path, envir: SexpId) -> EvalResult<()> {
    load_database(ctx, filebase, envir, SEXP_NIL, true)
}

fn load_database(
    ctx: &mut Ctx,
    filebase: &Path,
    envir: SexpId,
    select: SexpId,
    namespace: bool,
) -> EvalResult<()> {
    ctx.scoped(|ctx| {
        ctx.protect(envir);
        ctx.protect(select);
        ctx.session().env_enclos(envir)?;
        let (state, vars) = open(ctx, filebase)?;
        for (name, key) in filter(ctx, vars, select)? {
            if namespace && !namespace_binding(&name) {
                continue;
            }
            let value = promise(ctx, state, key)?;
            define(ctx, envir, &name, value)?;
        }
        Ok(())
    })
}

pub(super) fn force_record(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let key = Key::parse(ctx, args.get(0))?;
    let mut hook = DatabaseHook::new(ctx, args.get(1))?;
    let result = hook.fetch(ctx, key);
    let value = hook.finish(ctx, result)?;
    if ctx.session().type_of(value)? == SexpType::Prom {
        ctx.protect(value);
        ctx.force(value)
    } else {
        Ok(value)
    }
}

pub(super) fn force_reference(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let mut hook = DatabaseHook::new(ctx, args.get(1))?;
    let result = hook.read(ctx, args.get(0));
    hook.finish(ctx, result)
}

struct DatabaseHook {
    state: SexpId,
    cache: SexpId,
    references: SexpId,
    file: PathBuf,
    mode: u8,
    pending: SexpId,
    depth: usize,
}

impl DatabaseHook {
    fn new(ctx: &mut Ctx, state: SexpId) -> EvalResult<Self> {
        let cache = state_value(ctx, state, "cache")?;
        let references = state_value(ctx, state, "references")?;
        let file = state_value(ctx, state, "file")?;
        let file = PathBuf::from(string(ctx, file)?);
        let map = state_value(ctx, state, "map")?;
        let mode = required(ctx, map, "compressed")?;
        let mode = compression(ctx, mode)?;
        let pending = ctx.session_mut().alloc_env(EMPTY_ENV)?;
        ctx.protect(pending);
        Ok(Self {
            state,
            cache,
            references,
            file,
            mode,
            pending,
            depth: 0,
        })
    }

    fn fetch(&mut self, ctx: &mut Ctx, key: Key) -> EvalResult {
        let bytes = read_record(ctx, &self.file, key, self.mode)?;
        unserialize_bytes(ctx, &bytes, Some(self))
    }

    fn finish(self, ctx: &mut Ctx, result: EvalResult) -> EvalResult {
        let value = result?;
        for (key, binding) in ctx.session().env_entries(self.pending)? {
            ctx.session_mut()
                .env_define(self.cache, key, binding.value)?;
        }
        Ok(value)
    }

    fn hydrate(&mut self, ctx: &mut Ctx, env: SexpId, reference: SexpId) -> EvalResult<()> {
        let (eager, lazy) = if ctx.session().type_of(reference)? == SexpType::Int {
            (reference, Vec::new())
        } else {
            let eager = required(ctx, reference, "eagerKey")?;
            let lazy = required(ctx, reference, "lazyKeys")?;
            (eager, named_entries(ctx, lazy)?)
        };
        let key = Key::parse(ctx, eager)?;
        let desc = self.fetch(ctx, key)?;
        ctx.protect(desc);
        let parent = required(ctx, desc, "enclos")?;
        let mut ancestor = parent;
        let mut seen = HashSet::from([env]);
        while !matches!(ancestor, EMPTY_ENV | SEXP_NIL) {
            if !seen.insert(ancestor) {
                return Err(ctx.stop(SEXP_NIL, "cyclic lazy-load environment parent chain"));
            }
            ancestor = ctx.session().env_enclos(ancestor)?;
        }
        ctx.session_mut().env_set_enclos(env, parent)?;
        let bindings = required(ctx, desc, "bindings")?;
        for (name, value) in named_entries(ctx, bindings)? {
            define(ctx, env, &name, value)?;
        }
        for (name, key) in lazy {
            Key::parse(ctx, key)?;
            let value = promise(ctx, self.state, key)?;
            define(ctx, env, &name, value)?;
        }
        let attrs = required(ctx, desc, "attributes")?;
        if attrs != SEXP_NIL {
            for (name, value) in named_entries(ctx, attrs)? {
                let name = ctx.session_mut().intern(&name)?;
                ctx.session_mut().set_attrib(env, name, value)?;
            }
        }
        let s4 = required(ctx, desc, "isS4")?;
        let s4 = flag(ctx, s4)?;
        ctx.session_mut().set_s4_flag(env, s4)?;
        let locked = required(ctx, desc, "locked")?;
        let locked = flag(ctx, locked)?;
        ctx.session_mut().env_set_locked(env, locked)?;
        Ok(())
    }
}

impl PersistHook for DatabaseHook {
    fn write(&mut self, _: &mut Ctx, _: SexpId) -> EvalResult<Option<SexpId>> {
        Ok(None)
    }

    fn read(&mut self, ctx: &mut Ctx, key: SexpId) -> EvalResult {
        let name = string(ctx, key)?;
        let symbol = ctx.session_mut().intern(&name)?;
        if let Some(binding) = ctx
            .session()
            .env_get(self.cache, symbol)
            .or_else(|| ctx.session().env_get(self.pending, symbol))
        {
            return Ok(binding.value);
        }
        if self.depth >= ctx.host().limits().max_expressions.min(64) {
            return Err(ctx.stop(SEXP_NIL, "lazy-load environment chain is too deeply nested"));
        }
        let reference = ctx
            .session()
            .env_get(self.references, symbol)
            .map(|binding| binding.value)
            .ok_or_else(|| ctx.stop(SEXP_NIL, format!("unknown lazy-load environment '{name}'")))?;
        let env = ctx.session_mut().alloc_env(EMPTY_ENV)?;
        ctx.protect(env);
        ctx.session_mut().env_define(self.pending, symbol, env)?;
        self.depth += 1;
        let result = self.hydrate(ctx, env, reference);
        self.depth -= 1;
        result?;
        Ok(env)
    }
}

struct Callback {
    fun: SexpId,
    call: Call,
}

impl PersistHook for Callback {
    fn write(&mut self, _: &mut Ctx, _: SexpId) -> EvalResult<Option<SexpId>> {
        Ok(None)
    }

    fn read(&mut self, ctx: &mut Ctx, key: SexpId) -> EvalResult {
        ctx.apply(
            self.fun,
            &[(None, key)],
            self.call.err_call(),
            self.call.env,
        )
    }
}

pub(super) fn fetch_with_callback(
    ctx: &mut Ctx,
    file: &Path,
    key: Key,
    mode: u8,
    fun: SexpId,
    call: &Call,
) -> EvalResult {
    let bytes = read_record(ctx, file, key, mode)?;
    let mut callback = Callback { fun, call: *call };
    let result = unserialize_bytes(ctx, &bytes, Some(&mut callback))?;
    if ctx.session().type_of(result)? == SexpType::Prom {
        ctx.protect(result);
        ctx.force(result)
    } else {
        Ok(result)
    }
}

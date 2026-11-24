use std::borrow::Cow;
use std::collections::HashMap;
use std::io::BufRead;

use roxide_eval::{AltOp, Unwind};
use roxide_gc::SexpId;
use roxide_host::Limits;
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, Binding, CharId, EMPTY_ENV, Encoding, GLOBAL_ENV, NA_INTEGER,
    NA_REAL, PromiseState, RComplex, RLogical, RSession, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND,
    SexpType, Sym, ValueApi,
};

use super::{
    ACTIVE_BINDING, ALTREP, ATTR, BASE, BASE_NS, BYTES, EMPTY, GLOBAL, LATIN1, LOCKED_BINDING,
    MISSING, NAMESPACE, NIL, OBJECT, PACKAGE, PERSIST, REF, Registry, S4, TAG, UNBOUND, UTF8,
    WireResult, invalid,
};

use super::session::CodecSession;

const BCODE: u32 = 21;
const BCREPREF: u32 = 243;
const BCREPDEF: u32 = 244;
const ATTRLANG: u32 = 240;
const ATTRLIST: u32 = 239;

pub(super) fn decode(
    session: &mut CodecSession<'_, '_>,
    bytes: &[u8],
    workspace: bool,
    limits: &Limits,
    registry: &Registry,
) -> WireResult<SexpId> {
    if bytes.len() > limits.max_decompress_bytes {
        return Err(invalid("serialized input exceeds the decompression limit"));
    }
    decode_from(session, Input::Slice(bytes), workspace, limits, registry).map(|(value, _)| value)
}

pub(super) enum StreamObject {
    Decoded(SexpId),
    Buffered(Vec<u8>),
}

pub(super) fn decode_stream_or_buffer(
    session: &mut RSession,
    input: &mut dyn BufRead,
    workspace: bool,
    limits: &Limits,
    registry: &Registry,
) -> WireResult<StreamObject> {
    let mut bytes = Vec::new();
    let input = Input::Stream(input, &mut bytes);
    let (value, replay) = decode_from(
        &mut CodecSession::Stream(session),
        input,
        workspace,
        limits,
        registry,
    )?;
    Ok(if replay {
        StreamObject::Buffered(bytes)
    } else {
        StreamObject::Decoded(value)
    })
}

fn decode_from(
    session: &mut CodecSession<'_, '_>,
    input: Input<'_>,
    workspace: bool,
    limits: &Limits,
    registry: &Registry,
) -> WireResult<(SexpId, bool)> {
    let remaining = match &input {
        Input::Slice(bytes) => bytes.len(),
        Input::Stream(..) => limits.max_decompress_bytes,
    };
    let mut reader = Reader {
        input,
        remaining,
        scratch: Vec::new(),
        refs: Vec::new(),
        binding_flags: HashMap::new(),
        env_parents: HashMap::new(),
        version: 0,
        ascii: false,
        replay: false,
        heap_start: session.heap.live_bytes(),
        limits,
        registry,
    };
    let workspace_format = if workspace {
        Some(match reader.take(5)? {
            b"RDX2\n" => (2, false),
            b"RDX3\n" => (3, false),
            b"RDA2\n" => (2, true),
            b"RDA3\n" => (3, true),
            _ => return Err(invalid("invalid R workspace header")),
        })
    } else {
        None
    };
    reader.ascii = match (reader.take(2)?, workspace_format) {
        (b"A\n", None | Some((_, true))) => true,
        (b"X\n", None | Some((_, false))) => false,
        (_, Some((_, true))) => return Err(invalid("invalid ASCII stream header")),
        _ => return Err(invalid("invalid XDR stream header")),
    };
    reader.version = reader.word()?;
    if !matches!(reader.version, 2 | 3)
        || workspace_format.is_some_and(|(version, _)| version != reader.version)
    {
        return Err(invalid("unsupported serialization version"));
    }
    reader.word()?;
    reader.word()?;
    if reader.version == 3 {
        let len = reader.vector_len::<u8>(1)?;
        reader.string_bytes(len)?;
    }
    let value = reader.item(session, 0)?;
    reader.check_heap(session)?;
    validate_parents(reader.env_parents)?;
    Ok((value, reader.replay))
}

enum Input<'a> {
    Slice(&'a [u8]),
    Stream(&'a mut dyn BufRead, &'a mut Vec<u8>),
}

struct Reader<'a, 'limits> {
    ascii: bool,
    replay: bool,
    input: Input<'a>,
    remaining: usize,
    scratch: Vec<u8>,
    refs: Vec<SexpId>,
    binding_flags: HashMap<SexpId, u32>,
    env_parents: HashMap<SexpId, (SexpId, Option<SexpId>)>,
    version: u32,
    heap_start: usize,
    limits: &'limits Limits,
    registry: &'limits Registry,
}

impl Reader<'_, '_> {
    fn take(&mut self, n: usize) -> WireResult<&[u8]> {
        self.remaining = self
            .remaining
            .checked_sub(n)
            .ok_or_else(|| match self.input {
                Input::Slice(_) => truncated(),
                Input::Stream(..) => invalid("serialized input exceeds the decompression limit"),
            })?;
        match &mut self.input {
            Input::Slice(bytes) => {
                let (head, rest) = bytes.split_at_checked(n).ok_or_else(truncated)?;
                *bytes = rest;
                Ok(head)
            }
            Input::Stream(input, captured) => {
                self.scratch.clear();
                while self.scratch.len() < n {
                    let available = input.fill_buf()?;
                    if available.is_empty() {
                        return Err(truncated());
                    }
                    let count = available.len().min(n - self.scratch.len());
                    self.scratch.try_reserve(count).map_err(invalid)?;
                    self.scratch.extend_from_slice(&available[..count]);
                    input.consume(count);
                }
                captured.try_reserve(n).map_err(invalid)?;
                captured.extend_from_slice(&self.scratch);
                Ok(&self.scratch)
            }
        }
    }

    fn line(&mut self) -> WireResult<&str> {
        match &mut self.input {
            Input::Slice(bytes) => {
                let end = bytes
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .ok_or_else(truncated)?;
                let line = &bytes[..end];
                *bytes = &bytes[end + 1..];
                self.remaining = bytes.len();
                std::str::from_utf8(line).map_err(invalid)
            }
            Input::Stream(input, captured) => {
                self.scratch.clear();
                loop {
                    let available = input.fill_buf()?;
                    if available.is_empty() {
                        return Err(truncated());
                    }
                    let end = available.iter().position(|&byte| byte == b'\n');
                    let count = end.map_or(available.len(), |i| i + 1);
                    self.remaining = self.remaining.checked_sub(count).ok_or_else(|| {
                        invalid("serialized input exceeds the decompression limit")
                    })?;
                    self.scratch.try_reserve(count).map_err(invalid)?;
                    self.scratch.extend_from_slice(&available[..count]);
                    input.consume(count);
                    if end.is_some() {
                        captured.try_reserve(self.scratch.len()).map_err(invalid)?;
                        captured.extend_from_slice(&self.scratch);
                        self.scratch.pop();
                        return std::str::from_utf8(&self.scratch).map_err(invalid);
                    }
                }
            }
        }
    }

    fn string_bytes(&mut self, len: usize) -> WireResult<Cow<'_, [u8]>> {
        if self.ascii {
            return super::ascii::decode_string(self.line()?.as_bytes(), len).map(Cow::Owned);
        }
        self.take(len).map(Cow::Borrowed)
    }

    fn word(&mut self) -> WireResult<u32> {
        if self.ascii {
            return match self.line()? {
                "NA" => Ok(NA_INTEGER as u32),
                line => line.parse::<i32>().map(|n| n as u32).map_err(invalid),
            };
        }
        let b = self.take(4)?.try_into().map_err(invalid)?;
        Ok(u32::from_be_bytes(b))
    }

    fn length(&mut self) -> WireResult<usize> {
        let n = self.word()?;
        match n {
            u32::MAX => {
                let hi = u64::from(self.word()?);
                let lo = u64::from(self.word()?);
                usize::try_from((hi << 32) | lo).map_err(invalid)
            }
            n if n <= i32::MAX as u32 => Ok(n as usize),
            _ => Err(invalid("invalid serialized length")),
        }
    }

    fn vector_len<T>(&mut self, width: usize) -> WireResult<usize> {
        let len = self.length()?;
        self.check_vector::<T>(len, width)?;
        Ok(len)
    }

    fn check_vector<T>(&self, len: usize, width: usize) -> WireResult<()> {
        if len > self.limits.max_vector_bytes / size_of::<T>() {
            return Err(invalid("serialized vector exceeds the allocation limit"));
        }
        let width = if self.ascii { 1 } else { width };
        if len > self.remaining / width {
            return Err(match self.input {
                Input::Slice(_) => truncated(),
                Input::Stream(..) => invalid("serialized input exceeds the decompression limit"),
            });
        }
        Ok(())
    }

    fn f64(&mut self) -> WireResult<f64> {
        if self.ascii {
            return match self.line()? {
                "NA" => Ok(NA_REAL),
                line => line.parse().map_err(invalid),
            };
        }
        Ok(f64::from_be_bytes(
            self.take(8)?.try_into().map_err(invalid)?,
        ))
    }

    fn vector<T>(
        &mut self,
        width: usize,
        mut get: impl FnMut(&mut Self) -> WireResult<T>,
    ) -> WireResult<Vec<T>> {
        let len = self.vector_len::<T>(width)?;
        let mut values = Vec::new();
        self.reserve_vector(&mut values, len)?;
        for _ in 0..len {
            let value = get(self)?;
            values.try_reserve(1).map_err(invalid)?;
            values.push(value);
        }
        Ok(values)
    }

    fn reserve_vector<T>(&self, values: &mut Vec<T>, len: usize) -> WireResult<()> {
        if matches!(self.input, Input::Slice(_)) {
            values.try_reserve_exact(len).map_err(invalid)?;
        }
        Ok(())
    }

    fn reference(&mut self, id: SexpId) -> WireResult<()> {
        if self.refs.len() >= self.limits.max_vector_bytes / size_of::<SexpId>() {
            return Err(invalid(
                "serialization reference table exceeds the allocation limit",
            ));
        }
        self.refs.try_reserve(1).map_err(invalid)?;
        self.refs.push(id);
        Ok(())
    }

    fn item(&mut self, session: &mut CodecSession<'_, '_>, depth: usize) -> WireResult<SexpId> {
        self.check_depth(depth)?;
        let flags = self.word()?;
        let value = self.item_with_flags(session, flags, depth + 1)?;
        if !matches!(
            flags & 0xff,
            0 | NIL | GLOBAL | BASE | BASE_NS | EMPTY | MISSING | UNBOUND | REF | 1
        ) {
            session.root(value);
        }
        Ok(value)
    }

    fn check_depth(&self, depth: usize) -> WireResult<()> {
        if depth > self.limits.max_expressions {
            return Err(invalid("serialized object exceeds the nesting limit"));
        }
        Ok(())
    }

    fn check_heap(&self, session: &RSession) -> WireResult<()> {
        let limit = self
            .limits
            .max_vector_bytes
            .max(self.limits.max_decompress_bytes);
        if session.heap.live_bytes().saturating_sub(self.heap_start) > limit {
            return Err(invalid(
                "serialized object exceeds the heap allocation limit",
            ));
        }
        Ok(())
    }

    fn item_with_flags(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        flags: u32,
        depth: usize,
    ) -> WireResult<SexpId> {
        self.check_heap(session)?;
        let ty = flags & 0xff;
        let value = match ty {
            0 | NIL => return Ok(SEXP_NIL),
            GLOBAL => return Ok(GLOBAL_ENV),
            BASE => return Ok(BASE_ENV),
            BASE_NS => return Ok(BASE_NAMESPACE),
            EMPTY => return Ok(EMPTY_ENV),
            MISSING => return Ok(SEXP_MISSING),
            UNBOUND => return Ok(SEXP_UNBOUND),
            REF => {
                let inline = flags >> 8;
                let index = if inline == 0 { self.word()? } else { inline };
                return index
                    .checked_sub(1)
                    .and_then(|i| self.refs.get(i as usize))
                    .copied()
                    .ok_or_else(|| invalid("invalid serialization reference"));
            }
            1 => {
                let ch = self.read_char(session, depth)?;
                let name = char_text(session, ch)?;
                let sym = session.intern(&name)?.as_sexp();
                self.reference(sym)?;
                return Ok(sym);
            }
            2 | 6 | 17 => return self.cons_chain(session, flags, depth),
            3 | 5 => return self.function(session, flags, depth),
            4 => return self.environment(session, depth),
            7 | 8 => return self.primitive(session, ty),
            9 => return Ok(self.character(session, flags, depth)?.as_sexp()),
            10 => session
                .alloc_lgl_from(&self.vector(4, |r| Ok(RLogical::from_raw(r.word()? as i32)))?)?,
            13 => session.alloc_int_from(&self.vector(4, |r| Ok(r.word()? as i32))?)?,
            14 => session.alloc_real_from(&self.vector(8, Self::f64)?)?,
            15 => session.alloc_cplx_from(&self.vector(16, |r| {
                Ok(RComplex {
                    re: r.f64()?,
                    im: r.f64()?,
                })
            })?)?,
            16 => {
                let len = self.vector_len::<CharId>(8)?;
                let chars = self.characters(session, len, depth)?;
                session.alloc_str_from(&chars)?
            }
            19 => {
                let values = self.vector(4, |reader| reader.item(session, depth))?;
                crate::ops::common::list(session, &values)?
            }
            20 => {
                let values = self.vector(4, |reader| reader.item(session, depth))?;
                session.alloc_expr_from(&values)?
            }
            BCODE => {
                let source = self.bytecode(session, depth)?;
                self.optional_attributes(session, flags, depth)?;
                return Ok(source);
            }
            22 => {
                let ptr = session.alloc_extptr(0, SEXP_NIL, SEXP_NIL)?;
                session.root(ptr);
                self.reference(ptr)?;
                let prot = self.item(session, depth)?;
                session.set_extptr_prot(ptr, prot)?;
                let tag = self.item(session, depth)?;
                session.set_extptr_tag(ptr, tag)?;
                ptr
            }
            24 => {
                let len = self.vector_len::<u8>(1)?;
                if self.ascii {
                    let mut bytes = Vec::new();
                    bytes.try_reserve_exact(len).map_err(invalid)?;
                    for _ in 0..len {
                        bytes.push(u8::from_str_radix(self.line()?, 16).map_err(invalid)?);
                    }
                    session.alloc_raw_from(&bytes)?
                } else {
                    session.alloc_raw_from(self.take(len)?)?
                }
            }
            25 => {
                let object = session.alloc_s4_object()?;
                session.set_s4_flag(object, flags & S4 != 0)?;
                object
            }
            PERSIST => {
                if self.word()? != 0 {
                    return Err(invalid("invalid persistent reference header"));
                }
                let count = self.vector_len::<CharId>(8)?;
                if count == 0 {
                    return Err(invalid("empty persistent reference key"));
                }
                let chars = self.characters(session, count, depth)?;
                let key = session.alloc_str_from(&chars)?;
                let value = session.restore(key)?;
                self.reference(value)?;
                return Ok(value);
            }
            NAMESPACE | PACKAGE => return self.named_environment(session, ty, depth),
            ALTREP if self.version == 3 => return self.altrep(session, flags, depth),
            _ => return Err(invalid(format!("unsupported serialized R type {ty}"))),
        };
        if flags & ATTR != 0 {
            session.root(value);
        }
        let attrs = self.optional_attributes(session, flags, depth)?;
        self.apply_attributes(session, value, attrs, flags)?;
        Ok(value)
    }

    fn apply_attributes(
        &self,
        session: &mut RSession,
        value: SexpId,
        attrs: SexpId,
        flags: u32,
    ) -> WireResult<()> {
        if !self.replay {
            let entries = attribute_entries(session, attrs)?;
            set_attributes(session, value, entries, flags)?;
        }
        Ok(())
    }

    fn read_char(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        depth: usize,
    ) -> WireResult<CharId> {
        let flags = self.word()?;
        if flags & 0xff != SexpType::Char as u32 {
            return Err(invalid("invalid serialized character element"));
        }
        self.character(session, flags, depth)
    }

    fn characters(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        len: usize,
        depth: usize,
    ) -> WireResult<Vec<CharId>> {
        let mut chars = Vec::new();
        self.reserve_vector(&mut chars, len)?;
        let mut protected = 0;
        for _ in 0..len {
            let flags = self.word()?;
            if flags & 0xff != SexpType::Char as u32 {
                return Err(invalid("invalid serialized character element"));
            }
            if flags & ATTR != 0 {
                // Reading an attribute can call a hook. Ordinary strings have no attributes.
                for ch in &chars[protected..] {
                    session.root(CharId::as_sexp(*ch));
                }
                protected = chars.len() + 1;
            }
            chars.push(self.character(session, flags, depth)?);
        }
        Ok(chars)
    }

    fn character(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        flags: u32,
        depth: usize,
    ) -> WireResult<CharId> {
        let len = self.word()?;
        let ch = if len == u32::MAX {
            CharId::NA
        } else {
            if len > i32::MAX as u32 {
                return Err(invalid("invalid serialized string length"));
            }
            self.check_vector::<u8>(len as usize, 1)?;
            let bytes = self.string_bytes(len as usize)?;
            let encoding = if flags & BYTES != 0 {
                Encoding::Bytes
            } else if flags & LATIN1 != 0 {
                Encoding::Latin1
            } else if flags & UTF8 != 0 {
                if std::str::from_utf8(&bytes).is_err() {
                    return Err(invalid("invalid UTF-8 in serialized character data"));
                }
                Encoding::Utf8
            } else {
                Encoding::Native
            };
            session.mk_char(&bytes, encoding)?
        };
        if flags & ATTR != 0 {
            session.root(ch.as_sexp());
            self.optional_attributes(session, flags, depth)?;
        }
        Ok(ch)
    }

    fn optional_attributes(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        flags: u32,
        depth: usize,
    ) -> WireResult<SexpId> {
        if flags & ATTR != 0 {
            self.item(session, depth)
        } else {
            Ok(SEXP_NIL)
        }
    }

    fn cons_chain(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        mut flags: u32,
        depth: usize,
    ) -> WireResult<SexpId> {
        let mut cells = Vec::new();
        while matches!(flags & 0xff, 2 | 6 | 17) {
            let attrs = self.optional_attributes(session, flags, depth)?;
            let tag = if flags & TAG != 0 {
                self.item(session, depth)?
            } else {
                SEXP_NIL
            };
            let car = self.item(session, depth)?;
            if (cells.len() + 1) * size_of::<(u32, SexpId, SexpId, SexpId)>()
                > self.limits.max_vector_bytes
            {
                return Err(invalid("serialized pairlist exceeds the allocation limit"));
            }
            cells.try_reserve(1).map_err(invalid)?;
            cells.push((flags, attrs, tag, car));
            flags = self.word()?;
        }
        let mut tail = self.item_with_flags(session, flags, depth)?;
        session.root(tail);
        for (flags, attrs, tag, car) in cells.into_iter().rev() {
            self.check_heap(session)?;
            tail = match flags & 0xff {
                2 => session.cons(car, tail, tag)?,
                6 => session.lcons(car, tail, tag)?,
                _ => session.dcons(car, tail, tag)?,
            };
            if flags & (LOCKED_BINDING | ACTIVE_BINDING | (3 << 12)) != 0 {
                self.binding_flags.try_reserve(1).map_err(invalid)?;
                self.binding_flags.insert(tail, flags);
            }
            self.apply_attributes(session, tail, attrs, flags)?;
        }
        Ok(tail)
    }

    fn function(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        flags: u32,
        depth: usize,
    ) -> WireResult<SexpId> {
        let attrs = self.optional_attributes(session, flags, depth)?;
        let env = if flags & TAG != 0 {
            self.item(session, depth)?
        } else {
            SEXP_NIL
        };
        let car = self.item(session, depth)?;
        let cdr = self.item(session, depth)?;
        let value = if flags & 0xff == SexpType::Clo as u32 {
            let env = if env == SEXP_NIL { BASE_ENV } else { env };
            session.env_enclos(env)?;
            if car != SEXP_NIL && session.type_of(car)? != SexpType::List {
                return Err(invalid("invalid serialized closure formals"));
            }
            session.alloc_closure(car, cdr, env)?
        } else {
            if car == SEXP_UNBOUND {
                session.env_enclos(env)?;
            }
            let promise = session.alloc_promise(cdr, env)?;
            if car != SEXP_UNBOUND {
                session.set_promise_state(promise, PromiseState::Forced, SEXP_NIL, car)?;
            }
            promise
        };
        self.apply_attributes(session, value, attrs, flags)?;
        Ok(value)
    }

    fn environment(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        depth: usize,
    ) -> WireResult<SexpId> {
        let env = session.alloc_env(EMPTY_ENV)?;
        session.root(env);
        self.reference(env)?;
        let locked = match self.word()? {
            0 => false,
            1 => true,
            _ => return Err(invalid("invalid serialized environment lock")),
        };
        let enclos = match self.item(session, depth)? {
            SEXP_NIL => BASE_ENV,
            parent => parent,
        };
        session.env_set_enclos(env, enclos)?;
        self.env_parents.try_reserve(1).map_err(invalid)?;
        self.env_parents.insert(env, (enclos, None));
        let frame = self.item(session, depth)?;
        let table = self.item(session, depth)?;
        let attrs = self.item(session, depth)?;
        self.define_frame(session, env, frame)?;
        if table != SEXP_NIL {
            if session.type_of(table)? != SexpType::Vec {
                return Err(invalid("invalid serialized environment hash table"));
            }
            let count = session.list_slice(table)?.len();
            for i in 0..count {
                let bucket = session.list_get(table, i)?;
                self.define_frame(session, env, bucket)?;
            }
        }
        self.apply_attributes(session, env, attrs, 0)?;
        session.env_set_locked(env, locked)?;
        Ok(env)
    }

    fn define_frame(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        env: SexpId,
        mut frame: SexpId,
    ) -> WireResult<()> {
        let mut entries = Vec::new();
        while frame != SEXP_NIL {
            if session.type_of(frame)? != SexpType::List {
                return Err(invalid("invalid serialized environment frame"));
            }
            let (value, tail, tag) = session.cons_parts(frame)?;
            let name = symbol(session, tag)?;
            let flags = self.binding_flags.remove(&frame).unwrap_or(0);
            if (entries.len() + 1) * size_of::<(Sym, Binding)>() > self.limits.max_vector_bytes {
                return Err(invalid(
                    "serialized environment frame exceeds the allocation limit",
                ));
            }
            entries.try_reserve(1).map_err(invalid)?;
            entries.push((
                name,
                Binding {
                    value,
                    locked: flags & LOCKED_BINDING != 0,
                    active: flags & ACTIVE_BINDING != 0,
                    missing: ((flags >> 12) & 3) as u8,
                },
            ));
            frame = tail;
        }
        for (name, binding) in entries.into_iter().rev() {
            if session.env_get(env, name).is_some() {
                return Err(invalid("duplicate serialized environment binding"));
            }
            session.env_define_binding(env, name, binding)?;
        }
        Ok(())
    }

    fn named_environment(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        ty: u32,
        depth: usize,
    ) -> WireResult<SexpId> {
        if self.word()? != 0 {
            return Err(invalid("invalid serialized package or namespace"));
        }
        let count = self.vector_len::<CharId>(8)?;
        if count == 0 {
            return Err(invalid("missing serialized package or namespace name"));
        }
        let ch = self.read_char(session, depth)?;
        let name = char_text(session, ch)?;
        for _ in 1..count {
            self.read_char(session, depth)?;
        }
        let env = match name.as_str() {
            "base" if ty == NAMESPACE => BASE_NAMESPACE,
            "package:base" => BASE_ENV,
            _ => {
                let registered = self
                    .registry
                    .packages
                    .iter()
                    .find_map(|(pkg, ns, attached)| {
                        if ty == NAMESPACE && name == pkg.as_str() {
                            Some(*ns)
                        } else if ty == PACKAGE
                            && name.strip_prefix("package:") == Some(pkg.as_str())
                        {
                            *attached
                        } else {
                            None
                        }
                    });
                if let Some(env) = registered {
                    env
                } else {
                    session.alloc_env(BASE_ENV)?
                }
            }
        };
        self.reference(env)?;
        Ok(env)
    }

    fn primitive(&mut self, session: &mut CodecSession<'_, '_>, ty: u32) -> WireResult<SexpId> {
        let len = self.vector_len::<u8>(1)?;
        let bytes = self.string_bytes(len)?;
        let name = std::str::from_utf8(&bytes).map_err(invalid)?;
        let sym = session.intern(name)?;
        let expected = ty == SexpType::Special as u32;
        for env in Registry::envs(&self.registry.packages) {
            if let Some(binding) = session.env_get(env, sym)
                && let Ok((_, special)) = session.prim_parts(binding.value)
                && special == expected
            {
                return Ok(binding.value);
            }
        }
        let name = session.sym_name(sym)?;
        Err(invalid(format!("unknown serialized primitive '{name}'")))
    }

    fn altrep(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        flags: u32,
        depth: usize,
    ) -> WireResult<SexpId> {
        let info = self.item(session, depth)?;
        let state = self.item(session, depth)?;
        let attrs = self.item(session, depth)?;
        let (class, rest, _) = session.cons_parts(info)?;
        let (package, rest, _) = session.cons_parts(rest)?;
        let (kind, end, _) = session.cons_parts(rest)?;
        let kind = session.int(kind)?;
        if end != SEXP_NIL || kind.len() != 1 {
            return Err(invalid("invalid serialized ALTREP class info"));
        }
        let ty = match kind.get(0) {
            10 => SexpType::Lgl,
            13 => SexpType::Int,
            14 => SexpType::Real,
            15 => SexpType::Cplx,
            16 => SexpType::Str,
            19 => SexpType::Vec,
            24 => SexpType::Raw,
            _ => return Err(invalid("invalid serialized ALTREP vector type")),
        };
        let class = symbol(session, class)?;
        let package = symbol(session, package)?;
        let class_name = session.sym_name(class)?.to_owned();
        let package_name = session.sym_name(package)?.to_owned();
        let builtin = match (package_name.as_str(), class_name.as_str()) {
            ("base", "compact_intseq") => Some(SexpType::Int),
            ("base", "compact_realseq") => Some(SexpType::Real),
            ("base", "wrap_logical") => Some(SexpType::Lgl),
            ("base", "wrap_integer") => Some(SexpType::Int),
            ("base", "wrap_real") => Some(SexpType::Real),
            ("base", "wrap_complex") => Some(SexpType::Cplx),
            ("base", "wrap_string") => Some(SexpType::Str),
            ("base", "wrap_raw") => Some(SexpType::Raw),
            _ => None,
        };
        if matches!(session, CodecSession::Stream(_)) && (self.replay || builtin.is_none()) {
            self.replay = true;
            return empty_vector(session, ty);
        }

        let entries = attribute_entries(session, attrs)?;
        let value = if let Some(base_type) = builtin {
            if ty != base_type {
                return Err(invalid("invalid serialized ALTREP vector type"));
            }
            if class_name.starts_with("wrap_") {
                let (target, metadata, _) = session.cons_parts(state)?;
                if session.type_of(target)? != ty || session.int(metadata)?.len() != 2 {
                    return Err(invalid("invalid serialized ALTREP wrapper state"));
                }
                session.alloc_alt_wrapper(target)?
            } else {
                self.compact_sequence(session, ty, state)?
            }
        } else {
            let op = AltOp::Unserialize {
                class,
                package,
                ty,
                state,
                attrs,
                objf: i32::from(flags & OBJECT != 0),
                levels: ((flags >> 12) & 0xffff) as i32,
            };
            let value = session.eval(|ctx, call| {
                if let Some(value) = restore_altrep(ctx, op)? {
                    return Ok(Some(value));
                }
                if ctx.ns_lookup(&package_name).is_none() {
                    ctx.with_native_error_handler(
                        |ctx| ctx.load_namespace(&package_name, call),
                        |_, _| Ok(SEXP_NIL),
                    )?;
                    if let Some(value) = restore_altrep(ctx, op)? {
                        return Ok(Some(value));
                    }
                }
                ctx.warn_now(call, format!(
                    "cannot unserialize ALTVEC object of class '{class_name}' from package '{package_name}'; returning length zero vector"
                ))?;
                Ok(None)
            })?;
            let Some(value) = value else {
                return empty_vector(session, ty);
            };
            if value == SEXP_NIL
                || !session.attrs(value)?.is_empty()
                || session.is_object(value)?
                || session.is_s4(value)?
                || session.gp(value)? != 0
            {
                return Ok(value);
            }
            value
        };
        session.root(value);
        if entries
            .iter()
            .any(|&(name, _)| session.sym_name(name).is_ok_and(|name| name == "names"))
            && session
                .foreign_alt(value)?
                .is_some_and(|alt| !alt.owned && alt.len.is_none())
        {
            session.eval(|ctx, _| ctx.altrep_len(value))?;
        }
        set_attributes(session, value, entries, flags)?;
        session.set_object_flag(value, flags & OBJECT != 0)?;
        session.set_gp(value, ((flags >> 12) & 0xffff) as u16)?;
        Ok(value)
    }

    fn compact_sequence(
        &self,
        session: &mut RSession,
        ty: SexpType,
        state: SexpId,
    ) -> WireResult<SexpId> {
        let state = session.real(state)?;
        if state.len() != 3 {
            return Err(invalid("invalid serialized ALTREP state"));
        }
        let (len, start, step) = (state.get(0), state.get(1), state.get(2));
        let int = ty == SexpType::Int;
        let size = if int {
            size_of::<i32>()
        } else {
            size_of::<f64>()
        };
        if !len.is_finite()
            || len < 0.0
            || len.fract() != 0.0
            || len > (self.limits.max_vector_bytes / size) as f64
        {
            return Err(invalid("invalid serialized compact sequence length"));
        }
        if step.abs() != 1.0 || !start.is_finite() {
            return Err(invalid("invalid serialized compact sequence state"));
        }
        let value = if int {
            if start.fract() != 0.0 || start.abs() > f64::from(i32::MAX) {
                return Err(invalid("invalid serialized compact integer sequence start"));
            }
            session.alloc_compact_intseq(start as i32, len as usize, step as i8)?
        } else {
            session.alloc_compact_realseq(start, len as usize, step)?
        };
        Ok(value)
    }

    fn bytecode(&mut self, session: &mut CodecSession<'_, '_>, depth: usize) -> WireResult<SexpId> {
        let count = self.vector_len::<Option<SexpId>>(1)?;
        let mut refs = Vec::new();
        refs.try_reserve_exact(count).map_err(invalid)?;
        refs.resize(count, None);
        self.bytecode_body(session, &mut refs, depth)
    }

    fn bytecode_body(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        refs: &mut [Option<SexpId>],
        depth: usize,
    ) -> WireResult<SexpId> {
        self.check_depth(depth)?;
        let code = self.item(session, depth)?;
        if session.type_of(code)? != SexpType::Int {
            return Err(invalid("invalid serialized bytecode instructions"));
        }
        let count = self.vector_len::<SexpId>(8)?;
        if count == 0 {
            return Err(invalid("serialized bytecode has no source expression"));
        }
        // The first constant is the source expression. Read the rest to keep the reference table in order.
        let marker = self.word()?;
        let source = self.bytecode_constant(session, refs, marker, depth + 1)?;
        for _ in 1..count {
            let marker = self.word()?;
            self.bytecode_constant(session, refs, marker, depth + 1)?;
        }
        Ok(source)
    }

    fn bytecode_constant(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        refs: &mut [Option<SexpId>],
        kind: u32,
        depth: usize,
    ) -> WireResult<SexpId> {
        match kind {
            BCODE => self.bytecode_body(session, refs, depth),
            2 | 6 | ATTRLIST | ATTRLANG | BCREPREF | BCREPDEF => {
                self.bytecode_node(session, refs, kind, depth)
            }
            _ => self.item(session, depth),
        }
    }

    fn bytecode_child(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        refs: &mut [Option<SexpId>],
        depth: usize,
    ) -> WireResult<SexpId> {
        match self.word()? {
            0 => self.item(session, depth),
            kind => self.bytecode_node(session, refs, kind, depth),
        }
    }

    fn bytecode_node(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        refs: &mut [Option<SexpId>],
        kind: u32,
        depth: usize,
    ) -> WireResult<SexpId> {
        self.check_depth(depth)?;
        self.check_heap(session)?;
        match kind {
            BCREPREF => refs
                .get(self.word()? as usize)
                .copied()
                .flatten()
                .filter(|&value| value != SEXP_NIL)
                .ok_or_else(|| invalid("invalid or cyclic bytecode repetition reference")),
            BCREPDEF => {
                let index = self.word()? as usize;
                if !matches!(refs.get(index), Some(None)) {
                    return Err(invalid("invalid bytecode repetition index"));
                }
                let kind = self.word()?;
                if !matches!(kind, 2 | 6 | ATTRLIST | ATTRLANG) {
                    return Err(invalid("invalid bytecode repetition node"));
                }
                refs[index] = Some(SEXP_NIL);
                let value = self.bytecode_node(session, refs, kind, depth + 1)?;
                refs[index] = Some(value);
                Ok(value)
            }
            2 | 6 | ATTRLIST | ATTRLANG => {
                let attrs = if matches!(kind, ATTRLIST | ATTRLANG) {
                    self.item(session, depth + 1)?
                } else {
                    SEXP_NIL
                };
                let tag = self.item(session, depth + 1)?;
                let car = self.bytecode_child(session, refs, depth + 1)?;
                let tail = self.bytecode_child(session, refs, depth + 1)?;
                let value = if matches!(kind, 6 | ATTRLANG) {
                    session.lcons(car, tail, tag)?
                } else {
                    session.cons(car, tail, tag)?
                };
                session.root(value);
                self.apply_attributes(session, value, attrs, 0)?;
                Ok(value)
            }
            _ => Err(invalid("invalid bytecode child marker")),
        }
    }
}

fn validate_parents(mut parents: HashMap<SexpId, (SexpId, Option<SexpId>)>) -> WireResult<()> {
    // Only environments from this stream can point back into it.
    let starts: Vec<_> = parents.keys().copied().collect();
    for start in starts {
        let mut env = start;
        while let Some((parent, visited_from)) = parents.get_mut(&env) {
            match *visited_from {
                Some(previous) if previous == start => {
                    return Err(invalid("cyclic serialized environment parent chain"));
                }
                Some(_) => break,
                None => {
                    *visited_from = Some(start);
                    env = *parent;
                }
            }
        }
    }
    Ok(())
}

fn char_text(session: &RSession, ch: CharId) -> WireResult<String> {
    if ch == CharId::NA {
        return Err(invalid("missing serialized symbol name"));
    }
    let bytes = session.char_bytes(ch)?;
    if session.char_encoding(ch)? == Encoding::Latin1 {
        Ok(bytes.iter().copied().map(char::from).collect())
    } else {
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(invalid)
    }
}

fn symbol(session: &mut RSession, id: SexpId) -> WireResult<Sym> {
    if matches!(id, SEXP_MISSING | SEXP_UNBOUND) {
        return Err(invalid("invalid serialized symbol"));
    }
    let name = session.sym_name_of(id)?.to_owned();
    if name.is_empty() {
        return Err(invalid("empty serialized symbol name"));
    }
    Ok(session.intern(&name)?)
}

fn attribute_entries(session: &mut RSession, mut attrs: SexpId) -> WireResult<Vec<(Sym, SexpId)>> {
    let mut entries = Vec::new();
    while attrs != SEXP_NIL {
        if session.type_of(attrs)? != SexpType::List {
            return Err(invalid("invalid serialized attribute list"));
        }
        let (value, next, name) = session.cons_parts(attrs)?;
        entries.try_reserve(1).map_err(invalid)?;
        entries.push((symbol(session, name)?, value));
        attrs = next;
    }
    Ok(entries)
}

fn set_attributes(
    session: &mut RSession,
    id: SexpId,
    entries: Vec<(Sym, SexpId)>,
    flags: u32,
) -> WireResult<()> {
    for &(name, value) in &entries {
        if session.sym_name(name)? == "names"
            && value != SEXP_NIL
            && session.str_slice(value)?.len() != session.xlength(id)?
        {
            return Err(invalid("invalid serialized names attribute length"));
        }
    }
    if !entries.is_empty() {
        session.replace_attributes(id, entries)?;
    }
    if flags & S4 != 0 {
        session.set_s4_flag(id, true)?;
    }
    Ok(())
}

fn restore_altrep(
    ctx: &mut roxide_eval::Ctx,
    op: AltOp,
) -> roxide_eval::EvalResult<Option<SexpId>> {
    if ctx.native_hooks().is_none() {
        return Ok(None);
    }
    match ctx.altrep_method(op) {
        Err(Unwind::Internal(error)) if error.message == "foreign ALTREP hook not installed" => {
            Ok(None)
        }
        result => result,
    }
}

fn empty_vector(session: &mut RSession, ty: SexpType) -> WireResult<SexpId> {
    Ok(match ty {
        SexpType::Lgl => session.alloc_lgl_from(&[])?,
        SexpType::Int => session.alloc_int_from(&[])?,
        SexpType::Real => session.alloc_real_from(&[])?,
        SexpType::Cplx => session.alloc_cplx_from(&[])?,
        SexpType::Str => session.alloc_str_from(&[])?,
        SexpType::Vec => session.alloc_list(0)?,
        SexpType::Raw => session.alloc_raw_from(&[])?,
        _ => return Err(invalid("invalid serialized ALTREP vector type")),
    })
}

fn truncated() -> super::CodecError {
    invalid("truncated XDR stream")
}

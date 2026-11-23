use std::collections::HashMap;
use std::io::BufRead;

use roxide_gc::SexpId;
use roxide_host::Limits;
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, Binding, CharId, EMPTY_ENV, Encoding, GLOBAL_ENV, PromiseState,
    RComplex, RLogical, RSession, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, Sym, ValueApi,
};

use super::{
    ACTIVE_BINDING, ALTREP, ATTR, BASE, BASE_NS, BYTES, EMPTY, GLOBAL, LATIN1, LOCKED_BINDING,
    MISSING, NAMESPACE, NIL, PACKAGE, REF, Registry, S4, TAG, UNBOUND, UTF8, WireResult, invalid,
};

pub(super) fn decode(
    session: &mut RSession,
    bytes: &[u8],
    workspace: bool,
    limits: &Limits,
    registry: &Registry,
) -> WireResult<SexpId> {
    if bytes.len() > limits.max_decompress_bytes {
        return Err(invalid("serialized input exceeds the decompression limit"));
    }
    decode_from(session, Input::Slice(bytes), workspace, limits, registry)
}

pub(super) fn decode_stream(
    session: &mut RSession,
    input: &mut dyn BufRead,
    workspace: bool,
    limits: &Limits,
    registry: &Registry,
) -> WireResult<SexpId> {
    decode_from(session, Input::Stream(input), workspace, limits, registry)
}

fn decode_from(
    session: &mut RSession,
    input: Input<'_>,
    workspace: bool,
    limits: &Limits,
    registry: &Registry,
) -> WireResult<SexpId> {
    let remaining = match &input {
        Input::Slice(bytes) => bytes.len(),
        Input::Stream(_) => limits.max_decompress_bytes,
    };
    let mut reader = Reader {
        input,
        remaining,
        scratch: Vec::new(),
        refs: Vec::new(),
        binding_flags: HashMap::new(),
        env_parents: HashMap::new(),
        version: 0,
        heap_start: session.heap.live_bytes(),
        limits,
        registry,
    };
    let workspace_version = if workspace {
        match reader.take(5)? {
            b"RDX2\n" => Some(2),
            b"RDX3\n" => Some(3),
            _ => return Err(invalid("invalid R workspace header")),
        }
    } else {
        None
    };
    if reader.take(2)? != b"X\n" {
        return Err(invalid("invalid XDR stream header"));
    }
    reader.version = reader.word()?;
    if !matches!(reader.version, 2 | 3) || workspace_version.is_some_and(|v| v != reader.version) {
        return Err(invalid("unsupported serialization version"));
    }
    reader.take(8)?;
    if reader.version == 3 {
        let len = reader.vector_len::<u8>(1)?;
        reader.take(len)?;
    }
    let value = reader.item(session, 0)?;
    reader.check_heap(session)?;
    validate_parents(reader.env_parents)?;
    Ok(value)
}

enum Input<'a> {
    Slice(&'a [u8]),
    Stream(&'a mut dyn BufRead),
}

struct Reader<'a, 'limits> {
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
                Input::Stream(_) => invalid("serialized input exceeds the decompression limit"),
            })?;
        match &mut self.input {
            Input::Slice(bytes) => {
                let (head, rest) = bytes.split_at_checked(n).ok_or_else(truncated)?;
                *bytes = rest;
                Ok(head)
            }
            Input::Stream(input) => {
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
                Ok(&self.scratch)
            }
        }
    }

    fn word(&mut self) -> WireResult<u32> {
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
        if len > self.remaining / width {
            return Err(match self.input {
                Input::Slice(_) => truncated(),
                Input::Stream(_) => invalid("serialized input exceeds the decompression limit"),
            });
        }
        Ok(())
    }

    fn f64(&mut self) -> WireResult<f64> {
        Ok(f64::from_be_bytes(
            self.take(8)?.try_into().map_err(invalid)?,
        ))
    }

    fn numeric<T>(
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

    fn item(&mut self, session: &mut RSession, depth: usize) -> WireResult<SexpId> {
        self.check_depth(depth)?;
        let flags = self.word()?;
        self.item_with_flags(session, flags, depth + 1)
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
        session: &mut RSession,
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
                .alloc_lgl_from(&self.numeric(4, |r| Ok(RLogical::from_raw(r.word()? as i32)))?)?,
            13 => session.alloc_int_from(&self.numeric(4, |r| Ok(r.word()? as i32))?)?,
            14 => session.alloc_real_from(&self.numeric(8, Self::f64)?)?,
            15 => session.alloc_cplx_from(&self.numeric(16, |r| {
                Ok(RComplex {
                    re: r.f64()?,
                    im: r.f64()?,
                })
            })?)?,
            16 => {
                let len = self.vector_len::<CharId>(8)?;
                let mut chars = Vec::new();
                self.reserve_vector(&mut chars, len)?;
                for _ in 0..len {
                    let ch = self.read_char(session, depth)?;
                    chars.try_reserve(1).map_err(invalid)?;
                    chars.push(ch);
                }
                session.alloc_str_from(&chars)?
            }
            19 => {
                let len = self.vector_len::<SexpId>(4)?;
                let mut values = Vec::new();
                self.reserve_vector(&mut values, len)?;
                for _ in 0..len {
                    let value = self.item(session, depth)?;
                    values.try_reserve(1).map_err(invalid)?;
                    values.push(value);
                }
                let list = session.alloc_list(len)?;
                for (i, value) in values.into_iter().enumerate() {
                    session.list_set(list, i, value)?;
                }
                list
            }
            20 => {
                let len = self.vector_len::<SexpId>(4)?;
                let mut exprs = Vec::new();
                self.reserve_vector(&mut exprs, len)?;
                for _ in 0..len {
                    let value = self.item(session, depth)?;
                    exprs.try_reserve(1).map_err(invalid)?;
                    exprs.push(value);
                }
                session.alloc_expr_from(&exprs)?
            }
            21 => return self.bytecode(session, depth),
            24 => {
                let len = self.vector_len::<u8>(1)?;
                session.alloc_raw_from(self.take(len)?)?
            }
            NAMESPACE | PACKAGE => return self.named_environment(session, ty, depth),
            ALTREP if self.version == 3 => return self.altrep(session, flags, depth),
            _ => return Err(invalid(format!("unsupported serialized R type {ty}"))),
        };
        let attrs = self.optional_attributes(session, flags, depth)?;
        apply_attributes(session, value, attrs, flags)?;
        Ok(value)
    }

    fn read_char(&mut self, session: &mut RSession, depth: usize) -> WireResult<CharId> {
        let flags = self.word()?;
        if flags & 0xff != SexpType::Char as u32 {
            return Err(invalid("invalid serialized character element"));
        }
        self.character(session, flags, depth)
    }

    fn character(
        &mut self,
        session: &mut RSession,
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
            let bytes = self.take(len as usize)?;
            let encoding = if flags & BYTES != 0 {
                Encoding::Bytes
            } else if flags & LATIN1 != 0 {
                Encoding::Latin1
            } else if flags & UTF8 != 0 {
                if std::str::from_utf8(bytes).is_err() {
                    return Err(invalid("invalid UTF-8 in serialized character data"));
                }
                Encoding::Utf8
            } else {
                Encoding::Native
            };
            session.mk_char(bytes, encoding)?
        };
        self.optional_attributes(session, flags, depth)?;
        Ok(ch)
    }

    fn optional_attributes(
        &mut self,
        session: &mut RSession,
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
        session: &mut RSession,
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
            apply_attributes(session, tail, attrs, flags)?;
        }
        Ok(tail)
    }

    fn function(&mut self, session: &mut RSession, flags: u32, depth: usize) -> WireResult<SexpId> {
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
        apply_attributes(session, value, attrs, flags)?;
        Ok(value)
    }

    fn environment(&mut self, session: &mut RSession, depth: usize) -> WireResult<SexpId> {
        let env = session.alloc_env(EMPTY_ENV)?;
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
        apply_attributes(session, env, attrs, 0)?;
        session.env_set_locked(env, locked)?;
        Ok(env)
    }

    fn define_frame(
        &mut self,
        session: &mut RSession,
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
        session: &mut RSession,
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

    fn primitive(&mut self, session: &mut RSession, ty: u32) -> WireResult<SexpId> {
        let len = self.vector_len::<u8>(1)?;
        let name = std::str::from_utf8(self.take(len)?).map_err(invalid)?;
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

    fn altrep(&mut self, session: &mut RSession, flags: u32, depth: usize) -> WireResult<SexpId> {
        let class = self.item(session, depth)?;
        let state = self.item(session, depth)?;
        let attrs = self.item(session, depth)?;
        let (class, rest, _) = session.cons_parts(class)?;
        let (package, rest, _) = session.cons_parts(rest)?;
        let (kind, end, _) = session.cons_parts(rest)?;
        if end != SEXP_NIL || session.sym_name_of(package)? != "base" {
            return Err(invalid("unsupported serialized ALTREP class"));
        }
        let kind = session.int(kind)?;
        let state = session.real(state)?;
        if kind.len() != 1 || state.len() != 3 {
            return Err(invalid("invalid serialized ALTREP state"));
        }
        let (len, start, step) = (state.get(0), state.get(1), state.get(2));
        let (ty, int) = match (session.sym_name_of(class)?, kind.get(0)) {
            ("compact_intseq", 13) => (size_of::<i32>(), true),
            ("compact_realseq", 14) => (size_of::<f64>(), false),
            (name, _) => {
                return Err(invalid(format!(
                    "unsupported serialized ALTREP class '{name}'"
                )));
            }
        };
        if !len.is_finite()
            || len < 0.0
            || len.fract() != 0.0
            || len > (self.limits.max_vector_bytes / ty) as f64
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
        apply_attributes(session, value, attrs, flags)?;
        Ok(value)
    }

    fn bytecode(&mut self, session: &mut RSession, depth: usize) -> WireResult<SexpId> {
        let count = self.vector_len::<Option<SexpId>>(1)?;
        let mut refs = Vec::new();
        refs.try_reserve_exact(count).map_err(invalid)?;
        refs.resize(count, None);
        self.bytecode_body(session, &mut refs, depth)
    }

    fn bytecode_body(
        &mut self,
        session: &mut RSession,
        refs: &mut [Option<SexpId>],
        depth: usize,
    ) -> WireResult<SexpId> {
        self.check_depth(depth)?;
        let code = self.item(session, depth)?;
        if session.type_of(code)? != SexpType::Int {
            return Err(invalid("invalid serialized bytecode instructions"));
        }
        let count = self.vector_len::<SexpId>(4)?;
        if count == 0 {
            return Err(invalid("serialized bytecode has no source expression"));
        }
        // The first constant is the source expression. Read the rest to keep the reference table in order.
        let source = self.bytecode_constant(session, refs, depth + 1)?;
        for _ in 1..count {
            self.bytecode_constant(session, refs, depth + 1)?;
        }
        Ok(source)
    }

    fn bytecode_constant(
        &mut self,
        session: &mut RSession,
        refs: &mut [Option<SexpId>],
        depth: usize,
    ) -> WireResult<SexpId> {
        self.check_depth(depth)?;
        let kind = self.word()?;
        let (kind, index) = match kind {
            243 => {
                let i = self.word()? as usize;
                return refs
                    .get(i)
                    .copied()
                    .flatten()
                    .ok_or_else(|| invalid("invalid bytecode reference"));
            }
            244 => {
                let i = self.word()? as usize;
                if !matches!(refs.get(i), Some(None)) {
                    return Err(invalid("invalid bytecode repetition index"));
                }
                (self.word()?, Some(i))
            }
            _ => (kind, None),
        };
        let value = match kind {
            21 => self.bytecode_body(session, refs, depth + 1)?,
            2 | 6 | 239 | 240 => {
                let attrs = if matches!(kind, 239 | 240) {
                    self.item(session, depth + 1)?
                } else {
                    SEXP_NIL
                };
                let tag = self.item(session, depth + 1)?;
                let car = self.bytecode_constant(session, refs, depth + 1)?;
                let tail = self.bytecode_constant(session, refs, depth + 1)?;
                let value = if matches!(kind, 6 | 240) {
                    session.lcons(car, tail, tag)?
                } else {
                    session.cons(car, tail, tag)?
                };
                apply_attributes(session, value, attrs, 0)?;
                value
            }
            _ => self.item(session, depth + 1)?,
        };
        if let Some(i) = index {
            refs[i] = Some(value);
        }
        Ok(value)
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

fn apply_attributes(
    session: &mut RSession,
    id: SexpId,
    mut attrs: SexpId,
    flags: u32,
) -> WireResult<()> {
    let mut entries = Vec::new();
    while attrs != SEXP_NIL {
        if session.type_of(attrs)? != SexpType::List {
            return Err(invalid("invalid serialized attribute list"));
        }
        let (value, next, name) = session.cons_parts(attrs)?;
        let name = symbol(session, name)?;
        if session.sym_name(name)? == "names"
            && value != SEXP_NIL
            && session.str_slice(value)?.len() != session.xlength(id)?
        {
            return Err(invalid("invalid serialized names attribute length"));
        }
        entries.try_reserve(1).map_err(invalid)?;
        entries.push((name, value));
        attrs = next;
    }
    if !entries.is_empty() {
        session.replace_attributes(id, entries)?;
    }
    if flags & S4 != 0 {
        session.set_s4_flag(id, true)?;
    }
    Ok(())
}

fn truncated() -> super::CodecError {
    invalid("truncated XDR stream")
}

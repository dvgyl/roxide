use std::collections::HashMap;

use roxide_gc::SexpId;
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, Binding, CharId, EMPTY_ENV, Encoding, PromiseState, RSession,
    SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, Sym, ValueApi,
};

use super::super::{ACTIVE_BINDING, LOCKED_BINDING, NAMESPACE, PACKAGE, Registry, S4, TAG};
use super::{CodecSession, Reader, WireResult, invalid};

impl Reader<'_, '_> {
    pub(super) fn cons_chain(
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
        session.root(tail)?;
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

    pub(super) fn function(
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

    pub(super) fn environment(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        depth: usize,
    ) -> WireResult<SexpId> {
        let env = session.alloc_env(EMPTY_ENV)?;
        session.root(env)?;
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

    pub(super) fn named_environment(
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

    pub(super) fn primitive(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        ty: u32,
    ) -> WireResult<SexpId> {
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
}

pub(super) fn validate_parents(
    mut parents: HashMap<SexpId, (SexpId, Option<SexpId>)>,
) -> WireResult<()> {
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

pub(super) fn char_text(session: &RSession, ch: CharId) -> WireResult<String> {
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

pub(super) fn symbol(session: &mut RSession, id: SexpId) -> WireResult<Sym> {
    if matches!(id, SEXP_MISSING | SEXP_UNBOUND) {
        return Err(invalid("invalid serialized symbol"));
    }
    let name = session.sym_name_of(id)?.to_owned();
    if name.is_empty() {
        return Err(invalid("empty serialized symbol name"));
    }
    Ok(session.intern(&name)?)
}

pub(super) fn attribute_entries(
    session: &mut RSession,
    mut attrs: SexpId,
) -> WireResult<Vec<(Sym, SexpId)>> {
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

pub(super) fn set_attributes(
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

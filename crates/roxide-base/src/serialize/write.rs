use std::collections::HashMap;

use roxide_gc::SexpId;
use roxide_host::Limits;
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, CharId, EMPTY_ENV, Elem, Encoding, GLOBAL_ENV, PromiseState,
    RSession, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, Sym, ValueApi, ValueError, VecView,
};

use super::{
    ACTIVE_BINDING, ALTREP, ASCII, ATTR, BASE, BASE_NS, BYTES, EMPTY, GLOBAL, LATIN1,
    LOCKED_BINDING, MAX_INLINE_REF, MISSING, NAMESPACE, NIL, OBJECT, PACKAGE, REF, Registry, S4,
    TAG, UNBOUND, UTF8, WireResult, invalid,
};

pub(super) fn encode(
    session: &mut RSession,
    value: SexpId,
    version: u32,
    workspace: bool,
    limits: &Limits,
    registry: &Registry,
) -> WireResult<Vec<u8>> {
    let mut writer = Writer {
        bytes: Vec::new(),
        refs: HashMap::new(),
        registry,
        version,
        limits,
    };
    if workspace {
        writer.push(if version == 2 { b"RDX2\n" } else { b"RDX3\n" })?;
    }
    writer.push(b"X\n")?;
    writer.word(version)?;
    writer.word(0x0004_0602)?;
    writer.word(if version == 2 {
        0x0002_0300
    } else {
        0x0003_0500
    })?;
    if version == 3 {
        writer.word(5)?;
        writer.push(b"UTF-8")?;
    }
    writer.item(session, value, 0)?;
    Ok(writer.bytes)
}

struct Writer<'a> {
    bytes: Vec<u8>,
    refs: HashMap<SexpId, u32>,
    registry: &'a Registry,
    version: u32,
    limits: &'a Limits,
}

impl Writer<'_> {
    fn push(&mut self, bytes: &[u8]) -> WireResult<()> {
        let limit = self
            .limits
            .max_decompress_bytes
            .min(self.limits.max_vector_bytes);
        if bytes.len() > limit - self.bytes.len() {
            return Err(invalid("serialized output exceeds the byte limit"));
        }
        self.bytes.try_reserve(bytes.len()).map_err(invalid)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn word(&mut self, word: u32) -> WireResult<()> {
        self.push(&word.to_be_bytes())
    }

    fn length(&mut self, len: usize) -> WireResult<()> {
        if len <= i32::MAX as usize {
            self.word(len as u32)
        } else {
            self.word(u32::MAX)?;
            self.word(((len as u64) >> 32) as u32)?;
            self.word(len as u32)
        }
    }

    fn reference(&mut self, id: SexpId) -> WireResult<bool> {
        if let Some(&index) = self.refs.get(&id) {
            if index <= MAX_INLINE_REF {
                self.word((index << 8) | REF)?;
            } else {
                self.word(REF)?;
                self.word(index)?;
            }
            Ok(true)
        } else {
            let index = u32::try_from(self.refs.len() + 1).map_err(invalid)?;
            self.refs.try_reserve(1).map_err(invalid)?;
            self.refs.insert(id, index);
            Ok(false)
        }
    }

    fn item(&mut self, session: &mut RSession, mut id: SexpId, depth: usize) -> WireResult<()> {
        if depth > self.limits.max_expressions {
            return Err(invalid("serialized object exceeds the nesting limit"));
        }
        loop {
            let singleton = match id {
                SEXP_NIL => NIL,
                GLOBAL_ENV => GLOBAL,
                SEXP_UNBOUND => UNBOUND,
                SEXP_MISSING => MISSING,
                BASE_NAMESPACE => BASE_NS,
                EMPTY_ENV => EMPTY,
                BASE_ENV => BASE,
                _ => 0,
            };
            if singleton != 0 {
                return self.word(singleton);
            }
            let ty = session.type_of(id)?;
            if matches!(ty, SexpType::Sym | SexpType::Env) && self.reference(id)? {
                return Ok(());
            }
            if ty == SexpType::Sym {
                self.word(ty as u32)?;
                let name = session.sym_name_of(id)?;
                let encoding = if name.is_ascii() {
                    Encoding::Native
                } else {
                    Encoding::Utf8
                };
                return self.string(name.as_bytes(), encoding);
            }

            let attrs = session.attrs(id)?;
            if ty == SexpType::Env {
                for &(name, namespace, attached) in &self.registry.packages {
                    if id == namespace {
                        self.word(NAMESPACE)?;
                        self.word(0)?;
                        self.word(2)?;
                        self.string(name.as_bytes(), Encoding::Native)?;
                        return self.string(b"4.6.2", Encoding::Native);
                    }
                    if Some(id) == attached {
                        self.word(PACKAGE)?;
                        self.word(0)?;
                        self.word(1)?;
                        return self.string(format!("package:{name}").as_bytes(), Encoding::Native);
                    }
                }
                return self.environment(session, id, &attrs, depth + 1);
            }
            let object = if session.is_object(id)? { OBJECT } else { 0 };
            let s4 = if session.is_s4(id)? { S4 } else { 0 };
            let flags = ty as u32 | object | s4 | if attrs.is_empty() { 0 } else { ATTR };
            match ty {
                SexpType::List | SexpType::Lang | SexpType::Dot => {
                    let (car, cdr, tag) = session.cons_parts(id)?;
                    self.word(flags | if tag == SEXP_NIL { 0 } else { TAG })?;
                    if !attrs.is_empty() {
                        self.attributes(session, &attrs, depth + 1)?;
                    }
                    if tag != SEXP_NIL {
                        self.item(session, tag, depth + 1)?;
                    }
                    self.item(session, car, depth + 1)?;
                    id = cdr;
                }
                SexpType::Clo | SexpType::Prom => {
                    let (tag, car, cdr) = if ty == SexpType::Clo {
                        let (formals, body, env) = session.closure_parts(id)?;
                        (env, formals, body)
                    } else {
                        let (expr, env, value, state) = session.promise_parts(id)?;
                        let forced = state == PromiseState::Forced;
                        (
                            if forced { SEXP_NIL } else { env },
                            if forced { value } else { SEXP_UNBOUND },
                            expr,
                        )
                    };
                    self.word(flags | if tag == SEXP_NIL { 0 } else { TAG })?;
                    if !attrs.is_empty() {
                        self.attributes(session, &attrs, depth + 1)?;
                    }
                    if tag != SEXP_NIL {
                        self.item(session, tag, depth + 1)?;
                    }
                    self.item(session, car, depth + 1)?;
                    return self.item(session, cdr, depth + 1);
                }
                _ => return self.leaf(session, id, ty, flags, &attrs, depth + 1),
            }
        }
    }

    fn leaf(
        &mut self,
        session: &mut RSession,
        id: SexpId,
        ty: SexpType,
        flags: u32,
        attrs: &[(Sym, SexpId)],
        depth: usize,
    ) -> WireResult<()> {
        if self.version == 3 && attrs.is_empty() {
            let sequence = match ty {
                SexpType::Int => compact(session.int(id)?, "compact_intseq", f64::from),
                SexpType::Real => compact(session.real(id)?, "compact_realseq", |x| x),
                _ => None,
            };
            if let Some((class, state)) = sequence {
                return self.altrep(session, class, ty, state, depth);
            }
        }
        self.word(flags)?;
        match ty {
            SexpType::Lgl => self.numeric(session.lgl(id)?, |x| x.raw().to_be_bytes())?,
            SexpType::Int => self.numeric(session.int(id)?, i32::to_be_bytes)?,
            SexpType::Real => self.numeric(session.real(id)?, f64::to_be_bytes)?,
            SexpType::Cplx => {
                let view = session.cplx(id)?;
                self.length(view.len())?;
                for x in view.iter() {
                    self.push(&x.re.to_be_bytes())?;
                    self.push(&x.im.to_be_bytes())?;
                }
            }
            SexpType::Raw => {
                let raw = session.raw(id)?;
                let bytes = raw
                    .as_slice()
                    .ok_or_else(|| invalid("cannot access raw vector bytes"))?;
                self.length(bytes.len())?;
                self.push(bytes)?;
            }
            SexpType::Str => {
                let chars = session.str_slice(id)?;
                self.length(chars.len())?;
                for &ch in chars {
                    self.character(session, ch)?;
                }
            }
            SexpType::Vec | SexpType::Expr => {
                let len = session.list_slice(id)?.len();
                self.length(len)?;
                for i in 0..len {
                    let child = session.list_slice(id)?[i];
                    self.item(session, child, depth)?;
                }
            }
            SexpType::Special | SexpType::Builtin => {
                let (prim, _) = session.prim_parts(id)?;
                let name = self
                    .registry
                    .primitives
                    .get(&prim)
                    .ok_or_else(|| invalid("cannot write an unregistered primitive"))?
                    .as_bytes();
                self.length(name.len())?;
                self.push(name)?;
            }
            SexpType::Obj => {}
            _ => {
                return Err(invalid(format!(
                    "cannot serialize R type {}",
                    ty.type_name(false)
                )));
            }
        }
        if !attrs.is_empty() {
            self.attributes(session, attrs, depth)?;
        }
        Ok(())
    }

    fn numeric<T: Elem, const N: usize>(
        &mut self,
        vector: VecView<'_, T>,
        bytes: impl Fn(T) -> [u8; N],
    ) -> WireResult<()> {
        self.length(vector.len())?;
        for elt in vector.iter() {
            self.push(&bytes(elt))?;
        }
        Ok(())
    }

    fn string(&mut self, bytes: &[u8], encoding: Encoding) -> WireResult<()> {
        let flags = match encoding {
            Encoding::Native if bytes.is_ascii() => ASCII,
            Encoding::Native => 0,
            Encoding::Utf8 => UTF8,
            Encoding::Latin1 => LATIN1,
            Encoding::Bytes => BYTES | if bytes.is_ascii() { ASCII } else { 0 },
        };
        self.word(SexpType::Char as u32 | flags)?;
        self.word(i32::try_from(bytes.len()).map_err(invalid)? as u32)?;
        self.push(bytes)
    }

    fn character(&mut self, session: &RSession, ch: CharId) -> WireResult<()> {
        if ch == CharId::NA {
            self.word(SexpType::Char as u32)?;
            return self.word(u32::MAX);
        }
        self.string(session.char_bytes(ch)?, session.char_encoding(ch)?)
    }

    fn attributes(
        &mut self,
        session: &mut RSession,
        attrs: &[(Sym, SexpId)],
        depth: usize,
    ) -> WireResult<()> {
        for &(tag, value) in attrs {
            self.word(SexpType::List as u32 | TAG)?;
            self.item(session, tag.as_sexp(), depth)?;
            self.item(session, value, depth)?;
        }
        self.word(NIL)
    }

    fn environment(
        &mut self,
        session: &mut RSession,
        env: SexpId,
        attrs: &[(Sym, SexpId)],
        depth: usize,
    ) -> WireResult<()> {
        let locked = frame_locked(session, env)?;
        self.word(SexpType::Env as u32)?;
        self.word(u32::from(locked))?;
        self.item(session, session.env_enclos(env)?, depth)?;
        // R stores non-hashed bindings in reverse insertion order.
        for (tag, binding) in session.env_entries(env)?.into_iter().rev() {
            let locked = if binding.locked { LOCKED_BINDING } else { 0 };
            let active = if binding.active { ACTIVE_BINDING } else { 0 };
            self.word(
                SexpType::List as u32 | TAG | locked | active | (u32::from(binding.missing) << 12),
            )?;
            self.item(session, tag.as_sexp(), depth)?;
            self.item(session, binding.value, depth)?;
        }
        self.word(NIL)?;
        self.word(NIL)?;
        self.attributes(session, attrs, depth)
    }

    fn altrep(
        &mut self,
        session: &mut RSession,
        class: &str,
        ty: SexpType,
        [len, start, step]: [f64; 3],
        depth: usize,
    ) -> WireResult<()> {
        self.word(ALTREP)?;
        for name in [class, "base"] {
            self.word(SexpType::List as u32)?;
            let sym = session.intern(name)?;
            self.item(session, sym.as_sexp(), depth)?;
        }
        self.word(SexpType::List as u32)?;
        self.word(SexpType::Int as u32)?;
        self.word(1)?;
        self.word(ty as u32)?;
        self.word(NIL)?;
        self.word(SexpType::Real as u32)?;
        self.word(3)?;
        for elt in [len, start, step] {
            self.push(&elt.to_be_bytes())?;
        }
        self.word(NIL)
    }
}

fn compact<T: Elem>(
    view: VecView<'_, T>,
    name: &'static str,
    real: impl Fn(T) -> f64,
) -> Option<(&'static str, [f64; 3])> {
    let VecView::Alt { class, len, .. } = &view else {
        return None;
    };
    if *len < 2 || class.class_name() != name || class.package_name() != "base" {
        return None;
    }
    let start = real(view.get(0));
    let step = if real(view.get(*len - 1)) < start {
        -1.0
    } else {
        1.0
    };
    Some((name, [*len as f64, start, step]))
}

fn frame_locked(session: &mut RSession, env: SexpId) -> WireResult<bool> {
    // The value API has no lock getter. An absent name tests the frame lock.
    // Remove the temporary binding before any evaluation or garbage collection.
    let mut name = session.intern("")?;
    while session.env_get(env, name).is_some() {
        let next = format!("{}.", session.sym_name(name)?);
        name = session.intern(&next)?;
    }
    match session.env_define(env, name, SEXP_NIL) {
        Ok(()) => {
            session.env_remove(env, name)?;
            Ok(false)
        }
        Err(ValueError::LockedBinding(_)) => Ok(true),
        Err(err) => Err(err.into()),
    }
}

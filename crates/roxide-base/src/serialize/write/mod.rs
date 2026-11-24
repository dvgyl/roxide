use std::collections::HashMap;

use roxide_gc::SexpId;
use roxide_host::Limits;
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, CharId, EMPTY_ENV, Elem, Encoding, GLOBAL_ENV, NA_INTEGER,
    PromiseState, RSession, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, Sym, ValueApi, VecView,
    is_na_real,
};

use super::{
    ASCII, ATTR, BASE, BASE_NS, BYTES, EMPTY, Format, GLOBAL, LATIN1, MAX_INLINE_REF, MISSING,
    NAMESPACE, NIL, OBJECT, PACKAGE, PERSIST, REF, Registry, S4, TAG, UNBOUND, UTF8, WireResult,
    invalid,
};

use super::session::CodecSession;

mod altrep;
mod environment;

use altrep::compact;
use environment::namespace_version;

pub(super) fn encode(
    session: &mut CodecSession<'_, '_>,
    value: SexpId,
    Format {
        version,
        workspace,
        ascii,
    }: Format,
    limits: &Limits,
    registry: &Registry,
) -> WireResult<Vec<u8>> {
    session.root(value)?;
    if !matches!(version, 2 | 3) {
        return Err(invalid("unsupported serialization version"));
    }
    let mut writer = Writer {
        bytes: Vec::new(),
        refs: HashMap::new(),
        reference_count: 0,
        ascii,
        registry,
        version,
        limits,
    };
    if workspace {
        writer.push(match (ascii, version) {
            (true, 2) => b"RDA2\n",
            (true, _) => b"RDA3\n",
            (false, 2) => b"RDX2\n",
            (false, _) => b"RDX3\n",
        })?;
    }
    writer.push(if ascii { b"A\n" } else { b"X\n" })?;
    writer.word(version)?;
    writer.word(0x0004_0602)?;
    writer.word(if version == 2 {
        0x0002_0300
    } else {
        0x0003_0500
    })?;
    if version == 3 {
        writer.word(5)?;
        writer.string_bytes(b"UTF-8")?;
    }
    writer.item(session, value, 0)?;
    Ok(writer.bytes)
}

struct Writer<'a> {
    ascii: bool,
    bytes: Vec<u8>,
    refs: HashMap<SexpId, u32>,
    reference_count: u32,
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
        if self.ascii {
            let n = word as i32;
            if n == NA_INTEGER {
                return self.push(b"NA\n");
            }
            return self.push(format!("{n}\n").as_bytes());
        }
        self.push(&word.to_be_bytes())
    }

    fn real(&mut self, value: f64) -> WireResult<()> {
        if self.ascii {
            return match value {
                x if is_na_real(x) => self.push(b"NA\n"),
                x if x.is_nan() => self.push(b"NaN\n"),
                f64::INFINITY => self.push(b"Inf\n"),
                f64::NEG_INFINITY => self.push(b"-Inf\n"),
                x => self.push(super::ascii::format_real(x)?.as_bytes()),
            };
        }
        self.push(&value.to_be_bytes())
    }

    fn string_bytes(&mut self, bytes: &[u8]) -> WireResult<()> {
        if self.ascii {
            for &byte in bytes {
                if let Some(escape) = super::ascii::escape(byte) {
                    self.push(&[b'\\', escape])?;
                } else if byte.is_ascii_graphic() {
                    self.push(&[byte])?;
                } else {
                    self.push(format!("\\{byte:03o}").as_bytes())?;
                }
            }
            return self.push(b"\n");
        }
        self.push(bytes)
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
            let index = self.next_reference()?;
            self.refs.try_reserve(1).map_err(invalid)?;
            self.refs.insert(id, index);
            Ok(false)
        }
    }

    fn next_reference(&mut self) -> WireResult<u32> {
        self.reference_count = self
            .reference_count
            .checked_add(1)
            .ok_or_else(|| invalid("serialization reference table exceeds the size limit"))?;
        Ok(self.reference_count)
    }

    fn persistent(&mut self, session: &RSession, key: SexpId) -> WireResult<()> {
        let chars = session.str_slice(key)?;
        if chars.is_empty() {
            return Err(invalid("empty persistent reference key"));
        }
        self.next_reference()?;
        self.word(PERSIST)?;
        self.word(0)?;
        self.word(i32::try_from(chars.len()).map_err(invalid)? as u32)?;
        for &ch in chars {
            self.character(session, ch)?;
        }
        Ok(())
    }

    fn item(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        mut id: SexpId,
        depth: usize,
    ) -> WireResult<()> {
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
            let foreign = session.foreign_alt_seen() && session.foreign_alt(id)?.is_some();
            if foreign && self.write_foreign(session, id, ty, depth + 1)? {
                return Ok(());
            }
            let registry = self.registry;
            let registered = if ty == SexpType::Env {
                registry
                    .packages
                    .iter()
                    .find(|(_, ns, attached)| *ns == id || *attached == Some(id))
            } else {
                None
            };
            if registered.is_none()
                && matches!(ty, SexpType::Env | SexpType::ExtPtr | SexpType::WeakRef)
                && let Some(key) = session.persist(id)?
            {
                session.root(key)?;
                return self.persistent(session, key);
            }
            if matches!(ty, SexpType::Sym | SexpType::Env | SexpType::ExtPtr) {
                if self.reference(id)? {
                    return Ok(());
                }
                session.root(id)?;
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
            for &(_, value) in &attrs {
                session.root(value)?;
            }
            if ty == SexpType::Env {
                if let Some((name, namespace, _)) = registered {
                    if id == *namespace {
                        self.word(NAMESPACE)?;
                        self.word(0)?;
                        self.word(2)?;
                        self.string(name.as_bytes(), Encoding::Native)?;
                        let version = namespace_version(session, id)?;
                        return self.string(version.as_bytes(), Encoding::Native);
                    }
                    self.word(PACKAGE)?;
                    self.word(0)?;
                    self.word(1)?;
                    return self.string(format!("package:{name}").as_bytes(), Encoding::Native);
                }
                return self.environment(session, id, &attrs, depth + 1);
            }
            let object = if session.is_object(id)? { OBJECT } else { 0 };
            let s4 = if session.is_s4(id)? { S4 } else { 0 };
            let flags = ty as u32 | object | s4 | if attrs.is_empty() { 0 } else { ATTR };
            match ty {
                SexpType::List | SexpType::Lang | SexpType::Dot => {
                    let (car, cdr, tag) = session.cons_parts(id)?;
                    for value in [car, cdr, tag] {
                        session.root(value)?;
                    }
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
                    for value in [car, cdr, tag] {
                        session.root(value)?;
                    }
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
                _ => {
                    if !foreign && self.version >= 3 && attrs.is_empty() {
                        let sequence = match ty {
                            SexpType::Int => compact(session.int(id)?, "compact_intseq", f64::from),
                            SexpType::Real => compact(session.real(id)?, "compact_realseq", |x| x),
                            _ => None,
                        };
                        if let Some((class, state)) = sequence {
                            return self.altrep(session, class, ty, state, depth + 1);
                        }
                    }
                    return self.leaf(session, id, ty, flags, &attrs, depth + 1);
                }
            }
        }
    }

    fn leaf(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        id: SexpId,
        ty: SexpType,
        flags: u32,
        attrs: &[(Sym, SexpId)],
        depth: usize,
    ) -> WireResult<()> {
        self.word(flags)?;
        match ty {
            SexpType::Lgl => {
                self.numeric(session.lgl(id)?, |writer, x| writer.word(x.raw() as u32))?
            }
            SexpType::Int => self.numeric(session.int(id)?, |writer, x| writer.word(x as u32))?,
            SexpType::Real => self.numeric(session.real(id)?, Self::real)?,
            SexpType::Cplx => {
                let view = session.cplx(id)?;
                self.length(view.len())?;
                for x in view.iter() {
                    self.real(x.re)?;
                    self.real(x.im)?;
                }
            }
            SexpType::Raw => {
                let raw = session.raw(id)?;
                let bytes = raw
                    .as_slice()
                    .ok_or_else(|| invalid("cannot access raw vector bytes"))?;
                self.length(bytes.len())?;
                if self.ascii {
                    for byte in bytes {
                        self.push(format!("{byte:02x}\n").as_bytes())?;
                    }
                } else {
                    self.push(bytes)?;
                }
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
                    let child = session.list_get(id, i)?;
                    session.root(child)?;
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
                self.string_bytes(name)?;
            }
            SexpType::ExtPtr => {
                let prot = session.extptr_prot(id)?;
                let tag = session.extptr_tag(id)?;
                session.root(tag)?;
                session.root(prot)?;
                self.item(session, prot, depth)?;
                self.item(session, tag, depth)?;
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

    fn numeric<T: Elem>(
        &mut self,
        vector: VecView<'_, T>,
        write: impl Fn(&mut Self, T) -> WireResult<()>,
    ) -> WireResult<()> {
        self.length(vector.len())?;
        for elt in vector.iter() {
            write(self, elt)?;
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
        self.string_bytes(bytes)
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
        session: &mut CodecSession<'_, '_>,
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
}

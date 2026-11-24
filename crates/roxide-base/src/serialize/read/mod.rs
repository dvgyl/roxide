use std::collections::HashMap;
use std::io::BufRead;

use roxide_gc::SexpId;
use roxide_host::Limits;
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, CharId, EMPTY_ENV, Encoding, GLOBAL_ENV, RComplex, RLogical,
    RSession, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, ValueApi,
};

use super::session::CodecSession;
use super::{
    ALTREP, ATTR, BASE, BASE_NS, BYTES, EMPTY, GLOBAL, LATIN1, MISSING, NAMESPACE, NIL, PACKAGE,
    PERSIST, REF, Registry, S4, UNBOUND, UTF8, WireResult, invalid,
};

mod altrep;
mod bytecode;
mod input;
mod objects;

use bytecode::BCODE;
use input::{Input, StreamInput};
use objects::{attribute_entries, char_text, set_attributes, validate_parents};

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
    let input = Input::Stream(StreamInput::new(input));
    let (value, replay) = decode_from(
        &mut CodecSession::Stream(session),
        input,
        workspace,
        limits,
        registry,
    )?;
    Ok(if let Some(bytes) = replay {
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
) -> WireResult<(SexpId, Option<Vec<u8>>)> {
    let remaining = match &input {
        Input::Slice(bytes) => bytes.len(),
        Input::Stream(..) => limits.max_decompress_bytes,
    };
    let mut reader = Reader {
        input,
        remaining,
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
    if reader.version == 2
        && let Input::Stream(stream) = &mut reader.input
    {
        stream.disable_replay();
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
    let replay = match reader.input {
        Input::Stream(stream) if reader.replay => Some(stream.replay()?),
        _ => None,
    };
    Ok((value, replay))
}

struct Reader<'a, 'limits> {
    ascii: bool,
    replay: bool,
    input: Input<'a>,
    remaining: usize,
    refs: Vec<SexpId>,
    binding_flags: HashMap<SexpId, u32>,
    env_parents: HashMap<SexpId, (SexpId, Option<SexpId>)>,
    version: u32,
    heap_start: usize,
    limits: &'limits Limits,
    registry: &'limits Registry,
}

impl Reader<'_, '_> {
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
        if depth == 0
            && flags & ATTR == 0
            && matches!(flags & 0xff, 10 | 13 | 14 | 15 | 24)
            && let Input::Stream(stream) = &mut self.input
        {
            stream.disable_replay();
        }
        let value = self.item_with_flags(session, flags, depth + 1)?;
        if !matches!(
            flags & 0xff,
            0 | NIL | GLOBAL | BASE | BASE_NS | EMPTY | MISSING | UNBOUND | REF | 1
        ) {
            session.root(value)?;
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
                session.root(ptr)?;
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
            session.root(value)?;
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
                    session.root(CharId::as_sexp(*ch))?;
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
            session.root(ch.as_sexp())?;
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
}

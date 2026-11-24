use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use super::{CodecSession, Reader, WireResult, invalid};

pub(super) const BCODE: u32 = 21;
const BCREPREF: u32 = 243;
const BCREPDEF: u32 = 244;
const ATTRLANG: u32 = 240;
const ATTRLIST: u32 = 239;

impl Reader<'_, '_> {
    pub(super) fn bytecode(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        depth: usize,
    ) -> WireResult<SexpId> {
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
                session.root(value)?;
                self.apply_attributes(session, value, attrs, 0)?;
                Ok(value)
            }
            _ => Err(invalid("invalid bytecode child marker")),
        }
    }
}

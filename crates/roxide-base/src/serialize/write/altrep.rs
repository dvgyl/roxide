use roxide_eval::AltOp;
use roxide_gc::SexpId;
use roxide_value::{Elem, SexpType, Sym, ValueApi, VecView};

use super::super::{ALTREP, NIL, OBJECT, S4};
use super::{CodecSession, WireResult, Writer, invalid};

impl Writer<'_> {
    pub(super) fn write_foreign(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        id: SexpId,
        ty: SexpType,
        depth: usize,
    ) -> WireResult<bool> {
        if self.version >= 3
            && let Some(state) =
                session.eval(|ctx, _| ctx.altrep_method(AltOp::SerializedState(id)))?
        {
            session.root(state)?;
            let (class, package) = session
                .alt_class_names(id)?
                .ok_or_else(|| invalid("missing foreign ALTREP class"))?;
            let flags = ALTREP
                | if session.is_object(id)? { OBJECT } else { 0 }
                | if session.is_s4(id)? { S4 } else { 0 }
                | (u32::from(session.gp(id)?) << 12);
            self.altrep_header(session, flags, [class, package], ty, depth)?;
            let attrs = session.attrs(id)?;
            for &(_, value) in &attrs {
                session.root(value)?;
            }
            self.item(session, state, depth)?;
            self.attributes(session, &attrs, depth)?;
            return Ok(true);
        }
        session.eval(|ctx, _| ctx.resolve_altrep(id))?;
        Ok(false)
    }

    pub(super) fn altrep(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        class: &str,
        ty: SexpType,
        [len, start, step]: [f64; 3],
        depth: usize,
    ) -> WireResult<()> {
        let class = session.intern(class)?;
        let package = session.intern("base")?;
        self.altrep_header(session, ALTREP, [class, package], ty, depth)?;
        self.word(SexpType::Real as u32)?;
        self.word(3)?;
        for elt in [len, start, step] {
            self.real(elt)?;
        }
        self.word(NIL)
    }

    pub(super) fn altrep_header(
        &mut self,
        session: &mut CodecSession<'_, '_>,
        flags: u32,
        names: [Sym; 2],
        ty: SexpType,
        depth: usize,
    ) -> WireResult<()> {
        self.word(flags)?;
        for name in names {
            self.word(SexpType::List as u32)?;
            self.item(session, name.as_sexp(), depth)?;
        }
        self.word(SexpType::List as u32)?;
        self.word(SexpType::Int as u32)?;
        self.word(1)?;
        self.word(ty as u32)?;
        self.word(NIL)
    }
}

pub(super) fn compact<T: Elem>(
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

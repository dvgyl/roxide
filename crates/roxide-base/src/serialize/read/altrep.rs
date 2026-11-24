use roxide_eval::{AltOp, Unwind};
use roxide_gc::SexpId;
use roxide_value::{RSession, SEXP_NIL, SexpType, ValueApi};

use super::super::OBJECT;
use super::objects::symbol;
use super::{CodecSession, Reader, WireResult, attribute_entries, invalid, set_attributes};

impl Reader<'_, '_> {
    pub(super) fn altrep(
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
        if let Some(base_type) = builtin {
            if ty != base_type {
                return Err(invalid("invalid serialized ALTREP vector type"));
            }
            let value = if class_name.starts_with("wrap_") {
                let (target, metadata, _) = session.cons_parts(state)?;
                if session.type_of(target)? != ty || session.int(metadata)?.len() != 2 {
                    return Err(invalid("invalid serialized ALTREP wrapper state"));
                }
                session.alloc_alt_wrapper(target)?
            } else {
                self.compact_sequence(session, ty, state)?
            };
            set_attributes(session, value, entries, flags)?;
            session.set_object_flag(value, flags & OBJECT != 0)?;
            session.set_gp(value, ((flags >> 12) & 0xffff) as u16)?;
            return Ok(value);
        }

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
            ctx.with_native_error_handler(
                |ctx| ctx.load_namespace(&package_name, call),
                |_, _| Ok(SEXP_NIL),
            )?;
            if let Some(value) = restore_altrep(ctx, op)? {
                return Ok(Some(value));
            }
            ctx.warn_now(call, format!(
                "cannot unserialize ALTVEC object of class '{class_name}' from package '{package_name}'; returning length zero vector"
            ))?;
            Ok(None)
        })?;
        if let Some(value) = value {
            return Ok(value);
        }
        empty_vector(session, ty)
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

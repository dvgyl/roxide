use std::ffi::c_int;

use roxide_eval::{AltOp, AltReply, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{AltNeed, SEXP_NIL, SexpType, ValueApi};

use super::payload::{Access, materialize};
use super::registry::{class, class_error, foreign, latest, lazy_methods};
use super::vector::{c_flags, optional_value, protect};
use crate::abi::{Rboolean, Sexp};
use crate::api::memory::{release_transient_frame, save_transient_frame};
use crate::api::pairlist::cells;
use crate::api::vector::{alloc_vector, checked_len};

fn method_length(ctx: &mut Ctx, x: SexpId) -> EvalResult<usize> {
    let info = foreign(ctx, x)?;
    if let Some(len) = info.len {
        return Ok(len);
    }
    let methods = class(ctx, info.class)?.methods;
    let len = methods
        .length(ctx, Sexp::from_id(x))?
        .ok_or_else(|| class_error(ctx, x, "no ALTREP Length method defined"))?;
    checked_len(ctx, len)
}

pub(super) fn ensure_length(ctx: &mut Ctx, x: SexpId) -> EvalResult<usize> {
    let len = method_length(ctx, x)?;
    ctx.session_mut().seal_foreign_alt_len(x, len)?;
    Ok(ctx.session().xlength(x)?)
}

pub(crate) fn resolve_for_boundary(ctx: &mut Ctx, x: SexpId, need: AltNeed) -> EvalResult<()> {
    match need {
        AltNeed::Length => ensure_length(ctx, x).map(|_| ()),
        AltNeed::Payload => materialize(ctx, x, Access::Evaluate),
    }
}

fn with_transient_frame<T>(
    ctx: &mut Ctx,
    f: impl FnOnce(&mut Ctx) -> EvalResult<T>,
) -> EvalResult<T> {
    if crate::cx::depth() != 0 {
        return f(ctx);
    }
    save_transient_frame(ctx);
    let result = f(ctx);
    release_transient_frame(ctx);
    result
}

pub(crate) fn dispatch(ctx: &mut Ctx, op: AltOp) -> EvalResult<AltReply> {
    with_transient_frame(ctx, |ctx| dispatch_method(ctx, op))
}

fn dispatch_method(ctx: &mut Ctx, op: AltOp) -> EvalResult<AltReply> {
    let x = match op {
        AltOp::Length(x) => return method_length(ctx, x).map(AltReply::Len),
        AltOp::Materialize(x) => {
            return materialize(ctx, x, Access::Evaluate).map(|()| AltReply::Unit);
        }
        AltOp::IsSorted(x) => {
            return Ok(AltReply::Flag(c_flags(ctx, x, true)?.unwrap_or(c_int::MIN)));
        }
        AltOp::NoNa(x) => return Ok(AltReply::Flag(c_flags(ctx, x, false)?.unwrap_or(0))),
        AltOp::Unserialize { .. } => return unserialize(ctx, op).map(AltReply::Value),
        AltOp::SerializedState(x) => {
            let info = foreign(ctx, x)?;
            let result = class(ctx, info.class)?
                .methods
                .serialized_state(ctx, Sexp::from_id(x))?;
            return optional_value(ctx, result).map(AltReply::Value);
        }
        AltOp::Coerce { x, .. }
        | AltOp::ExtractSubset { x, .. }
        | AltOp::Sum { x, .. }
        | AltOp::Min { x, .. }
        | AltOp::Max { x, .. } => x,
    };
    let ty = ctx.session().type_of(x)?;
    let Some(methods) = lazy_methods(ctx, x)? else {
        return Ok(AltReply::Value(None));
    };
    let x = Sexp::from_id(x);
    let result = match op {
        AltOp::Coerce { to, .. } => methods.coerce(ctx, x, to as c_int)?,
        AltOp::ExtractSubset { indx, call, .. } => {
            methods.extract_subset(ctx, x, Sexp::from_id(indx), Sexp::from_id(call))?
        }
        AltOp::Sum { na_rm, .. } => {
            let flag = Rboolean::from(na_rm);
            match ty {
                SexpType::Int => methods.int_sum(ctx, x, flag)?,
                SexpType::Real => methods.real_sum(ctx, x, flag)?,
                SexpType::Lgl => methods.lgl_sum(ctx, x, flag)?,
                _ => None,
            }
        }
        AltOp::Min { na_rm, .. } => {
            let flag = Rboolean::from(na_rm);
            match ty {
                SexpType::Int => methods.int_min(ctx, x, flag)?,
                SexpType::Real => methods.real_min(ctx, x, flag)?,
                _ => None,
            }
        }
        AltOp::Max { na_rm, .. } => {
            let flag = Rboolean::from(na_rm);
            match ty {
                SexpType::Int => methods.int_max(ctx, x, flag)?,
                SexpType::Real => methods.real_max(ctx, x, flag)?,
                _ => None,
            }
        }
        _ => None,
    };
    optional_value(ctx, result).map(AltReply::Value)
}

fn unserialize(ctx: &mut Ctx, op: AltOp) -> EvalResult<Option<SexpId>> {
    let AltOp::Unserialize {
        class,
        package,
        ty,
        state,
        attrs,
        objf,
        levels,
    } = op
    else {
        return Ok(None);
    };
    let mut registered = latest(ctx, class, package);
    if registered.is_none() {
        let pkg = ctx.session().sym_name(package)?.to_owned();
        ctx.with_native_error_handler(
            |ctx| ctx.load_namespace(&pkg, SEXP_NIL),
            |_, _| Ok(SEXP_NIL),
        )?;
        registered = latest(ctx, class, package);
    }
    let Some(class_info) = registered else {
        let s = ctx.session();
        let name = s.sym_name(class)?;
        let pkg = s.sym_name(package)?;
        let message = format!(
            "cannot unserialize ALTVEC object of class '{name}' from package '{pkg}'; returning length zero vector"
        );
        ctx.warning_at(SEXP_NIL, &message)?;
        return alloc_vector(ctx, ty, 0).map(Some);
    };
    ctx.scoped(|ctx| {
        let kind = ctx.session_mut().scalar_int(ty as c_int)?;
        protect(ctx, kind)?;
        let type_cell = ctx.session_mut().cons(kind, SEXP_NIL, SEXP_NIL)?;
        protect(ctx, type_cell)?;
        let package_cell = ctx
            .session_mut()
            .cons(package.as_sexp(), type_cell, SEXP_NIL)?;
        protect(ctx, package_cell)?;
        let info = ctx
            .session_mut()
            .cons(class.as_sexp(), package_cell, SEXP_NIL)?;
        protect(ctx, info)?;
        let info = Sexp::from_id(info);
        let state = Sexp::from_id(state);
        let methods = class_info.methods;
        if let Some(result) =
            methods.unserialize_ex(ctx, info, state, Sexp::from_id(attrs), objf, levels)?
        {
            return optional_value(ctx, Some(result));
        }
        let result = methods.unserialize(ctx, info, state)?;
        let Some(result) = optional_value(ctx, result)? else {
            return Err(ctx.stop(SEXP_NIL, "ALTREP class has no usable Unserialize method"));
        };
        protect(ctx, result)?;
        let attributes = cells(ctx, attrs)?;
        let attributes = attributes
            .into_iter()
            .map(|(_, value, tag)| Ok((ctx.session().sexp_as_sym(tag)?, value)))
            .collect::<EvalResult<_>>()?;
        let s = ctx.session_mut();
        s.replace_attributes(result, attributes)?;
        s.set_object_flag(result, objf != 0)?;
        s.set_gp(result, levels as u16)?;
        Ok(Some(result))
    })
}

/// Call the registered Inspect method for a lazy foreign ALTREP vector.
///
/// Return `None` if the vector has no applicable Inspect method.
///
/// # Safety
///
/// If supplied, `subtree` must accept the C callback arguments and obey the
/// R C API rules for protection and non-local jumps.
pub unsafe fn inspect(
    ctx: &mut Ctx,
    x: SexpId,
    pre: c_int,
    deep: c_int,
    pvec: c_int,
    subtree: Option<unsafe extern "C" fn(Sexp, c_int, c_int, c_int)>,
) -> EvalResult<Option<Rboolean>> {
    let Some(methods) = lazy_methods(ctx, x)? else {
        return Ok(None);
    };
    ctx.scoped(|ctx| {
        protect(ctx, x)?;
        with_transient_frame(ctx, |ctx| {
            methods.inspect(ctx, Sexp::from_id(x), pre, deep, pvec, subtree)
        })
    })
}

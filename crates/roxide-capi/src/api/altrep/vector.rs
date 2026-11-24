use std::ffi::{c_int, c_void};
use std::ptr;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SYM_NAMES, SexpType, ValueApi};

use super::payload::{Access, materialize};
use super::registry::{class_error, lazy_methods};
use crate::abi::{R_xlen_t, Rboolean, Sexp};
use crate::api::vector::{id, index, require_type};

pub(super) fn protect(ctx: &mut Ctx, x: SexpId) -> EvalResult<()> {
    ctx.session_mut()
        .heap
        .try_protect(x)
        .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))?;
    Ok(())
}

pub(super) fn value(ctx: &mut Ctx, x: Sexp) -> EvalResult<SexpId> {
    id(ctx, x, "ALTREP method result")
}

pub(super) fn optional_value(ctx: &mut Ctx, x: Option<Sexp>) -> EvalResult<Option<SexpId>> {
    x.filter(|&x| x != Sexp::NULL)
        .map(|x| value(ctx, x))
        .transpose()
}

pub(crate) fn c_dataptr(ctx: &mut Ctx, x: SexpId, writable: Rboolean) -> EvalResult<Option<usize>> {
    let Some(info) = ctx.session().foreign_alt(x)? else {
        return Ok(None);
    };
    if writable != 0 && matches!(info.ty, SexpType::Str | SexpType::Vec) {
        return Err(ctx.stop(
            SEXP_NIL,
            "writable data requires a numeric, logical or raw vector",
        ));
    }
    let access = if writable != 0 {
        Access::Write
    } else {
        Access::Read
    };
    materialize(ctx, x, access)?;
    let s = ctx.session_mut();
    let (addr, _) = if writable != 0 {
        let data = s.data_ptr_rw(x)?;
        s.mark_foreign_alt_dirty(x)?;
        data
    } else {
        s.data_ptr_ro(x)?
    };
    Ok(Some(addr))
}

pub(crate) fn c_or_null(ctx: &mut Ctx, x: SexpId) -> EvalResult<Option<*const c_void>> {
    let Some(methods) = lazy_methods(ctx, x)? else {
        return Ok(None);
    };
    ctx.scoped(|ctx| {
        protect(ctx, x)?;
        Ok(Some(
            methods
                .dataptr_or_null(ctx, Sexp::from_id(x))?
                .unwrap_or(ptr::null()),
        ))
    })
}

pub(super) fn checked_region(ctx: &mut Ctx, got: R_xlen_t, len: R_xlen_t) -> EvalResult<R_xlen_t> {
    if !(0..=len).contains(&got) {
        return Err(ctx.stop(
            SEXP_NIL,
            format!("invalid ALTREP Get_region result {got} for length {len}"),
        ));
    }
    Ok(got)
}

pub(crate) unsafe fn c_region<T>(
    ctx: &mut Ctx,
    x: SexpId,
    ty: SexpType,
    start: usize,
    len: usize,
    out: *mut T,
) -> EvalResult<Option<R_xlen_t>> {
    let Some(methods) = lazy_methods(ctx, x)? else {
        return Ok(None);
    };
    ctx.scoped(|ctx| {
        protect(ctx, x)?;
        let x = Sexp::from_id(x);
        let start = start as R_xlen_t;
        let len = len as R_xlen_t;
        macro_rules! region_or_elt {
            ($region:ident, $elt:ident) => {{
                let out = out.cast();
                if let Some(got) = methods.$region(ctx, x, start, len, out)? {
                    return checked_region(ctx, got, len).map(Some);
                }
                for i in 0..len {
                    let Some(item) = methods.$elt(ctx, x, start + i)? else {
                        return Ok(None);
                    };
                    // SAFETY: The caller supplies len writable elements of the declared type.
                    unsafe { out.add(i as usize).write(item) };
                }
                Ok(Some(len))
            }};
        }
        match ty {
            SexpType::Int => region_or_elt!(int_region, int_elt),
            SexpType::Real => region_or_elt!(real_region, real_elt),
            SexpType::Lgl => region_or_elt!(lgl_region, lgl_elt),
            SexpType::Raw => region_or_elt!(raw_region, raw_elt),
            SexpType::Cplx => region_or_elt!(cplx_region, cplx_elt),
            _ => Ok(None),
        }
    })
}

macro_rules! atomic_elt {
    ($($name:ident $elt:ident $region:ident $element:ty, $fill:expr;)*) => {$(
        pub(crate) fn $name(ctx: &mut Ctx, x: SexpId, i: usize) -> EvalResult<Option<$element>> {
            let Some(methods) = lazy_methods(ctx, x)? else { return Ok(None) };
            ctx.scoped(|ctx| {
                protect(ctx, x)?;
                let x = Sexp::from_id(x);
                let i = i as R_xlen_t;
                if let Some(item) = methods.$elt(ctx, x, i)? {
                    return Ok(Some(item));
                }
                let mut item = $fill;
                if let Some(got) = methods.$region(ctx, x, i, 1, &mut item)?
                    && checked_region(ctx, got, 1)? == 1
                {
                    return Ok(Some(item));
                }
                Ok(None)
            })
        }
    )*};
}

atomic_elt! {
    int_elt int_elt int_region c_int, 0;
    real_elt real_elt real_region f64, 0.0;
    lgl_elt lgl_elt lgl_region c_int, 0;
    raw_elt raw_elt raw_region u8, 0;
    cplx_elt cplx_elt cplx_region crate::abi::Rcomplex, crate::abi::Rcomplex {r: 0.0, i: 0.0};
}

pub(crate) fn object_elt(
    ctx: &mut Ctx,
    x: SexpId,
    ty: SexpType,
    i: usize,
) -> EvalResult<Option<SexpId>> {
    let Some(methods) = lazy_methods(ctx, x)? else {
        return Ok(None);
    };
    ctx.scoped(|ctx| {
        protect(ctx, x)?;
        let vector = Sexp::from_id(x);
        let i = i as R_xlen_t;
        let (result, missing) = match ty {
            SexpType::Str => (
                methods.str_elt(ctx, vector, i)?,
                "No Elt method found for ALTSTRING class",
            ),
            SexpType::Vec => (
                methods.list_elt(ctx, vector, i)?,
                "ALTLIST classes must provide an Elt method",
            ),
            _ => return Ok(None),
        };
        let elt = result.ok_or_else(|| class_error(ctx, x, missing))?;
        let elt = value(ctx, elt)?;
        if ty == SexpType::Str {
            ctx.session().sexp_as_char(elt)?;
        }
        Ok(Some(elt))
    })
}

pub(crate) fn c_set_elt(
    ctx: &mut Ctx,
    x: SexpId,
    ty: SexpType,
    i: usize,
    item: SexpId,
) -> EvalResult<bool> {
    let Some(methods) = lazy_methods(ctx, x)? else {
        return Ok(false);
    };
    ctx.scoped(|ctx| {
        protect(ctx, x)?;
        protect(ctx, item)?;
        let i = i as R_xlen_t;
        let item = Sexp::from_id(item);
        let vector = Sexp::from_id(x);
        let (result, missing) = match ty {
            SexpType::Str => (
                methods.str_set_elt(ctx, vector, i, item)?,
                "No Set_elt found for ALTSTRING class",
            ),
            SexpType::Vec => (
                methods.list_set_elt(ctx, vector, i, item)?,
                "ALTLIST classes must provide a Set_elt method",
            ),
            _ => return Ok(false),
        };
        result.ok_or_else(|| class_error(ctx, x, missing))?;
        Ok(true)
    })
}

pub(crate) fn c_flags(ctx: &mut Ctx, x: SexpId, sorted: bool) -> EvalResult<Option<c_int>> {
    let Some(methods) = lazy_methods(ctx, x)? else {
        return Ok(None);
    };
    let ty = ctx.session().type_of(x)?;
    ctx.scoped(|ctx| {
        protect(ctx, x)?;
        let x = Sexp::from_id(x);
        let flag = match (ty, sorted) {
            (SexpType::Int, true) => methods.int_sorted(ctx, x)?,
            (SexpType::Real, true) => methods.real_sorted(ctx, x)?,
            (SexpType::Lgl, true) => methods.lgl_sorted(ctx, x)?,
            (SexpType::Str, true) => methods.str_sorted(ctx, x)?,
            (SexpType::Int, false) => methods.int_no_na(ctx, x)?,
            (SexpType::Real, false) => methods.real_no_na(ctx, x)?,
            (SexpType::Lgl, false) => methods.lgl_no_na(ctx, x)?,
            (SexpType::Str, false) => methods.str_no_na(ctx, x)?,
            _ => None,
        };
        Ok(Some(flag.unwrap_or(if sorted { c_int::MIN } else { 0 })))
    })
}

pub(crate) fn c_duplicate(ctx: &mut Ctx, x: SexpId, deep: bool) -> EvalResult<Option<SexpId>> {
    let Some(methods) = lazy_methods(ctx, x)? else {
        return Ok(None);
    };
    let obj = Sexp::from_id(x);
    let flag = Rboolean::from(deep);
    let result = methods.duplicate_ex(ctx, obj, flag)?;
    if result.is_none() {
        let result = methods.duplicate(ctx, obj, flag)?;
        if let Some(duplicate) = optional_value(ctx, result)? {
            return ctx.scoped(|ctx| {
                protect(ctx, duplicate)?;
                if duplicate == SEXP_NIL || duplicate == x {
                    return Ok(Some(duplicate));
                }
                if ctx.session().attr(x, SYM_NAMES) != SEXP_NIL {
                    ctx.altrep_len(duplicate)?;
                }
                if !deep {
                    crate::api::attrib::copy_attrs(ctx, x, duplicate, false)?;
                    return Ok(Some(duplicate));
                }
                let mut attrs = ctx.session().attrs(x)?;
                for (_, value) in &mut attrs {
                    *value = ctx.session_mut().deep_duplicate(*value)?;
                    protect(ctx, *value)?;
                }
                let s = ctx.session_mut();
                s.replace_attributes(duplicate, attrs)?;
                s.set_object_flag(duplicate, s.is_object(x)?)?;
                s.set_s4_flag(duplicate, s.is_s4(x)?)?;
                Ok(Some(duplicate))
            });
        }
    } else if let Some(duplicate) = optional_value(ctx, result)? {
        return Ok(Some(duplicate));
    }
    c_dataptr(ctx, x, 0)?;
    Ok(None)
}

pub(crate) fn string_elt(ctx: &mut Ctx, x: SexpId, i: R_xlen_t, name: &str) -> EvalResult<SexpId> {
    require_type(ctx, x, SexpType::Str, name)?;
    let i = index(ctx, i, ctx.session().xlength(x)?)?;
    if let Some(elt) = object_elt(ctx, x, SexpType::Str, i)? {
        return Ok(elt);
    }
    Ok(ctx.session_mut().str_get(x, i)?.as_sexp())
}

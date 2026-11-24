use std::ffi::c_void;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use super::callbacks::Methods;
use super::registry::{class, class_error, foreign};
use super::vector::{checked_region, object_elt, protect, value};
use crate::abi::{R_xlen_t, Rboolean, Sexp};
use crate::api::resizable::element_width;
use crate::api::vector::{alloc_vector, check_capacity};

fn missing_pointer(ctx: &mut Ctx, x: SexpId) -> roxide_eval::Unwind {
    class_error(ctx, x, "cannot access data pointer for this ALTVEC object")
}

#[derive(Clone, Copy)]
pub(in crate::api) enum Access {
    Evaluate,
    Read,
    Write,
}

pub(in crate::api) fn materialize(ctx: &mut Ctx, x: SexpId, access: Access) -> EvalResult<()> {
    let info = foreign(ctx, x)?;
    if info.owned {
        return Ok(());
    }
    let methods = class(ctx, info.class)?.methods;
    if !matches!(access, Access::Evaluate) && !methods.has_dataptr() {
        return Err(missing_pointer(ctx, x));
    }
    ctx.scoped(|ctx| {
        protect(ctx, x)?;
        let len = super::dispatch::ensure_length(ctx, x)?;
        if foreign(ctx, x)?.owned {
            return Ok(());
        }
        let width = element_width(info.ty);
        check_capacity(ctx, len, width)?;
        let dense = alloc_vector(ctx, info.ty, len)?;
        protect(ctx, dense)?;

        let use_slots = matches!(info.ty, SexpType::Str | SexpType::Vec);
        let writable = Rboolean::from(matches!(access, Access::Write));
        let data = methods.dataptr(ctx, Sexp::from_id(x), writable)?;
        if foreign(ctx, x)?.owned {
            return Ok(());
        }
        if let Some(data) = data {
            if len != 0 && data.is_null() {
                return Err(missing_pointer(ctx, x));
            }
            if len != 0 {
                if use_slots {
                    copy_slots(ctx, dense, info.ty, data, len)?;
                } else {
                    let (out, _) = ctx.session_mut().data_ptr_rw(dense)?;
                    // SAFETY: Dataptr supplies len elements of the declared type.
                    // The private dense buffer has room for those elements.
                    unsafe { std::ptr::copy(data.cast::<u8>(), out as *mut u8, len * width) };
                }
            }
        } else if use_slots {
            for i in 0..len {
                let item = object_elt(ctx, x, info.ty, i)?
                    .ok_or_else(|| ctx.stop(SEXP_NIL, "ALTREP class has no Elt method"))?;
                if foreign(ctx, x)?.owned {
                    break;
                }
                set_slot(ctx, dense, info.ty, i, item)?;
            }
        } else if len != 0 {
            let (out, _) = ctx.session_mut().data_ptr_rw(dense)?;
            fill_atomic(
                ctx,
                methods,
                x,
                info.ty,
                len as R_xlen_t,
                out as *mut c_void,
            )?;
        }
        if !ctx.session_mut().adopt_foreign_alt_payload(x, dense)? {
            ctx.session().data_ptr_ro(x)?;
        }
        Ok(())
    })
}

fn copy_slots(
    ctx: &mut Ctx,
    dense: SexpId,
    ty: SexpType,
    data: *mut c_void,
    len: usize,
) -> EvalResult<()> {
    for i in 0..len {
        // SAFETY: Dataptr supplies len readable SEXP handles for this vector type.
        let item = unsafe { data.cast::<Sexp>().add(i).read_unaligned() };
        let item = value(ctx, item)?;
        set_slot(ctx, dense, ty, i, item)?;
    }
    Ok(())
}

fn set_slot(ctx: &mut Ctx, dense: SexpId, ty: SexpType, i: usize, item: SexpId) -> EvalResult<()> {
    let s = ctx.session_mut();
    if ty == SexpType::Str {
        s.str_set(dense, i, s.sexp_as_char(item)?)?;
    } else {
        s.list_set(dense, i, item)?;
    }
    Ok(())
}

fn fill_atomic(
    ctx: &mut Ctx,
    methods: Methods,
    x: SexpId,
    ty: SexpType,
    len: R_xlen_t,
    out: *mut c_void,
) -> EvalResult<()> {
    let object = Sexp::from_id(x);
    macro_rules! fill {
        ($region:ident, $fill:ident, $elt:ident, $ty:ty) => {{
            let out = out.cast::<$ty>();
            let mut start = 0;
            while start < len {
                // SAFETY: The private dense buffer has len elements and start is in bounds.
                let target = unsafe { out.add(start as usize) };
                let got = methods.$region(ctx, object, start, len - start, target)?;
                if got.is_some() && foreign(ctx, x)?.owned {
                    break;
                }
                match got {
                    Some(n) if checked_region(ctx, n, len - start)? != 0 => start += n,
                    _ if start == 0 && methods.$fill(ctx, object, len, out)? => break,
                    _ => {
                        let item = methods.$elt(ctx, object, start)?
                            .ok_or_else(|| ctx.stop(SEXP_NIL, "ALTREP class cannot supply its elements"))?;
                        if foreign(ctx, x)?.owned {
                            break;
                        }
                        // SAFETY: target points to one writable element in the private dense buffer.
                        unsafe { target.write(item) };
                        start += 1;
                    }
                }
            }
        }};
    }
    match ty {
        SexpType::Int => fill!(int_region, int_fill, int_elt, i32),
        SexpType::Real => fill!(real_region, real_fill, real_elt, f64),
        SexpType::Lgl => fill!(lgl_region, lgl_fill, lgl_elt, i32),
        SexpType::Raw => fill!(raw_region, raw_fill, raw_elt, u8),
        SexpType::Cplx => fill!(cplx_region, cplx_fill, cplx_elt, crate::abi::Rcomplex),
        _ => return Err(ctx.stop(SEXP_NIL, "expected an atomic ALTREP vector")),
    }
    Ok(())
}

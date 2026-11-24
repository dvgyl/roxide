//! R 4.6.2 resizable vector API (`R_allocResizableVector`, `R_resizeVector`, etc.).

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, NA_INTEGER, NA_REAL, RComplex, RLogical, SEXP_NIL, SYM_DIM, SYM_DIMNAMES, SYM_NAMES,
    SexpType, ValueApi,
};

use super::pairlist::cells;
use super::vector::{
    alloc_vector, allocating, api, check_capacity, checked_len, decode_type, id, is_vector,
};
use crate::abi::{R_xlen_t, SEXPTYPE, Sexp};

pub(crate) fn element_width(ty: SexpType) -> usize {
    match ty {
        SexpType::Lgl | SexpType::Int => size_of::<i32>(),
        SexpType::Cplx => size_of::<RComplex>(),
        SexpType::Raw => size_of::<u8>(),
        _ => size_of::<SexpId>(),
    }
}

fn copy_elements(
    ctx: &mut Ctx,
    from: SexpId,
    to: SexpId,
    ty: SexpType,
    n: usize,
    fill_na: bool,
) -> EvalResult<()> {
    let s = ctx.session_mut();
    let len = s.xlength(to)?;
    if ty == SexpType::Str {
        for i in 0..n {
            let ch = s.str_get(from, i)?;
            s.str_set(to, i, ch)?;
        }
        if fill_na {
            for i in n..len {
                s.str_set(to, i, CharId::NA)?;
            }
        }
    } else if matches!(ty, SexpType::Vec | SexpType::Expr) {
        for i in 0..n {
            let value = s.list_get(from, i)?;
            s.list_set(to, i, value)?;
        }
    } else {
        macro_rules! copy {
            ($ty:ty, $get:ident, $na:expr) => {{
                let (ptr, _) = s.data_ptr_rw(to)?;
                let view = s.$get(from)?;
                // SAFETY: The fresh destination has the declared type and length.
                // It has no aliases, and the source has a distinct backing buffer.
                let dst = unsafe { std::slice::from_raw_parts_mut(ptr as *mut $ty, len) };
                for (i, slot) in dst[..n].iter_mut().enumerate() {
                    *slot = view.get(i);
                }
                if fill_na {
                    dst[n..].fill($na);
                }
            }};
        }

        match ty {
            SexpType::Int => copy!(i32, int, NA_INTEGER),
            SexpType::Lgl => copy!(RLogical, lgl, RLogical::NA),
            SexpType::Real => copy!(f64, real, NA_REAL),
            SexpType::Cplx => copy!(RComplex, cplx, RComplex::NA),
            SexpType::Raw => copy!(u8, raw, 0),
            _ => return Err(ctx.stop(SEXP_NIL, "cannot copy elements of this type")),
        }
    }
    Ok(())
}

pub(crate) fn set_length(ctx: &mut Ctx, x: SexpId, n: usize) -> EvalResult<SexpId> {
    let ty = ctx.session().type_of(x)?;
    if ty == SexpType::Nil {
        return Ok(x);
    }
    if ty == SexpType::List {
        let elements = cells(ctx, x)?;
        if n == elements.len() {
            return Ok(x);
        }
        check_capacity(ctx, n, size_of::<roxide_value::RObject>())?;
        let s = ctx.session_mut();
        let mut out = SEXP_NIL;
        for i in (0..n).rev() {
            let (car, tag) = elements
                .get(i)
                .map_or((SEXP_NIL, SEXP_NIL), |&(_, car, tag)| (car, tag));
            out = s.cons(car, out, tag)?;
        }
        return Ok(out);
    }
    if !is_vector(ty) {
        return Err(ctx.stop(SEXP_NIL, "cannot set length of non-vector"));
    }
    let old = ctx.session().xlength(x)?;
    if old == n {
        return Ok(x);
    }
    let out = alloc_vector(ctx, ty, n)?;
    copy_elements(ctx, x, out, ty, n.min(old), true)?;
    let names = ctx.session().attr(x, SYM_NAMES);
    if names != SEXP_NIL {
        let labels = alloc_vector(ctx, SexpType::Str, n)?;
        let count = n.min(ctx.session().xlength(names)?);
        copy_elements(ctx, names, labels, SexpType::Str, count, true)?;
        ctx.session_mut().set_attrib(out, SYM_NAMES, labels)?;
    }
    Ok(out)
}

api! {
    /// Allocate a pinned vector with the specified length and capacity.
    fn R_allocResizableVector(ty: SEXPTYPE, maxlen: R_xlen_t) -> Sexp = Sexp::NULL; |ctx| {
        let n = checked_len(ctx, maxlen)?;
        let ty = decode_type(ctx, ty)?;
        check_capacity(ctx, n, element_width(ty))?;
        allocating(ctx, &[], |ctx| {
            Ok(Sexp::from_id(ctx.session_mut().alloc_resizable(ty, n, n)?))
        })
    }
    /// Resize a pinned vector within its capacity and remove shape attributes.
    fn R_resizeVector(x: Sexp, n: R_xlen_t) -> () = (); |ctx| {
        let x = id(ctx, x, "R_resizeVector")?;
        let n = checked_len(ctx, n)?;
        let s = ctx.session_mut();
        s.resize_vector(x, n)?;
        for key in [SYM_DIM, SYM_DIMNAMES, SYM_NAMES] {
            if s.attr(x, key) != SEXP_NIL {
                s.set_attrib(x, key, SEXP_NIL)?;
            }
        }
        Ok(())
    }
    /// Test whether a vector can be resized in place.
    fn R_isResizable(x: Sexp) -> bool = false; |ctx| {
        let x = id(ctx, x, "R_isResizable")?;
        Ok(ctx.session().is_resizable(x)?)
    }
    /// Return the maximal length of a resizable vector, or the current length otherwise.
    fn R_maxLength(x: Sexp) -> R_xlen_t = 0; |ctx| {
        let x = id(ctx, x, "R_maxLength")?;
        let s = ctx.session();
        let n = if s.is_resizable(x)? { s.vec_truelength(x)? } else { super::vector::length(ctx, x)? };
        Ok(n as R_xlen_t)
    }
    /// Copy a vector into pinned storage with capacity equal to its current length.
    fn R_duplicateAsResizable(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "R_duplicateAsResizable")?;
        let ty = ctx.session().type_of(x)?;
        if !is_vector(ty) {
            return Err(ctx.stop(SEXP_NIL, "cannot make a non-vector resizable"));
        }
        let n = ctx.session().xlength(x)?;
        check_capacity(ctx, n, element_width(ty))?;
        allocating(ctx, &[x], |ctx| {
            let s = ctx.session_mut();
            let copy = s.deep_duplicate(x)?;
            let out = s.alloc_resizable(ty, n, n)?;
            copy_elements(ctx, copy, out, ty, n, false)?;
            let s = ctx.session_mut();
            s.replace_attributes(out, s.attrs(copy)?)?;
            s.set_object_flag(out, s.is_object(copy)?)?;
            s.set_s4_flag(out, s.is_s4(copy)?)?;
            Ok(Sexp::from_id(out))
        })
    }
}

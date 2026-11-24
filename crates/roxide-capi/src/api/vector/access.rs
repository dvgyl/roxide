use std::ffi::{c_int, c_void};
use std::ptr::{null, null_mut};

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{Elem, RComplex, RLogical, SEXP_NIL, SYM_DIM, SexpType, ValueApi, VecView};

use super::{allocating, api, check_capacity, checked_len, id, is_atomic, is_vector};
use crate::abi::*;
use crate::api::attrib::inherits;
use crate::api::pairlist::cells;
use crate::api::resizable::element_width;

pub(crate) fn length(ctx: &mut Ctx, x: SexpId) -> EvalResult<usize> {
    let ty = ctx.session().type_of(x)?;
    if matches!(ty, SexpType::List | SexpType::Lang | SexpType::Dot) {
        return Ok(cells(ctx, x)?.len());
    }
    if ty == SexpType::Char {
        let (_, len) = ctx.session().data_ptr_ro(x)?;
        return Ok(len);
    }
    Ok(ctx.session().xlength(x)?)
}

fn short_len(ctx: &mut Ctx, x: SexpId) -> EvalResult<c_int> {
    c_int::try_from(length(ctx, x)?)
        .map_err(|_| ctx.stop(SEXP_NIL, "long vectors not supported yet"))
}

fn by_dimension(ctx: &mut Ctx, x: SexpId, i: usize) -> EvalResult<c_int> {
    let dims = ctx.session().attr(x, SYM_DIM);
    if dims != SEXP_NIL {
        let values = ctx.session().int(dims)?;
        return Ok(if i < values.len() { values.get(i) } else { 1 });
    }
    if i == 0 { short_len(ctx, x) } else { Ok(1) }
}

fn matrix_extent(ctx: &mut Ctx, x: SexpId, axis: usize) -> EvalResult<c_int> {
    if inherits(ctx, x, b"data.frame")? {
        let axis_value = if axis == 0 {
            ctx.session_mut().attr_row_names(x)?
        } else {
            x
        };
        return short_len(ctx, axis_value);
    }
    let ty = ctx.session().type_of(x)?;
    if x != SEXP_NIL && !is_vector(ty) && ty != SexpType::List {
        return Err(ctx.stop(SEXP_NIL, "object is not a matrix"));
    }
    by_dimension(ctx, x, axis)
}

api! {
    /// Return the row count of a vector, array, or data frame.
    fn R_nrow(x: Sexp) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "R_nrow")?;
        allocating(ctx, &[x], |ctx| matrix_extent(ctx, x, 0))
    }
    /// Return the column count of a vector, array, or data frame.
    fn R_ncol(x: Sexp) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "R_ncol")?;
        matrix_extent(ctx, x, 1)
    }
    /// Return the row count.
    fn Rf_nrows(x: Sexp) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "Rf_nrows")?;
        by_dimension(ctx, x, 0)
    }
    /// Return the column count.
    fn Rf_ncols(x: Sexp) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "Rf_ncols")?;
        by_dimension(ctx, x, 1)
    }
}

macro_rules! length_exports {
    ($($name:ident: $ty:ty => $get:ident),+ $(,)?) => {
        api! {$(
            /// Return the number of elements in an R object.
            fn $name(x: Sexp) -> $ty = 0; |ctx| {
                let x = id(ctx, x, stringify!($name))?;
                Ok($get(ctx, x)? as $ty)
            }
        )+}
    };
}

length_exports!(LENGTH: R_len_t => short_len, Rf_length: R_len_t => short_len,
                XLENGTH: R_xlen_t => length, Rf_xlength: R_xlen_t => length);

pub(crate) fn require_type(ctx: &mut Ctx, x: SexpId, ty: SexpType, name: &str) -> EvalResult<()> {
    let actual = ctx.session().type_of(x)?;
    if actual != ty {
        let actual = actual.type_name(ctx.session().is_s4(x)?);
        return Err(ctx.stop(
            SEXP_NIL,
            format!(
                "{name}() can only be applied to a '{}', not a '{actual}'",
                ty.type_name(false)
            ),
        ));
    }
    Ok(())
}

pub(crate) fn index(ctx: &mut Ctx, i: R_xlen_t, len: usize) -> EvalResult<usize> {
    if i < 0 || i as usize >= len {
        return Err(ctx.stop(
            SEXP_NIL,
            format!("subscript out of bounds ({i} for length {len})"),
        ));
    }
    Ok(i as usize)
}

pub(crate) fn read_ptr(ctx: &mut Ctx, x: SexpId, ty: SexpType) -> EvalResult<*const c_void> {
    let s = ctx.session();
    if s.is_altrep(x)? && s.data_ptr_ro(x).is_err() {
        let n = s.xlength(x)?;
        check_capacity(ctx, n, element_width(ty))?;
    }
    let s = ctx.session_mut();
    let ptr = match ty {
        SexpType::Int => s.int_dataptr_ro(x)?.as_ptr().cast(),
        SexpType::Real => s.real_dataptr_ro(x)?.as_ptr().cast(),
        SexpType::Lgl => s.lgl_dataptr_ro(x)?.as_ptr().cast(),
        SexpType::Cplx => s.cplx_dataptr_ro(x)?.as_ptr().cast(),
        SexpType::Raw => s.raw_dataptr_ro(x)?.as_ptr().cast(),
        SexpType::Str => {
            s.str_dataptr_ro(x)?;
            s.data_ptr_ro(x)?.0 as *const c_void
        }
        _ => s.data_ptr_ro(x)?.0 as *const c_void,
    };
    Ok(ptr)
}

pub(super) fn write_ptr(ctx: &mut Ctx, x: SexpId) -> EvalResult<usize> {
    let ty = ctx.session().type_of(x)?;
    if !is_atomic(ty) || ty == SexpType::Str {
        return Err(ctx.stop(
            SEXP_NIL,
            "writable data requires a numeric, logical or raw vector",
        ));
    }
    if ctx.session().is_altrep(x)? {
        read_ptr(ctx, x, ty)?;
    }
    Ok(ctx.session_mut().data_ptr_rw(x)?.0)
}

unsafe fn write_element<T>(ptr: usize, i: usize, value: T) {
    // SAFETY: The caller checked the type and index against this writable vector.
    unsafe { (ptr as *mut T).add(i).write(value) };
}

unsafe fn copy_region<T: Elem>(
    view: VecView<'_, T>,
    start: usize,
    count: usize,
    out: *mut T,
    fill: T,
) -> usize {
    match view {
        VecView::Slice(values) => {
            // SAFETY: The caller checked the input range and supplies a writable output buffer.
            unsafe { std::ptr::copy(values.as_ptr().add(start), out, count) };
            count
        }
        VecView::Alt { class, heap, .. } => {
            // SAFETY: The caller supplies count writable elements, which can be uninitialized.
            let buffer = unsafe {
                std::slice::from_raw_parts_mut(out.cast::<std::mem::MaybeUninit<T>>(), count)
            };
            buffer.fill(std::mem::MaybeUninit::new(fill));
            // SAFETY: The previous fill initialized every element of the exclusive buffer.
            let out = unsafe { std::slice::from_raw_parts_mut(out, count) };
            class.get_region(heap, start, out)
        }
    }
}

macro_rules! atomic {
    ($ty:ident, $engine:ty, $c:ty, $view:ident, $get:expr, $put:expr, $fill:expr;
     $rw:ident, $ro:ident, $or_null:ident, $elt:ident, $set:ident, $region:ident) => {
        api! {
            /// Return a writable pointer to the vector elements.
            fn $rw(x: Sexp) -> *mut $c = null_mut(); |ctx| {
                let name = stringify!($rw);
                let x = id(ctx, x, name)?;
                require_type(ctx, x, SexpType::$ty, name)?;
                Ok(write_ptr(ctx, x)? as *mut $c)
            }
            /// Return a read-only pointer to the vector elements.
            fn $ro(x: Sexp) -> *const $c = null(); |ctx| {
                let name = stringify!($ro);
                let x = id(ctx, x, name)?;
                require_type(ctx, x, SexpType::$ty, name)?;
                Ok(read_ptr(ctx, x, SexpType::$ty)?.cast())
            }
            /// Return a pointer if the vector already exposes a contiguous buffer.
            fn $or_null(x: Sexp) -> *const $c = null(); |ctx| {
                let name = stringify!($or_null);
                let x = id(ctx, x, name)?;
                require_type(ctx, x, SexpType::$ty, name)?;
                Ok(ctx.session().$view(x)?.as_slice().map_or(null(), |s| s.as_ptr().cast()))
            }
            /// Return one element without materializing an ALTREP vector.
            fn $elt(x: Sexp, i: R_xlen_t) -> $c = ($get)($fill); |ctx| {
                let name = stringify!($elt);
                let x = id(ctx, x, name)?;
                require_type(ctx, x, SexpType::$ty, name)?;
                let i = index(ctx, i, ctx.session().xlength(x)?)?;
                Ok(($get)(ctx.session().$view(x)?.get(i)))
            }
            /// Set an element of an atomic vector.
            fn $set(x: Sexp, i: R_xlen_t, value: $c) -> () = (); |ctx| {
                let name = stringify!($set);
                let x = id(ctx, x, name)?;
                require_type(ctx, x, SexpType::$ty, name)?;
                let i = index(ctx, i, ctx.session().xlength(x)?)?;
                let ptr = write_ptr(ctx, x)?;
                write_element::<$engine>(ptr, i, ($put)(value));
                Ok(())
            }
            /// Copy a range of elements to the caller's buffer.
            fn $region(x: Sexp, start: R_xlen_t, count: R_xlen_t, out: *mut $c) -> R_xlen_t = 0; |ctx| {
                let name = stringify!($region);
                let x = id(ctx, x, name)?;
                require_type(ctx, x, SexpType::$ty, name)?;
                let start = checked_len(ctx, start)?;
                let count = checked_len(ctx, count)?;
                let view = ctx.session().$view(x)?;
                let count = count.min(view.len().saturating_sub(start));
                if count == 0 { return Ok(0); }
                if out.is_null() {
                    return Err(ctx.stop(SEXP_NIL, format!("invalid NULL buffer passed to {name}")));
                }
                Ok(copy_region(view, start, count, out.cast::<$engine>(), $fill) as R_xlen_t)
            }
        }
    };
}

atomic!(Int, i32, c_int, int, std::convert::identity, std::convert::identity, 0;
    INTEGER, INTEGER_RO, INTEGER_OR_NULL, INTEGER_ELT, SET_INTEGER_ELT, INTEGER_GET_REGION);
atomic!(Real, f64, f64, real, std::convert::identity, std::convert::identity, 0.0;
    REAL, REAL_RO, REAL_OR_NULL, REAL_ELT, SET_REAL_ELT, REAL_GET_REGION);
atomic!(Lgl, RLogical, c_int, lgl, RLogical::raw, RLogical::from_raw, RLogical::NA;
    LOGICAL, LOGICAL_RO, LOGICAL_OR_NULL, LOGICAL_ELT, SET_LOGICAL_ELT, LOGICAL_GET_REGION);
atomic!(Cplx, RComplex, Rcomplex, cplx, complex_to_c, complex_from_c, RComplex::NA;
    COMPLEX, COMPLEX_RO, COMPLEX_OR_NULL, COMPLEX_ELT, SET_COMPLEX_ELT, COMPLEX_GET_REGION);
atomic!(Raw, u8, Rbyte, raw, std::convert::identity, std::convert::identity, 0;
    RAW, RAW_RO, RAW_OR_NULL, RAW_ELT, SET_RAW_ELT, RAW_GET_REGION);

pub(crate) fn complex_to_c(v: RComplex) -> Rcomplex {
    Rcomplex { r: v.re, i: v.im }
}
pub(crate) fn complex_from_c(v: Rcomplex) -> RComplex {
    RComplex { re: v.r, im: v.i }
}

fn slice_ptr<T>(view: VecView<'_, T>) -> *const c_void
where
    T: Elem,
{
    view.as_slice().map_or(null(), |s| s.as_ptr().cast())
}

api! {
    /// Return read-only data, materializing an ALTREP vector if necessary.
    fn DATAPTR_RO(x: Sexp) -> *const c_void = null(); |ctx| {
        let x = id(ctx, x, "DATAPTR_RO")?;
        let ty = ctx.session().type_of(x)?;
        read_ptr(ctx, x, ty)
    }
    /// Return writable data for an atomic numeric, logical or raw vector.
    fn DATAPTR_RW(x: Sexp) -> *mut c_void = null_mut(); |ctx| {
        let x = id(ctx, x, "DATAPTR_RW")?;
        Ok(write_ptr(ctx, x)? as *mut c_void)
    }
    /// Return available data without materializing a vector.
    fn DATAPTR_OR_NULL(x: Sexp) -> *const c_void = null(); |ctx| {
        let x = id(ctx, x, "DATAPTR_OR_NULL")?;
        let s = ctx.session();
        Ok(match s.type_of(x)? {
            SexpType::Int => slice_ptr(s.int(x)?),
            SexpType::Real => slice_ptr(s.real(x)?),
            SexpType::Lgl => slice_ptr(s.lgl(x)?),
            SexpType::Cplx => slice_ptr(s.cplx(x)?),
            SexpType::Raw => slice_ptr(s.raw(x)?),
            _ => s.data_ptr_ro(x).map_or(null(), |(ptr, _)| ptr as *const c_void),
        })
    }
    /// Return read-only data for a list or expression vector.
    fn VECTOR_PTR_RO(x: Sexp) -> *const Sexp = null(); |ctx| {
        let x = id(ctx, x, "VECTOR_PTR_RO")?;
        Ok(ctx.session().list_slice(x)?.as_ptr().cast())
    }
    /// Read one element of a list or expression vector.
    fn VECTOR_ELT(x: Sexp, i: R_xlen_t) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "VECTOR_ELT")?;
        let i = index(ctx, i, ctx.session().list_slice(x)?.len())?;
        Ok(Sexp::from_id(ctx.session().list_get(x, i)?))
    }
    /// Set and return an element of a list or expression vector.
    fn SET_VECTOR_ELT(x: Sexp, i: R_xlen_t, value: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "SET_VECTOR_ELT")?;
        let element = id(ctx, value, "SET_VECTOR_ELT")?;
        let i = index(ctx, i, ctx.session().list_slice(x)?.len())?;
        ctx.session_mut().list_set(x, i, element)?;
        Ok(value)
    }
    /// Test whether an object has the specified type and length one.
    fn IS_SCALAR(x: Sexp, ty: c_int) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "IS_SCALAR")?;
        Ok(c_int::from(ctx.session().type_of(x)? as c_int == ty && length(ctx, x)? == 1))
    }
}

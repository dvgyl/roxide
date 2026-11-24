//! Vector allocation, element access, data pointers, and type predicates.

#![allow(non_snake_case)]

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void};
use std::ptr::{null, null_mut};

use roxide_eval::{Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{
    CharId, Elem, Encoding, RComplex, RLogical, SEXP_NIL, SYM_DIM, SexpType, Sortedness, ValueApi,
    VecView,
};

use super::attrib::inherits;
use super::pairlist::{alloc_cells, cells};
use crate::abi::*;

macro_rules! api {
    ($(#[doc = $doc:expr] fn $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty =
        $default:expr; |$ctx:ident| $body:block)*) => {$(
        #[doc = $doc]
        ///
        /// # Safety
        /// Call inside an active native trampoline. Supply valid storage for
        /// all C buffers and follow the R protection rules for returned values.
        #[allow(non_snake_case)]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name($($arg: $ty),*) -> $ret {
            // SAFETY: The caller supplies the trampoline and C buffers.
            unsafe {
                $crate::jump::boundary($default, |$ctx| {
                    $crate::api::vector::signal_value_error($ctx, |$ctx| $body)
                })
            }
        }
    )*};
}
pub(crate) use api;

pub(crate) fn signal_value_error<T>(
    ctx: &mut Ctx,
    f: impl FnOnce(&mut Ctx) -> EvalResult<T>,
) -> EvalResult<T> {
    match f(ctx) {
        Err(roxide_eval::Unwind::Value(err)) => {
            Err(ctx.signal_error_at(SEXP_NIL, &err.to_string()))
        }
        result => result,
    }
}

pub(crate) fn id(ctx: &mut Ctx, x: Sexp, name: &str) -> EvalResult<SexpId> {
    let Some(value) = x.to_id() else {
        let reason = if x == Sexp::NULL { "NULL" } else { "malformed" };
        return Err(ctx.stop(SEXP_NIL, format!("invalid {reason} SEXP passed to {name}")));
    };
    if ctx.session().heap.get(value).is_err() {
        return Err(ctx.stop(SEXP_NIL, format!("stale SEXP passed to {name}")));
    }
    Ok(value)
}

pub(crate) fn allocating<T>(
    ctx: &mut Ctx,
    inputs: &[SexpId],
    f: impl FnOnce(&mut Ctx) -> EvalResult<T>,
) -> EvalResult<T> {
    ctx.scoped(|ctx| {
        for &x in inputs {
            ctx.session_mut()
                .heap
                .try_protect(x)
                .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))?;
        }
        ctx.safepoint();
        f(ctx)
    })
}

const TYPES: [SexpType; 24] = [
    SexpType::Nil,
    SexpType::Sym,
    SexpType::List,
    SexpType::Clo,
    SexpType::Env,
    SexpType::Prom,
    SexpType::Lang,
    SexpType::Special,
    SexpType::Builtin,
    SexpType::Char,
    SexpType::Lgl,
    SexpType::Int,
    SexpType::Real,
    SexpType::Cplx,
    SexpType::Str,
    SexpType::Dot,
    SexpType::Any,
    SexpType::Vec,
    SexpType::Expr,
    SexpType::Bcode,
    SexpType::ExtPtr,
    SexpType::WeakRef,
    SexpType::Raw,
    SexpType::Obj,
];

pub(crate) fn decode_type(ctx: &mut Ctx, ty: SEXPTYPE) -> EvalResult<SexpType> {
    TYPES
        .iter()
        .copied()
        .find(|t| *t as SEXPTYPE == ty)
        .ok_or_else(|| ctx.stop(SEXP_NIL, format!("invalid type code {ty}")))
}

pub(crate) fn checked_len(ctx: &mut Ctx, n: R_xlen_t) -> EvalResult<usize> {
    if !(0..=(1 << 52)).contains(&n) {
        return Err(ctx.stop(SEXP_NIL, format!("invalid length {n}")));
    }
    Ok(n as usize)
}

// Limit allocations initiated by C. Several value constructors copy an input buffer.
pub(crate) const MAX_VECTOR_BYTES: usize = 1 << 30;

pub(crate) fn check_capacity(ctx: &mut Ctx, n: usize, width: usize) -> EvalResult<()> {
    if n > MAX_VECTOR_BYTES / width {
        return Err(ctx.stop(SEXP_NIL, "vector allocation exceeds the C API size limit"));
    }
    Ok(())
}

fn filled<T: Copy>(ctx: &mut Ctx, n: usize, value: T) -> EvalResult<Vec<T>> {
    check_capacity(ctx, n, size_of::<T>())?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(n)
        .map_err(|_| ctx.stop(SEXP_NIL, format!("cannot allocate vector of length {n}")))?;
    values.resize(n, value);
    Ok(values)
}

pub(crate) fn alloc_vector(ctx: &mut Ctx, ty: SexpType, n: usize) -> EvalResult<SexpId> {
    macro_rules! make {
        ($method:ident, $fill:expr) => {{
            let values = filled(ctx, n, $fill)?;
            ctx.session_mut().$method(&values)?
        }};
    }
    Ok(match ty {
        SexpType::Nil => SEXP_NIL,
        SexpType::List | SexpType::Lang => return alloc_cells(ctx, n, ty == SexpType::Lang),
        SexpType::Lgl => make!(alloc_lgl_from, RLogical::FALSE),
        SexpType::Int => make!(alloc_int_from, 0),
        SexpType::Real => make!(alloc_real_from, 0.0),
        SexpType::Cplx => make!(alloc_cplx_from, RComplex { re: 0.0, im: 0.0 }),
        SexpType::Raw => make!(alloc_raw_from, 0),
        SexpType::Str => make!(alloc_str_from, CharId::BLANK),
        SexpType::Expr => make!(alloc_expr_from, SEXP_NIL),
        SexpType::Vec => {
            check_capacity(ctx, n, size_of::<SexpId>())?;
            ctx.session_mut().alloc_list(n)?
        }
        _ => {
            return Err(ctx.stop(
                SEXP_NIL,
                format!("cannot allocate vector of type '{}'", ty.type_name(false)),
            ));
        }
    })
}

pub(crate) fn dimensions_length(ctx: &mut Ctx, dims: &[c_int]) -> EvalResult<usize> {
    if dims.iter().any(|&n| n < 0) {
        return Err(ctx.stop(SEXP_NIL, "negative length vectors are not allowed"));
    }
    if dims.contains(&0) {
        return Ok(0);
    }
    dims.iter().try_fold(1usize, |size, &n| {
        size.checked_mul(n as usize)
            .ok_or_else(|| ctx.stop(SEXP_NIL, "array dimensions overflow"))
    })
}

fn alloc_array(ctx: &mut Ctx, ty: SEXPTYPE, dims: &[c_int]) -> EvalResult<Sexp> {
    let ty = decode_type(ctx, ty)?;
    if !is_vector(ty) || dims.is_empty() {
        return Err(ctx.stop(SEXP_NIL, "invalid type or dimensions for array allocation"));
    }
    let size = dimensions_length(ctx, dims)?;
    let vector = alloc_vector(ctx, ty, size)?;
    let session = ctx.session_mut();
    let dim = session.alloc_int_from(dims)?;
    session.set_attrib(vector, SYM_DIM, dim)?;
    Ok(Sexp::from_id(vector))
}

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

#[derive(Default)]
struct LegacyLengths(HashMap<SexpId, R_xlen_t>);
impl CtxExt for LegacyLengths {}
impl Trace for LegacyLengths {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

api! {
    /// Allocate a vector of the specified type and length.
    fn Rf_allocVector(ty: SEXPTYPE, n: R_xlen_t) -> Sexp = Sexp::NULL; |ctx| {
        let n = checked_len(ctx, n)?;
        let ty = decode_type(ctx, ty)?;
        allocating(ctx, &[], |ctx| alloc_vector(ctx, ty, n).map(Sexp::from_id))
    }
    /// Allocate a vector using the default allocator.
    fn Rf_allocVector3(ty: SEXPTYPE, n: R_xlen_t, allocator: *mut c_void) -> Sexp = Sexp::NULL; |ctx| {
        if !allocator.is_null() {
            return Err(ctx.stop(SEXP_NIL, "custom vector allocators are not supported"));
        }
        let n = checked_len(ctx, n)?;
        let ty = decode_type(ctx, ty)?;
        allocating(ctx, &[], |ctx| alloc_vector(ctx, ty, n).map(Sexp::from_id))
    }
    /// Allocate a matrix with the specified dimensions.
    fn Rf_allocMatrix(ty: SEXPTYPE, rows: c_int, cols: c_int) -> Sexp = Sexp::NULL; |ctx| {
        allocating(ctx, &[], |ctx| alloc_array(ctx, ty, &[rows, cols]))
    }
    /// Allocate an array using an integer vector of dimensions.
    fn Rf_allocArray(ty: SEXPTYPE, dims: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let dims = id(ctx, dims, "Rf_allocArray")?;
        allocating(ctx, &[dims], |ctx| {
            let dims = ctx.session().int(dims)?.to_vec();
            alloc_array(ctx, ty, &dims)
        })
    }
    /// Allocate a three-dimensional array.
    fn Rf_alloc3DArray(ty: SEXPTYPE, rows: c_int, cols: c_int, planes: c_int) -> Sexp = Sexp::NULL; |ctx| {
        allocating(ctx, &[], |ctx| alloc_array(ctx, ty, &[rows, cols, planes]))
    }
    /// Return the type code of an R object.
    fn TYPEOF(x: Sexp) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "TYPEOF")?;
        Ok(ctx.session().type_of(x)? as c_int)
    }
    /// Return the legacy vector length metadata.
    fn TRUELENGTH(x: Sexp) -> R_xlen_t = 0; |ctx| {
        let x = id(ctx, x, "TRUELENGTH")?;
        if ctx.session().is_resizable(x)? {
            return Ok(ctx.session().vec_truelength(x)? as R_xlen_t);
        }
        Ok(ctx.ext::<LegacyLengths>().0.get(&x).copied().unwrap_or(0))
    }
    /// Set legacy length metadata without modifying vector storage.
    fn SET_TRUELENGTH(x: Sexp, n: R_xlen_t) -> () = (); |ctx| {
        let x = id(ctx, x, "SET_TRUELENGTH")?;
        if !is_vector(ctx.session().type_of(x)?) || ctx.session().is_resizable(x)? {
            return Err(ctx.stop(SEXP_NIL, "cannot set legacy length metadata on this object"));
        }
        let mut lengths = std::mem::take(&mut ctx.ext::<LegacyLengths>().0);
        lengths.retain(|&key, _| ctx.session().heap.get(key).is_ok());
        if n == 0 { lengths.remove(&x); } else { lengths.insert(x, n); }
        ctx.ext::<LegacyLengths>().0 = lengths;
        Ok(())
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
    /// Set the object bit.
    fn SET_OBJECT(x: Sexp, value: c_int) -> () = (); |ctx| {
        let x = id(ctx, x, "SET_OBJECT")?;
        Ok(ctx.session_mut().set_object_flag(x, value != 0)?)
    }
    /// Set the S4 bit.
    fn SET_S4_OBJECT(x: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "SET_S4_OBJECT")?;
        Ok(ctx.session_mut().set_s4_flag(x, true)?)
    }
    /// Clear the S4 bit.
    fn UNSET_S4_OBJECT(x: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "UNSET_S4_OBJECT")?;
        Ok(ctx.session_mut().set_s4_flag(x, false)?)
    }
    /// Mark the object as shared permanently.
    fn MARK_NOT_MUTABLE(x: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "MARK_NOT_MUTABLE")?;
        ctx.session_mut().heap.mark_immutable(x);
        Ok(())
    }
    /// Return the legacy general-purpose bits.
    fn LEVELS(x: Sexp) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "LEVELS")?;
        Ok(c_int::from(ctx.session().gp(x)?))
    }
    /// Set the legacy general-purpose bits and return the supplied value.
    fn SETLEVELS(x: Sexp, value: c_int) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "SETLEVELS")?;
        ctx.session_mut().set_gp(x, value as u16)?;
        Ok(value)
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
        check_capacity(ctx, n, super::resizable::element_width(ty))?;
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

fn write_ptr(ctx: &mut Ctx, x: SexpId) -> EvalResult<usize> {
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

pub(crate) fn sorted_code(sorted: Sortedness) -> c_int {
    match sorted {
        Sortedness::Unknown => c_int::MIN,
        Sortedness::Unsorted => 0,
        Sortedness::Increasing => 1,
        Sortedness::Decreasing => -1,
        Sortedness::IncreasingNaFirst => 2,
        Sortedness::DecreasingNaFirst => -2,
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
                // The type and index checks establish the valid element address.
                (ptr as *mut $engine).add(i).write(($put)(value));
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
                // Each ABI element has the same layout and valid bits as its engine element.
                let out = out.cast::<$engine>();
                match view {
                    VecView::Slice(values) => {
                        std::ptr::copy(values.as_ptr().add(start), out, count);
                        Ok(count as R_xlen_t)
                    }
                    VecView::Alt { class, heap, .. } => {
                        let buffer = std::slice::from_raw_parts_mut(out.cast::<std::mem::MaybeUninit<$engine>>(), count);
                        buffer.fill(std::mem::MaybeUninit::new($fill));
                        let out = std::slice::from_raw_parts_mut(out, count);
                        Ok(class.get_region(heap, start, out) as R_xlen_t)
                    }
                }
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

macro_rules! metadata {
    ($($ty:ident, $sorted:ident, $no_na:ident);+ $(;)?) => {
        api! {$(
            /// Return known sortedness without materializing the vector.
            fn $sorted(x: Sexp) -> c_int = c_int::MIN; |ctx| {
                let x = id(ctx, x, stringify!($sorted))?;
                require_type(ctx, x, SexpType::$ty, stringify!($sorted))?;
                Ok(sorted_code(ctx.session().alt_is_sorted(x)?))
            }
            /// Test whether the representation guarantees no missing elements.
            fn $no_na(x: Sexp) -> c_int = 0; |ctx| {
                let x = id(ctx, x, stringify!($no_na))?;
                require_type(ctx, x, SexpType::$ty, stringify!($no_na))?;
                Ok(c_int::from(ctx.session().alt_no_na(x)?))
            }
        )+}
    };
}

metadata!(
    Int, INTEGER_IS_SORTED, INTEGER_NO_NA;
    Real, REAL_IS_SORTED, REAL_NO_NA;
    Lgl, LOGICAL_IS_SORTED, LOGICAL_NO_NA;
    Str, STRING_IS_SORTED, STRING_NO_NA;
);

pub(crate) fn is_atomic(ty: SexpType) -> bool {
    use SexpType::*;
    matches!(ty, Lgl | Int | Real | Cplx | Str | Raw)
}

fn is_vector_list(ty: SexpType) -> bool {
    matches!(ty, SexpType::Vec | SexpType::Expr)
}

pub(crate) fn is_vector(ty: SexpType) -> bool {
    is_atomic(ty) || is_vector_list(ty)
}

fn has_dimensions(ctx: &mut Ctx, x: SexpId, rank: Option<usize>) -> EvalResult<bool> {
    if !is_vector(ctx.session().type_of(x)?) {
        return Ok(false);
    }
    let dims = ctx.session().attr(x, SYM_DIM);
    let n = ctx.session().xlength(dims)?;
    Ok(rank.map_or(n > 0, |rank| n == rank))
}

fn factor(ctx: &mut Ctx, x: SexpId) -> EvalResult<bool> {
    Ok(ctx.session().type_of(x)? == SexpType::Int && inherits(ctx, x, b"factor")?)
}

fn vectorizable(ctx: &mut Ctx, x: SexpId) -> EvalResult<bool> {
    let ty = ctx.session().type_of(x)?;
    let items: Vec<SexpId> = match ty {
        SexpType::Nil => return Ok(true),
        SexpType::Vec => ctx.session().list_slice(x)?.to_vec(),
        SexpType::List => cells(ctx, x)?.into_iter().map(|(_, car, _)| car).collect(),
        _ => return Ok(false),
    };
    for item in items {
        let ty = ctx.session().type_of(item)?;
        if ty != SexpType::Nil && (!is_vector(ty) || ctx.session().xlength(item)? > 1) {
            return Ok(false);
        }
    }
    Ok(true)
}

macro_rules! type_predicates {
    ($($name:ident => $($ty:ident)|+),+ $(,)?) => {
        api! {$(
            /// Test the object's R type.
            fn $name(x: Sexp) -> Rboolean = 0; |ctx| {
                let x = id(ctx, x, stringify!($name))?;
                Ok(c_int::from(matches!(ctx.session().type_of(x)?, $(SexpType::$ty)|+)))
            }
        )+}
    };
}

type_predicates!(
    Rf_isNull => Nil, Rf_isSymbol => Sym, Rf_isLogical => Lgl,
    Rf_isReal => Real, Rf_isComplex => Cplx, Rf_isExpression => Expr,
    Rf_isEnvironment => Env, Rf_isString => Str, Rf_isList => Nil | List,
    Rf_isNewList => Nil | Vec, Rf_isPairList => Nil | List | Lang | Dot,
    Rf_isFunction => Clo | Builtin | Special, Rf_isPrimitive => Builtin | Special,
    Rf_isLanguage => Nil | Lang,
);

macro_rules! predicates {
    ($($name:ident => |$ctx:ident, $x:ident| $test:expr),+ $(,)?) => {
        api! {$(
            /// Test an R object property.
            fn $name(value: Sexp) -> Rboolean = 0; |$ctx| {
                let $x = id($ctx, value, stringify!($name))?;
                Ok(c_int::from($test))
            }
        )+}
    };
}

predicates!(
    OBJECT => |ctx, x| ctx.session().is_object(x)?,
    Rf_isObject => |ctx, x| ctx.session().is_object(x)?,
    IS_S4_OBJECT => |ctx, x| ctx.session().is_s4(x)?,
    Rf_isS4 => |ctx, x| ctx.session().is_s4(x)?,
    ALTREP => |ctx, x| ctx.session().is_altrep(x)?,
    MAYBE_REFERENCED => |ctx, x| ctx.session().heap.maybe_referenced(x),
    MAYBE_SHARED => |ctx, x| ctx.session().heap.maybe_shared(x),
    NOT_SHARED => |ctx, x| !ctx.session().heap.maybe_shared(x),
    NO_REFERENCES => |ctx, x| !ctx.session().heap.maybe_referenced(x),
    IS_LONG_VEC => |ctx, x| length(ctx, x)? > c_int::MAX as usize,
    Rf_isInteger => |ctx, x| ctx.session().type_of(x)? == SexpType::Int && !factor(ctx, x)?,
    Rf_isNumeric => |ctx, x| matches!(ctx.session().type_of(x)?, SexpType::Lgl | SexpType::Int | SexpType::Real)
        && !factor(ctx, x)?,
    Rf_isNumber => |ctx, x| matches!(ctx.session().type_of(x)?,
        SexpType::Lgl | SexpType::Int | SexpType::Real | SexpType::Cplx) && !factor(ctx, x)?,
    Rf_isFactor => |ctx, x| factor(ctx, x)?,
    Rf_isVector => |ctx, x| is_vector(ctx.session().type_of(x)?),
    Rf_isVectorAtomic => |ctx, x| is_atomic(ctx.session().type_of(x)?),
    Rf_isVectorList => |ctx, x| is_vector_list(ctx.session().type_of(x)?),
    Rf_isVectorizable => |ctx, x| vectorizable(ctx, x)?,
    Rf_isMatrix => |ctx, x| has_dimensions(ctx, x, Some(2))?,
    Rf_isArray => |ctx, x| has_dimensions(ctx, x, None)?,
    Rf_isFrame => |ctx, x| inherits(ctx, x, b"data.frame")?,
    Rf_isDataFrame => |ctx, x| inherits(ctx, x, b"data.frame")?,
    Rf_isOrdered => |ctx, x| factor(ctx, x)? && inherits(ctx, x, b"ordered")?,
    Rf_isUnordered => |ctx, x| factor(ctx, x)? && !inherits(ctx, x, b"ordered")?,
    Rf_isTs => |ctx, x| ctx.session().attr(x, roxide_value::SYM_TSP) != SEXP_NIL,
    Rf_isScalarString => |ctx, x| ctx.session().type_of(x)? == SexpType::Str && length(ctx, x)? == 1,
    Rf_isValidString => |ctx, x| ctx.session().type_of(x)? == SexpType::Str && length(ctx, x)? > 0,
    Rf_isValidStringF => |ctx, x| {
        if ctx.session().type_of(x)? != SexpType::Str || length(ctx, x)? == 0 {
            false
        } else {
            let s = ctx.session_mut();
            let ch = s.str_get(x, 0)?;
            !s.char_bytes(ch)?.is_empty()
        }
    },
);

fn type_name(ty: SEXPTYPE) -> Option<&'static std::ffi::CStr> {
    Some(match ty {
        NILSXP => c"NULL",
        SYMSXP => c"symbol",
        LISTSXP => c"pairlist",
        CLOSXP => c"closure",
        ENVSXP => c"environment",
        PROMSXP => c"promise",
        LANGSXP => c"language",
        SPECIALSXP => c"special",
        BUILTINSXP => c"builtin",
        CHARSXP => c"char",
        LGLSXP => c"logical",
        INTSXP => c"integer",
        REALSXP => c"double",
        CPLXSXP => c"complex",
        STRSXP => c"character",
        DOTSXP => c"...",
        ANYSXP => c"any",
        VECSXP => c"list",
        EXPRSXP => c"expression",
        BCODESXP => c"bytecode",
        EXTPTRSXP => c"externalptr",
        WEAKREFSXP => c"weakref",
        RAWSXP => c"raw",
        OBJSXP => c"S4",
        _ => return None,
    })
}

fn type_string(ctx: &mut Ctx, ty: SEXPTYPE, warn: bool, scalar: bool) -> EvalResult<Sexp> {
    allocating(ctx, &[], |ctx| {
        if warn && type_name(ty).is_none() {
            ctx.warning_at(
                SEXP_NIL,
                &format!("type {ty} is unimplemented in 'type2str'"),
            )?;
        }
        let name = type_name(ty).unwrap_or(c"unknown");
        let s = ctx.session_mut();
        let ch = s.mk_char(name.to_bytes(), Encoding::Native)?;
        let x = if scalar {
            s.alloc_str_from(&[ch])?
        } else {
            ch.as_sexp()
        };
        Ok(Sexp::from_id(x))
    })
}

api! {
    /// Return a static C string for the type name.
    fn Rf_type2char(ty: SEXPTYPE) -> *const c_char = null(); |ctx| {
        if let Some(name) = type_name(ty) { return Ok(name.as_ptr()); }
        ctx.warning_at(SEXP_NIL, &format!("type {ty} is unimplemented in 'type2char'"))?;
        Ok(c"unknown".as_ptr())
    }
    /// Return the type name as an interned string.
    fn Rf_type2str(ty: SEXPTYPE) -> Sexp = Sexp::NULL; |ctx| {
        type_string(ctx, ty, true, false)
    }
    /// Return the type name as an interned string without a warning.
    fn Rf_type2str_nowarn(ty: SEXPTYPE) -> Sexp = Sexp::NULL; |ctx| {
        type_string(ctx, ty, false, false)
    }
    /// Return the type name as a character vector of length one.
    fn Rf_type2rstr(ty: SEXPTYPE) -> Sexp = Sexp::NULL; |ctx| {
        type_string(ctx, ty, true, true)
    }
    /// Look up the code for a type name.
    fn Rf_str2type(name: *const c_char) -> c_int = -1; |ctx| {
        let bytes = super::string::c_text(ctx, name, "Rf_str2type")?;
        if bytes == b"name" { return Ok(SYMSXP as c_int); }
        Ok(TYPES.iter().find(|&&t| type_name(t as SEXPTYPE).is_some_and(|s| s.to_bytes() == bytes))
            .map_or(-1, |&t| t as c_int))
    }
}

api! {
    /// Return a vector with the specified length.
    fn Rf_lengthgets(x: Sexp, n: R_len_t) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_lengthgets")?;
        let n = checked_len(ctx, n as R_xlen_t)?;
        allocating(ctx, &[x], |ctx| super::resizable::set_length(ctx, x, n).map(Sexp::from_id))
    }
    /// Return a vector with the specified long length.
    fn Rf_xlengthgets(x: Sexp, n: R_xlen_t) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_xlengthgets")?;
        let n = checked_len(ctx, n)?;
        allocating(ctx, &[x], |ctx| super::resizable::set_length(ctx, x, n).map(Sexp::from_id))
    }
}

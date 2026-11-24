use std::ffi::{c_char, c_int, c_void};

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, RComplex, RLogical, SEXP_NIL, SYM_DIM, SYM_NAMES, SexpType, ValueApi,
};

use super::access::write_ptr;
use super::{allocating, api, decode_type, id, is_vector, read_ptr, require_type};
use crate::abi::*;
use crate::api::pairlist::{alloc_cells, cells};
use crate::api::resizable;
use crate::api::string::c_text;

pub(crate) fn checked_len(ctx: &mut Ctx, n: R_xlen_t) -> EvalResult<usize> {
    if !(0..=(1 << 52)).contains(&n) {
        return Err(ctx.stop(SEXP_NIL, format!("invalid length {n}")));
    }
    Ok(n as usize)
}

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

unsafe fn named_vector(
    ctx: &mut Ctx,
    ty: SexpType,
    names: *const *const c_char,
) -> EvalResult<Sexp> {
    if names.is_null() {
        return Err(ctx.stop(SEXP_NIL, "invalid NULL names array"));
    }
    allocating(ctx, &[], |ctx| {
        let mut labels = Vec::new();
        loop {
            // SAFETY: The caller supplies pointers through the terminating empty string.
            let name = unsafe { names.add(labels.len()).read() };
            if name.is_null() {
                break;
            }
            // SAFETY: Each entry is a readable NUL-terminated string.
            let bytes = unsafe { c_text(ctx, name, "Rf_mkNamed")? };
            if bytes.is_empty() {
                break;
            }
            check_capacity(ctx, labels.len() + 1, size_of::<CharId>())?;
            let name = ctx.session_mut().mk_char(bytes, Encoding::Native)?;
            labels.push(name);
        }
        let value = alloc_vector(ctx, ty, labels.len())?;
        let s = ctx.session_mut();
        let names = s.alloc_str_from(&labels)?;
        s.set_attrib(value, SYM_NAMES, names)?;
        Ok(Sexp::from_id(value))
    })
}

#[derive(Clone, Copy)]
enum CopyOrder {
    Linear,
    ByRow,
}

fn matrix_dimensions(ctx: &mut Ctx, target: SexpId, size: usize) -> EvalResult<(usize, usize)> {
    let dims = ctx.session().int(ctx.session().attr(target, SYM_DIM))?;
    if dims.len() != 2
        || dims.get(0) <= 0
        || dims.get(1) <= 0
        || (dims.get(0) as usize).checked_mul(dims.get(1) as usize) != Some(size)
    {
        return Err(ctx.stop(SEXP_NIL, "invalid matrix dimensions"));
    }
    Ok((dims.get(0) as usize, dims.get(1) as usize))
}

fn recycle_index(i: usize, count: usize, dimensions: Option<(usize, usize)>) -> usize {
    match dimensions {
        Some((rows, columns)) => ((i % rows) * columns + i / rows) % count,
        None => i % count,
    }
}

fn copy_list_matrix(ctx: &mut Ctx, target: SexpId, source: SexpId, byrow: bool) -> EvalResult<()> {
    if target == SEXP_NIL {
        return Ok(());
    }
    require_type(ctx, target, SexpType::List, "Rf_copyListMatrix")?;
    require_type(ctx, source, SexpType::List, "Rf_copyListMatrix")?;
    let target_cells = cells(ctx, target)?;
    let source_cells = cells(ctx, source)?;
    let dims = if byrow {
        Some(matrix_dimensions(ctx, target, target_cells.len())?)
    } else {
        None
    };
    // GNU R copies alternate positions of the row-filled matrix into the start of the pairlist.
    let stride = if byrow { 2 } else { 1 };
    for ((cell, _, _), i) in target_cells
        .iter()
        .zip((0..target_cells.len()).step_by(stride))
    {
        let (_, value, _) = source_cells[recycle_index(i, source_cells.len(), dims)];
        let s = ctx.session_mut();
        let value = s.deep_duplicate(value)?;
        s.set_car(*cell, value)?;
    }
    Ok(())
}

fn copy_vector(ctx: &mut Ctx, target: SexpId, source: SexpId, order: CopyOrder) -> EvalResult<()> {
    let ty = ctx.session().type_of(target)?;
    if !is_vector(ty) || ctx.session().type_of(source)? != ty {
        return Err(ctx.stop(SEXP_NIL, "vector types must match"));
    }
    let size = ctx.session().xlength(target)?;
    let count = ctx.session().xlength(source)?;
    if size == 0 {
        return Ok(());
    }
    if count == 0 {
        return Err(ctx.stop(SEXP_NIL, "cannot recycle a vector of length zero"));
    }
    let dimensions = match order {
        CopyOrder::Linear => None,
        CopyOrder::ByRow => Some(matrix_dimensions(ctx, target, size)?),
    };
    let index = |i| recycle_index(i, count, dimensions);
    if target == source && dimensions.is_none() {
        return Ok(());
    }
    let width = resizable::element_width(ty);
    check_capacity(ctx, count, width)?;
    check_capacity(ctx, size, width)?;
    match ty {
        SexpType::Vec | SexpType::Expr => {
            let s = ctx.session_mut();
            let values = s.list_slice(source)?.to_vec();
            for i in 0..size {
                s.list_set(target, i, values[index(i)])?;
            }
        }
        SexpType::Str => {
            let s = ctx.session_mut();
            let values = s.str_dataptr_ro(source)?.to_vec();
            for i in 0..size {
                s.str_set(target, i, values[index(i)])?;
            }
        }
        _ => {
            let ptr = read_ptr(ctx, source, ty)?.cast::<u8>();
            // SAFETY: read_ptr provides count initialized elements of this atomic type.
            let bytes = unsafe { std::slice::from_raw_parts(ptr, count * width) }.to_vec();
            let ptr = write_ptr(ctx, target)? as *mut u8;
            for i in 0..size {
                // SAFETY: Each offset is within its buffer. The source is an independent copy.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        bytes.as_ptr().add(index(i) * width),
                        ptr.add(i * width),
                        width,
                    )
                };
            }
        }
    }
    Ok(())
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
    /// Allocate a vector with names from a terminated C string array.
    fn Rf_mkNamed(ty: SEXPTYPE, names: *mut *const c_char) -> Sexp = Sexp::NULL; |ctx| {
        let ty = decode_type(ctx, ty)?;
        named_vector(ctx, ty, names)
    }
    /// Copy and recycle the elements of one vector into another.
    fn Rf_copyVector(target: Sexp, source: Sexp) -> () = (); |ctx| {
        let target = id(ctx, target, "Rf_copyVector")?;
        let source = id(ctx, source, "Rf_copyVector")?;
        allocating(ctx, &[target, source], |ctx| copy_vector(ctx, target, source, CopyOrder::Linear))
    }
    /// Fill a matrix by row or column and recycle its source elements.
    fn Rf_copyMatrix(target: Sexp, source: Sexp, byrow: Rboolean) -> () = (); |ctx| {
        let target = id(ctx, target, "Rf_copyMatrix")?;
        let source = id(ctx, source, "Rf_copyMatrix")?;
        let order = if byrow == 0 { CopyOrder::Linear } else { CopyOrder::ByRow };
        allocating(ctx, &[target, source], |ctx| copy_vector(ctx, target, source, order))
    }
    /// Fill a list matrix by row or column and recycle its source elements.
    fn Rf_copyListMatrix(target: Sexp, source: Sexp, byrow: Rboolean) -> () = (); |ctx| {
        let target = id(ctx, target, "Rf_copyListMatrix")?;
        let source = id(ctx, source, "Rf_copyListMatrix")?;
        allocating(ctx, &[target, source], |ctx| copy_list_matrix(ctx, target, source, byrow != 0))
    }
}

api! {
    /// Return a vector with the specified length.
    fn Rf_lengthgets(x: Sexp, n: R_len_t) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_lengthgets")?;
        let n = checked_len(ctx, n as R_xlen_t)?;
        allocating(ctx, &[x], |ctx| resizable::set_length(ctx, x, n).map(Sexp::from_id))
    }
    /// Return a vector with the specified long length.
    fn Rf_xlengthgets(x: Sexp, n: R_xlen_t) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_xlengthgets")?;
        let n = checked_len(ctx, n)?;
        allocating(ctx, &[x], |ctx| resizable::set_length(ctx, x, n).map(Sexp::from_id))
    }
}

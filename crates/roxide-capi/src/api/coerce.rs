//! Type coercion (`Rf_coerceVector`, `Rf_as*`, `Rf_Scalar*`, `Rf_duplicate`).

use std::ffi::{c_char, c_int};

use roxide_eval::{AltOp, AltReply, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, NA_INTEGER, NA_REAL, RComplex, RLogical, SEXP_NIL, SYM_LEVELS, SYM_NAMES,
    SexpType, ValueApi,
};

use super::pairlist::cells;
use super::string::{c_text, utf8};
use super::sym::{install, print_name};
use super::vector::{
    allocating, api, check_capacity, complex_from_c, complex_to_c, decode_type, id, is_atomic,
    is_vector,
};
use crate::abi::{Rboolean, Rbyte, Rcomplex, Sexp};

pub(crate) fn scalar_element(
    ctx: &mut Ctx,
    x: SexpId,
    ty: SexpType,
    i: usize,
) -> EvalResult<SexpId> {
    let s = ctx.session_mut();
    Ok(match ty {
        SexpType::Lgl => {
            let v = s.lgl(x)?.get(i);
            s.scalar_lgl(v)?
        }
        SexpType::Int => {
            let v = s.int(x)?.get(i);
            s.scalar_int(v)?
        }
        SexpType::Real => {
            let v = s.real(x)?.get(i);
            s.scalar_real(v)?
        }
        SexpType::Cplx => {
            let v = s.cplx(x)?.get(i);
            s.alloc_cplx_from(&[v])?
        }
        SexpType::Raw => {
            let v = s.raw(x)?.get(i);
            s.alloc_raw_from(&[v])?
        }
        SexpType::Str => {
            let v = s.str_get(x, i)?;
            s.alloc_str_from(&[v])?
        }
        SexpType::Vec | SexpType::Expr => s.list_get(x, i)?,
        _ => x,
    })
}

fn to_pairlist(ctx: &mut Ctx, x: SexpId) -> EvalResult<SexpId> {
    let ty = ctx.session().type_of(x)?;
    if matches!(ty, SexpType::Nil | SexpType::List) {
        return Ok(x);
    }
    if !is_vector(ty) {
        return Err(ctx.stop(SEXP_NIL, "cannot coerce object to a pairlist"));
    }
    let names = ctx.session().attr(x, SYM_NAMES);
    let n = ctx.session().xlength(x)?;
    check_capacity(ctx, n, size_of::<roxide_value::RObject>())?;
    let mut tail = SEXP_NIL;
    for i in (0..n).rev() {
        let tag = if names == SEXP_NIL {
            SEXP_NIL
        } else {
            let ch = ctx.session_mut().str_get(names, i)?;
            let s = ctx.session();
            let bytes = s.char_bytes(ch)?;
            if bytes.is_empty() {
                SEXP_NIL
            } else {
                let name = utf8(bytes, s.char_encoding(ch)?).into_owned();
                install(ctx, &name)?
            }
        };
        let car = scalar_element(ctx, x, ty, i)?;
        tail = ctx.session_mut().cons(car, tail, tag)?;
    }
    Ok(tail)
}

fn to_vector_list(ctx: &mut Ctx, x: SexpId) -> EvalResult<SexpId> {
    let ty = ctx.session().type_of(x)?;
    if ty == SexpType::Vec {
        return Ok(x);
    }
    if ty == SexpType::Expr {
        let items = ctx.session().list_slice(x)?.to_vec();
        let s = ctx.session_mut();
        let list = s.alloc_list(items.len())?;
        for (i, item) in items.into_iter().enumerate() {
            s.list_set(list, i, item)?;
        }
        s.set_attrib(list, SYM_NAMES, s.attr(x, SYM_NAMES))?;
        return Ok(list);
    }
    let elements = cells(ctx, x)?;
    let list = ctx.session_mut().alloc_list(elements.len())?;
    let mut names = Vec::new();
    if elements.iter().any(|&(_, _, tag)| tag != SEXP_NIL) {
        names.reserve(elements.len());
        for &(_, _, tag) in &elements {
            names.push(if tag == SEXP_NIL {
                CharId::BLANK
            } else {
                print_name(ctx, tag)?
            });
        }
    }
    let s = ctx.session_mut();
    for (i, &(_, car, _)) in elements.iter().enumerate() {
        s.list_set(list, i, car)?;
    }
    if !names.is_empty() {
        let names = s.alloc_str_from(&names)?;
        s.set_attrib(list, SYM_NAMES, names)?;
    }
    Ok(list)
}

fn coerce_value(ctx: &mut Ctx, x: SexpId, ty: SexpType) -> EvalResult<SexpId> {
    let (result, warn) = ctx.session_mut().coerce(x, ty)?;
    ctx.scoped(|ctx| {
        ctx.protect(result);
        if warn.na_introduced {
            ctx.warning_at(SEXP_NIL, "NAs introduced by coercion")?;
        }
        if warn.imaginary_discarded {
            ctx.warning_at(SEXP_NIL, "imaginary parts discarded in coercion")?;
        }
        Ok(result)
    })
}

fn coerce_vector(ctx: &mut Ctx, x: SexpId, to: SexpType) -> EvalResult<SexpId> {
    use SexpType::*;
    let from = ctx.session().type_of(x)?;
    if from == to || to == Any {
        return Ok(x);
    }
    if ctx
        .session()
        .foreign_alt(x)?
        .is_some_and(|info| !info.owned)
    {
        if let AltReply::Value(Some(value)) = super::altrep::dispatch(ctx, AltOp::Coerce { x, to })?
        {
            return Ok(value);
        }
        super::altrep::materialize(ctx, x, super::altrep::Access::Evaluate)?;
    }
    if from == Str {
        ctx.session_mut().str_dataptr_ro(x)?;
    }
    if to == List {
        return to_pairlist(ctx, x);
    }
    if to == Sym && matches!(from, Str | Char) {
        let ch = if from == Char {
            ctx.session().sexp_as_char(x)?
        } else if ctx.session().xlength(x)? > 0 {
            ctx.session_mut().str_get(x, 0)?
        } else {
            return Err(ctx.stop(
                SEXP_NIL,
                "invalid type/length (symbol/0) in vector allocation",
            ));
        };
        let s = ctx.session();
        let name = utf8(s.char_bytes(ch)?, s.char_encoding(ch)?).into_owned();
        return install(ctx, &name);
    }
    if to == Expr {
        return if from == Vec {
            let items = ctx.session().list_slice(x)?.to_vec();
            let s = ctx.session_mut();
            let out = s.alloc_expr_from(&items)?;
            s.set_attrib(out, SYM_NAMES, s.attr(x, SYM_NAMES))?;
            Ok(out)
        } else {
            Ok(ctx.session_mut().alloc_expr_from(&[x])?)
        };
    }
    if matches!(from, Nil | List | Lang | Dot | Expr) && to == Vec {
        return to_vector_list(ctx, x);
    }
    let arg = if matches!(from, List | Dot | Expr) && is_atomic(to) {
        to_vector_list(ctx, x)?
    } else {
        x
    };
    if arg != x {
        ctx.protect(arg);
    }
    let result = coerce_value(ctx, arg, to)?;
    if is_atomic(from) && is_atomic(to) {
        let s = ctx.session_mut();
        s.replace_attributes(result, s.attrs(x)?)?;
        s.set_s4_flag(result, s.is_s4(x)?)?;
        s.set_object_flag(result, s.is_object(x)?)?;
    } else if from == List && is_atomic(to) {
        let s = ctx.session_mut();
        s.set_attrib(result, SYM_NAMES, s.attr(arg, SYM_NAMES))?;
    }
    Ok(result)
}

fn scalar_coerce(ctx: &mut Ctx, x: SexpId, to: SexpType) -> EvalResult<SexpId> {
    let ty = ctx.session().type_of(x)?;
    if !is_atomic(ty) || ctx.session().xlength(x)? == 0 {
        return Ok(SEXP_NIL);
    }
    if ty == to {
        return Ok(x);
    }
    let scalar = scalar_element(ctx, x, ty, 0)?;
    coerce_value(ctx, scalar, to)
}

fn as_logical(ctx: &mut Ctx, x: SexpId) -> EvalResult<c_int> {
    if ctx.session().type_of(x)? == SexpType::Lgl {
        let v = ctx.session().lgl(x)?;
        return Ok(if v.is_empty() {
            NA_INTEGER
        } else {
            v.get(0).raw()
        });
    }
    allocating(ctx, &[x], |ctx| {
        let out = scalar_coerce(ctx, x, SexpType::Lgl)?;
        Ok(if out == SEXP_NIL {
            NA_INTEGER
        } else {
            ctx.session().lgl(out)?.get(0).raw()
        })
    })
}

fn as_bool(ctx: &mut Ctx, x: SexpId) -> EvalResult<bool> {
    if is_atomic(ctx.session().type_of(x)?) {
        let len = ctx.session().xlength(x)?;
        if len > 1 {
            return Err(ctx.stop(
                SEXP_NIL,
                format!("'length = {len}' in coercion to 'logical(1)'"),
            ));
        }
    }
    match as_logical(ctx, x)? {
        NA_INTEGER => Err(ctx.stop(SEXP_NIL, "NA in coercion to boolean")),
        value => Ok(value != 0),
    }
}

macro_rules! as_scalars {
    ($($name:ident => $ty:ident, $result:ty, $view:ident, $missing:expr, $get:expr);+ $(;)?) => {
        api! {$(
            /// Convert the first element to the specified atomic type.
            fn $name(x: Sexp) -> $result = $missing; |ctx| {
                let x = id(ctx, x, stringify!($name))?;
                if ctx.session().type_of(x)? == SexpType::$ty {
                    let v = ctx.session().$view(x)?;
                    return Ok(if v.is_empty() { $missing } else { ($get)(v.get(0)) });
                }
                allocating(ctx, &[x], |ctx| {
                    let out = scalar_coerce(ctx, x, SexpType::$ty)?;
                    Ok(if out == SEXP_NIL { $missing } else { ($get)(ctx.session().$view(out)?.get(0)) })
                })
            }
        )+}
    };
}

as_scalars!(
    Rf_asInteger => Int, c_int, int, NA_INTEGER, std::convert::identity;
    Rf_asReal => Real, f64, real, NA_REAL, std::convert::identity;
    Rf_asComplex => Cplx, Rcomplex, cplx, complex_to_c(RComplex::NA), complex_to_c;
);

api! {
    /// Coerce an R object to the specified type.
    fn Rf_coerceVector(x: Sexp, ty: crate::abi::SEXPTYPE) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_coerceVector")?;
        let ty = decode_type(ctx, ty)?;
        allocating(ctx, &[x], |ctx| coerce_vector(ctx, x, ty).map(Sexp::from_id))
    }
    /// Convert the first element to a logical value.
    fn Rf_asLogical(x: Sexp) -> c_int = NA_INTEGER; |ctx| {
        let x = id(ctx, x, "Rf_asLogical")?;
        as_logical(ctx, x)
    }
    /// Test whether the object is a logical vector with one TRUE element.
    fn R_isTRUE(x: Sexp) -> bool = false; |ctx| {
        let x = id(ctx, x, "R_isTRUE")?;
        if ctx.session().type_of(x)? != SexpType::Lgl {
            return Ok(false);
        }
        let value = ctx.session().lgl(x)?;
        Ok(value.len() == 1 && value.get(0) == RLogical::TRUE)
    }
    /// Coerce an R value to a two-state C boolean.
    fn Rf_asBool(x: Sexp) -> bool = false; |ctx| {
        let x = id(ctx, x, "Rf_asBool")?;
        as_bool(ctx, x)
    }
    /// Coerce an R value to a two-state Rboolean.
    fn Rf_asRboolean(x: Sexp) -> Rboolean = 0; |ctx| {
        let x = id(ctx, x, "Rf_asRboolean")?;
        Ok(c_int::from(as_bool(ctx, x)?))
    }
    /// Convert the first element to a logical value, with optional length checking.
    fn Rf_asLogical2(x: Sexp, checking: c_int, call: Sexp) -> c_int = NA_INTEGER; |ctx| {
        let x = id(ctx, x, "Rf_asLogical2")?;
        let call = id(ctx, call, "Rf_asLogical2")?;
        ctx.scoped(|ctx| {
            ctx.protect(call);
            if checking != 0 && is_atomic(ctx.session().type_of(x)?) && ctx.session().xlength(x)? > 1 {
                return Err(ctx.stop(call, "'length > 1' in coercion to 'logical(1)'"));
            }
            as_logical(ctx, x)
        })
    }
    /// Convert the first element to an interned string.
    fn Rf_asChar(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_asChar")?;
        allocating(ctx, &[x], |ctx| {
            let ch = match ctx.session().type_of(x)? {
                SexpType::Char => ctx.session().sexp_as_char(x)?,
                SexpType::Sym => print_name(ctx, x)?,
                SexpType::Int if super::attrib::inherits(ctx, x, b"factor")? => {
                    let codes = ctx.session().int(x)?;
                    let code = if codes.is_empty() { NA_INTEGER } else { codes.get(0) };
                    let levels = ctx.session().attr(x, SYM_LEVELS);
                    if code <= 0 || code as usize > ctx.session().xlength(levels)? { CharId::NA }
                    else { ctx.session_mut().str_get(levels, code as usize - 1)? }
                }
                _ => {
                    let value = scalar_coerce(ctx, x, SexpType::Str)?;
                    if value == SEXP_NIL { CharId::NA } else { ctx.session_mut().str_get(value, 0)? }
                }
            };
            Ok(Sexp::from_id(ch.as_sexp()))
        })
    }
    /// Make a character vector from an interned string.
    fn Rf_ScalarString(value: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, value, "Rf_ScalarString")?;
        allocating(ctx, &[x], |ctx| {
            let s = ctx.session_mut();
            let ch = s.sexp_as_char(x)?;
            Ok(Sexp::from_id(s.alloc_str_from(&[ch])?))
        })
    }
    /// Make a character vector from a NUL-terminated string.
    fn Rf_mkString(text: *const c_char) -> Sexp = Sexp::NULL; |ctx| {
        let bytes = c_text(ctx, text, "Rf_mkString")?;
        check_capacity(ctx, bytes.len(), 1)?;
        let bytes = bytes.to_vec();
        allocating(ctx, &[], |ctx| {
            let s = ctx.session_mut();
            let ch = s.mk_char(&bytes, Encoding::Native)?;
            Ok(Sexp::from_id(s.alloc_str_from(&[ch])?))
        })
    }
    /// Return the number of levels of a factor.
    fn Rf_nlevels(x: Sexp) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "Rf_nlevels")?;
        let s = ctx.session();
        let n = s.xlength(s.attr(x, SYM_LEVELS))?;
        c_int::try_from(n).map_err(|_| ctx.stop(SEXP_NIL, "too many factor levels"))
    }
    /// Coerce a vector to a pairlist.
    fn Rf_VectorToPairList(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_VectorToPairList")?;
        allocating(ctx, &[x], |ctx| to_pairlist(ctx, x).map(Sexp::from_id))
    }
    /// Coerce a pairlist to a list vector.
    fn Rf_PairToVectorList(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_PairToVectorList")?;
        allocating(ctx, &[x], |ctx| to_vector_list(ctx, x).map(Sexp::from_id))
    }
}

macro_rules! make_scalars {
    ($($name:ident($ty:ty) => $alloc:ident, $arg:expr);+ $(;)?) => {
        api! {$(
            /// Allocate an atomic vector of length one.
            fn $name(value: $ty) -> Sexp = Sexp::NULL; |ctx| {
                allocating(ctx, &[], |ctx| {
                    Ok(Sexp::from_id(ctx.session_mut().$alloc(($arg)(value))?))
                })
            }
        )+}
    };
}

make_scalars!(
    Rf_ScalarLogical(c_int) => scalar_lgl, RLogical::from_raw;
    Rf_ScalarInteger(c_int) => scalar_int, std::convert::identity;
    Rf_ScalarReal(f64) => scalar_real, std::convert::identity;
);

api! {
    /// Allocate a complex vector of length one.
    fn Rf_ScalarComplex(value: Rcomplex) -> Sexp = Sexp::NULL; |ctx| {
        allocating(ctx, &[], |ctx| {
            Ok(Sexp::from_id(ctx.session_mut().alloc_cplx_from(&[complex_from_c(value)])?))
        })
    }
    /// Allocate a raw vector of length one.
    fn Rf_ScalarRaw(value: Rbyte) -> Sexp = Sexp::NULL; |ctx| {
        allocating(ctx, &[], |ctx| {
            Ok(Sexp::from_id(ctx.session_mut().alloc_raw_from(&[value])?))
        })
    }
}

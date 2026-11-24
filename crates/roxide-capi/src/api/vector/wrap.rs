use std::collections::HashMap;
use std::ffi::{c_char, c_int};
use std::ptr::null;

use roxide_eval::{Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{Encoding, SEXP_NIL, SYM_DIM, SexpType, Sortedness, ValueApi};

use super::{allocating, api, id, is_atomic, is_vector, is_vector_list, length, require_type};
use crate::abi::*;
use crate::api::attrib::{copy_attrs, inherits};
use crate::api::pairlist::cells;
use crate::api::string::c_text;

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

#[derive(Default)]
struct LegacyLengths(HashMap<SexpId, R_xlen_t>);
impl CtxExt for LegacyLengths {}
impl Trace for LegacyLengths {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

api! {
    /// Wrap an atomic vector in an ALTREP object when possible.
    fn R_tryWrap(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let value = id(ctx, x, "R_tryWrap")?;
        let ty = ctx.session().type_of(value)?;
        if !is_atomic(ty) {
            return Ok(x);
        }
        allocating(ctx, &[value], |ctx| {
            if ty == SexpType::Str { ctx.session_mut().str_dataptr_ro(value)?; }
            let result = ctx.session_mut().alloc_alt_wrapper(value)?;
            copy_attrs(ctx, value, result, false)?;
            Ok(Sexp::from_id(result))
        })
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
        let bytes = c_text(ctx, name, "Rf_str2type")?;
        if bytes == b"name" { return Ok(SYMSXP as c_int); }
        Ok(TYPES.iter().find(|&&t| type_name(t as SEXPTYPE).is_some_and(|s| s.to_bytes() == bytes))
            .map_or(-1, |&t| t as c_int))
    }
}

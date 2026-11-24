//! Matching, duplicate detection, identity, and sortedness.

use std::ffi::c_int;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    BASE_ENV, CharId, IdenticalOpts, RLogical, RSession, SEXP_NIL, SexpType, ValueApi, identical,
};

use super::string::utf8;
use super::vector::{allocating, api, id};
use crate::abi::{R_xlen_t, Rboolean, Sexp};

fn duplicates(
    ctx: &mut Ctx,
    x: Sexp,
    incomparables: Sexp,
    fromlast: Rboolean,
    first_only: bool,
) -> EvalResult<SexpId> {
    let x = id(ctx, x, "Rf_duplicated")?;
    let incomparables = id(ctx, incomparables, "Rf_duplicated")?;
    let fromlast = ctx
        .session_mut()
        .scalar_lgl(RLogical::from_option(Some(fromlast != 0)))?;
    let fromlast_arg = ctx.session_mut().intern("fromLast")?;
    let function = if first_only {
        "anyDuplicated.default"
    } else {
        "duplicated.default"
    };
    ctx.call_function(
        function,
        &[
            (None, x),
            (None, incomparables),
            (Some(fromlast_arg), fromlast),
        ],
        BASE_ENV,
    )
}

fn index(session: &RSession, x: SexpId) -> EvalResult<R_xlen_t> {
    Ok(match session.type_of(x)? {
        SexpType::Int => session.int(x)?.get(0) as R_xlen_t,
        _ => session.real(x)?.get(0) as R_xlen_t,
    })
}

fn out_of_order<T: PartialOrd>(values: impl IntoIterator<Item = T>, strict: bool) -> bool {
    let mut values = values.into_iter();
    let Some(mut previous) = values.next() else {
        return false;
    };
    for next in values {
        if previous > next || strict && previous == next {
            return true;
        }
        previous = next;
    }
    false
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum SortChar {
    Text(Vec<u8>),
    Missing,
}

fn unsorted(ctx: &mut Ctx, x: SexpId, strict: bool) -> EvalResult<bool> {
    let session = ctx.session_mut();
    Ok(match session.type_of(x)? {
        SexpType::Lgl => out_of_order(session.lgl(x)?.iter().map(RLogical::raw), strict),
        SexpType::Int => out_of_order(session.int(x)?.iter(), strict),
        SexpType::Real => out_of_order(session.real(x)?.iter(), strict),
        SexpType::Cplx => out_of_order(session.cplx(x)?.iter().map(|v| (v.re, v.im)), strict),
        SexpType::Raw => out_of_order(session.raw(x)?.iter(), strict),
        SexpType::Str => {
            let mut previous = None;
            for i in 0..session.xlength(x)? {
                let ch = session.str_get(x, i)?;
                let next = if ch == CharId::NA {
                    SortChar::Missing
                } else {
                    SortChar::Text(
                        utf8(session.char_bytes(ch)?, session.char_encoding(ch)?).into_owned(),
                    )
                };
                if previous
                    .as_ref()
                    .is_some_and(|previous| previous > &next || strict && previous == &next)
                {
                    return Ok(true);
                }
                previous = Some(next);
            }
            false
        }
        _ => return Err(ctx.stop(SEXP_NIL, "only atomic vectors can be tested to be sorted")),
    })
}

api! {
    /// Return the index of the first duplicate, or zero.
    fn Rf_any_duplicated(x: Sexp, fromlast: Rboolean) -> R_xlen_t = 0; |ctx| {
        let answer = duplicates(ctx, x, Sexp::from_id(SEXP_NIL), fromlast, true)?;
        index(ctx.session(), answer)
    }

    /// Return the first duplicate index after excluding incomparable values.
    fn Rf_any_duplicated3(x: Sexp, incomparables: Sexp, fromlast: Rboolean) -> R_xlen_t = 0; |ctx| {
        let answer = duplicates(ctx, x, incomparables, fromlast, true)?;
        index(ctx.session(), answer)
    }

    /// Return one logical value per input element to identify duplicates.
    fn Rf_duplicated(x: Sexp, fromlast: Rboolean) -> Sexp = Sexp::NULL; |ctx| {
        duplicates(ctx, x, Sexp::from_id(SEXP_NIL), fromlast, false).map(Sexp::from_id)
    }

    /// Return the first position in the table for each input element.
    fn Rf_match(table: Sexp, x: Sexp, nomatch: c_int) -> Sexp = Sexp::NULL; |ctx| {
        let table = id(ctx, table, "Rf_match")?;
        let x = id(ctx, x, "Rf_match")?;
        let nomatch = ctx.session_mut().scalar_int(nomatch)?;
        ctx.call_function("match", &[(None, x), (None, table), (None, nomatch)], BASE_ENV)
            .map(Sexp::from_id)
    }

    /// Compare two R objects with the selected identity flags.
    fn R_compute_identical(x: Sexp, y: Sexp, flags: c_int) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "R_compute_identical")?;
        let y = id(ctx, y, "R_compute_identical")?;
        let options = IdenticalOpts {
            num_eq: flags & 1 == 0,
            single_na: flags & 2 == 0,
            attrib_as_set: flags & 4 == 0,
            ignore_bytecode: flags & 8 == 0,
            ignore_environment: flags & 16 == 0,
            ignore_srcref: flags & 32 == 0,
        };
        Ok(c_int::from(identical(ctx.session(), x, y, options)?))
    }

    /// Test whether comparable values occur out of ascending order.
    fn Rf_isUnsorted(x: Sexp, strictly: Rboolean) -> Rboolean = 0; |ctx| {
        let x = id(ctx, x, "Rf_isUnsorted")?;
        allocating(ctx, &[x], |ctx| Ok(c_int::from(unsorted(ctx, x, strictly != 0)?)))
    }
}

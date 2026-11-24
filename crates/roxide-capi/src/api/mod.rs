//! Public GNU R 4.6.2 C API entry points grouped by function.

// Wave 1-A: core vector, attribute, coercion, pairlist, string, symbol, and resizable vector APIs.
pub mod attrib;
pub mod coerce;
pub mod pairlist;
pub mod resizable;
pub mod string;
pub mod sym;
pub mod vector;

// Wave 1-B: environment, evaluation, error/condition, printing, unwind, and parse APIs.
pub mod env;
pub mod error;
pub mod eval;
pub mod parse;
pub mod print;
pub mod unwind;

// Wave 1-C: memory/transient alloc, external pointers, weakrefs, Rmath, PRNG, arithmetic, utils, S4.
pub mod arith;
pub mod extptr;
pub mod memory;
pub mod random;
pub mod rmath;
pub mod s4;
pub mod utils;
pub mod weakref;

pub mod gc_alloc;
pub mod iconv;
pub mod linpack;
pub mod optim_integrate;

use roxide_eval::{Ctx, EvalResult, Unwind};
use roxide_value::{SEXP_NIL, Sym, ValueApi, ValueError};

use crate::{abi::Sexp, jump};

unsafe fn api<R: Copy>(fallback: R, mut f: impl FnMut(&mut Ctx) -> EvalResult<R>) -> R {
    // SAFETY: All callers require an active native trampoline.
    unsafe {
        jump::boundary(fallback, move |ctx| {
            ctx.scoped(|ctx| match f(ctx) {
                request @ Err(Unwind::Value(ValueError::ForeignAltrep { .. })) => request,
                Err(Unwind::Value(err)) => Err(ctx.stop(SEXP_NIL, err.to_string())),
                result => result,
            })
        })
    }
}

fn root(ctx: &mut Ctx, value: Sexp) -> EvalResult {
    let id = value
        .to_id()
        .ok_or_else(|| ctx.stop(SEXP_NIL, "invalid SEXP handle"))?;
    if ctx.session().heap.get(id).is_err() {
        return Err(ctx.stop(SEXP_NIL, "stale SEXP handle"));
    }
    ctx.session_mut()
        .heap
        .try_protect(id)
        .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))?;
    Ok(id)
}

fn symbol(ctx: &mut Ctx, value: Sexp) -> EvalResult<Sym> {
    let id = root(ctx, value)?;
    Ok(ctx.session().sexp_as_sym(id)?)
}

fn environment(ctx: &mut Ctx, value: Sexp) -> EvalResult {
    let id = root(ctx, value)?;
    ctx.session().env_enclos(id)?;
    Ok(id)
}

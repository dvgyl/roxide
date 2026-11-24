use std::ffi::c_int;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{EMPTY_ENV, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SYM_DOTS, SexpType, ValueApi};

use super::super::{api, sym::print_name, vector::alloc_vector};
use super::RDotType;
use super::bindings::legacy_lookup;
use super::eval::{BindingValue, binding_api_env, binding_value, promise_expression};
use crate::abi::{Rboolean, Sexp};

fn is_dots(ctx: &Ctx, value: SexpId) -> EvalResult<bool> {
    Ok(value == SEXP_MISSING || ctx.session().type_of(value)? == SexpType::Dot)
}

/// Find the first frame with a dots binding. Return the empty environment if absent.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_findDotsEnv(rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let mut env = binding_api_env(ctx, rho)?;
            for _ in 0..ctx.session().heap.live_objects() {
                if env == EMPTY_ENV || env == SEXP_NIL {
                    return Ok(Sexp::from_id(EMPTY_ENV));
                }
                let dots = legacy_lookup(ctx, SYM_DOTS, env)?;
                if is_dots(ctx, dots)? {
                    return Ok(Sexp::from_id(env));
                }
                env = ctx.session().env_enclos(env)?;
            }
            Err(ctx.stop(SEXP_NIL, "cyclic environment chain"))
        })
    }
}

/// Test for a dots binding in one frame.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_DotsExist(rho: Sexp) -> Rboolean {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(0, |ctx| {
            let env = binding_api_env(ctx, rho)?;
            let dots = legacy_lookup(ctx, SYM_DOTS, env)?;
            Ok(Rboolean::from(is_dots(ctx, dots)?))
        })
    }
}

fn local_dots(ctx: &mut Ctx, rho: Sexp) -> EvalResult {
    let env = binding_api_env(ctx, rho)?;
    let dots = legacy_lookup(ctx, SYM_DOTS, env)?;
    if dots == SEXP_UNBOUND {
        return Err(ctx.stop(
            SEXP_NIL,
            "incorrect context: the current call has no '...' to look in",
        ));
    }
    Ok(if ctx.session().type_of(dots)? == SexpType::Dot {
        dots
    } else {
        SEXP_NIL
    })
}

fn dots_length(ctx: &mut Ctx, dots: SexpId) -> EvalResult<usize> {
    let limit = ctx.session().heap.live_objects();
    let len = ctx
        .session()
        .pairlist_iter(dots)
        .take(limit.saturating_add(1))
        .count();
    if len > limit {
        return Err(ctx.stop(SEXP_NIL, "cyclic ... list"));
    }
    Ok(len)
}

/// Count the dots arguments in one frame.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_DotsLength(rho: Sexp) -> c_int {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(0, |ctx| {
            let dots = local_dots(ctx, rho)?;
            c_int::try_from(dots_length(ctx, dots)?)
                .map_err(|_| ctx.stop(SEXP_NIL, "too many ... arguments"))
        })
    }
}

/// Get the names of the dots arguments in one frame. Return R NULL if none have names.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_DotsNames(rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let mut dots = local_dots(ctx, rho)?;
            let len = dots_length(ctx, dots)?;
            if !ctx
                .session()
                .pairlist_iter(dots)
                .any(|(tag, _)| tag.is_some())
            {
                return Ok(Sexp::from_id(SEXP_NIL));
            }
            ctx.protect(dots);
            ctx.safepoint();
            let names = alloc_vector(ctx, SexpType::Str, len)?;
            for i in 0..len {
                let (_, rest, tag) = ctx.session().cons_parts(dots)?;
                if tag != SEXP_NIL {
                    let name = print_name(ctx, tag)?;
                    ctx.session_mut().str_set(names, i, name)?;
                }
                dots = rest;
            }
            Ok(Sexp::from_id(names))
        })
    }
}

fn dot_value(ctx: &mut Ctx, i: c_int, rho: Sexp) -> EvalResult {
    if i <= 0 {
        return Err(ctx.stop(
            SEXP_NIL,
            format!("indexing '...' with non-positive index {i}"),
        ));
    }
    let env = binding_api_env(ctx, rho)?;
    let dots = legacy_lookup(ctx, SYM_DOTS, env)?;
    if dots == SEXP_UNBOUND {
        return Err(ctx.stop(
            SEXP_NIL,
            format!("..{i} used in an incorrect context, no ... to look in"),
        ));
    }
    if !is_dots(ctx, dots)? {
        return Err(ctx.stop(SEXP_NIL, "bad ... value"));
    }
    let index = (i - 1) as usize;
    let limit = ctx.session().heap.live_objects();
    if index < limit
        && let Some((_, value)) = ctx.session().pairlist_iter(dots).nth(index)
    {
        return Ok(value);
    }
    let elements = if i == 1 { "element" } else { "elements" };
    Err(ctx.stop(
        SEXP_NIL,
        format!("the ... list contains fewer than {i} {elements}"),
    ))
}

/// Get the type of a dots argument without evaluating it.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetDotType(i: c_int, rho: Sexp) -> RDotType {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(RDotType::Value, |ctx| {
            let value = dot_value(ctx, i, rho)?;
            Ok(match binding_value(ctx, value)? {
                BindingValue::Missing => RDotType::Missing,
                BindingValue::Delayed { .. } => RDotType::Delayed,
                BindingValue::Forced { .. } => RDotType::Forced,
                _ => RDotType::Value,
            })
        })
    }
}

/// Get the value of a dots argument. Force its promise if necessary.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_DotsElt(i: c_int, rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let value = dot_value(ctx, i, rho)?;
            if value == SEXP_MISSING {
                return Err(ctx.stop(SEXP_NIL, "argument is missing, with no default"));
            }
            if ctx.session().type_of(value)? == SexpType::Prom {
                return ctx.force(value).map(Sexp::from_id);
            }
            Ok(Sexp::from_id(value))
        })
    }
}

/// Get the expression of a forced dots argument.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_DotForcedExpression(i: c_int, rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let value = dot_value(ctx, i, rho)?;
            match binding_value(ctx, value)? {
                BindingValue::Forced { expr } => promise_expression(ctx, expr),
                _ => Err(ctx.stop(SEXP_NIL, "not a forced ... element")),
            }
        })
    }
}

/// Get the expression of a delayed dots argument.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_DotDelayedExpression(i: c_int, rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let value = dot_value(ctx, i, rho)?;
            match binding_value(ctx, value)? {
                BindingValue::Delayed { expr, .. } => promise_expression(ctx, expr),
                _ => Err(ctx.stop(SEXP_NIL, "not a delayed ... element")),
            }
        })
    }
}

/// Get the evaluation environment of a delayed dots argument.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_DotDelayedEnvironment(i: c_int, rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let value = dot_value(ctx, i, rho)?;
            match binding_value(ctx, value)? {
                BindingValue::Delayed { env, .. } => Ok(Sexp::from_id(env)),
                _ => Err(ctx.stop(SEXP_NIL, "not a delayed ... element")),
            }
        })
    }
}

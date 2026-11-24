use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{PromiseState, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, Sym, ValueApi};

use super::super::{api, root};
use super::RBindingType;
use crate::abi::Sexp;

pub(super) enum BindingValue {
    Unbound,
    Value,
    Missing,
    Delayed { expr: SexpId, env: SexpId },
    Forced { expr: SexpId },
    Active,
}

pub(super) fn binding_value(ctx: &Ctx, value: SexpId) -> EvalResult<BindingValue> {
    Ok(match value {
        SEXP_UNBOUND => BindingValue::Unbound,
        SEXP_MISSING => BindingValue::Missing,
        _ if ctx.session().type_of(value)? == SexpType::Prom => {
            let (expr, env, _, state) = ctx.session().promise_parts(value)?;
            if state == PromiseState::Forced {
                BindingValue::Forced { expr }
            } else {
                BindingValue::Delayed { expr, env }
            }
        }
        _ => BindingValue::Value,
    })
}

fn binding_api_key(ctx: &mut Ctx, sym: Sexp, rho: Sexp) -> EvalResult<(Sym, SexpId)> {
    let sym = root(ctx, sym)?;
    let sym = ctx
        .session()
        .sexp_as_sym(sym)
        .map_err(|_| ctx.stop(SEXP_NIL, "not a symbol"))?;
    Ok((sym, binding_api_env(ctx, rho)?))
}

pub(super) fn binding_api_env(ctx: &mut Ctx, rho: Sexp) -> EvalResult {
    let env = root(ctx, rho)?;
    if ctx.session().type_of(env)? != SexpType::Env {
        return Err(ctx.stop(SEXP_NIL, "not an environment"));
    }
    Ok(env)
}

fn inspect_binding(ctx: &mut Ctx, sym: Sexp, rho: Sexp) -> EvalResult<BindingValue> {
    let (sym, env) = binding_api_key(ctx, sym, rho)?;
    match ctx.session().env_get(env, sym) {
        Some(binding) if binding.active => Ok(BindingValue::Active),
        Some(binding) => binding_value(ctx, binding.value),
        None => Ok(BindingValue::Unbound),
    }
}

pub(super) fn promise_expression(ctx: &mut Ctx, mut expr: SexpId) -> EvalResult<Sexp> {
    let session = ctx.session();
    for _ in 0..session.heap.live_objects() {
        if session.type_of(expr)? != SexpType::Prom {
            return Ok(Sexp::from_id(expr));
        }
        expr = session.promise_parts(expr)?.0;
    }
    Err(ctx.stop(SEXP_NIL, "cyclic promise expression"))
}

/// Get the type of a binding without evaluating it.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetBindingType(sym: Sexp, rho: Sexp) -> RBindingType {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(RBindingType::Unbound, |ctx| {
            Ok(match inspect_binding(ctx, sym, rho)? {
                BindingValue::Unbound => RBindingType::Unbound,
                BindingValue::Value => RBindingType::Value,
                BindingValue::Missing => RBindingType::Missing,
                BindingValue::Delayed { .. } => RBindingType::Delayed,
                BindingValue::Forced { .. } => RBindingType::Forced,
                BindingValue::Active => RBindingType::Active,
            })
        })
    }
}

/// Get the expression of a forced binding.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ForcedBindingExpression(sym: Sexp, rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| match inspect_binding(ctx, sym, rho)? {
            BindingValue::Forced { expr } => promise_expression(ctx, expr),
            _ => Err(ctx.stop(SEXP_NIL, "not a forced binding")),
        })
    }
}

/// Get the expression of a delayed binding.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_DelayedBindingExpression(sym: Sexp, rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| match inspect_binding(ctx, sym, rho)? {
            BindingValue::Delayed { expr, .. } => promise_expression(ctx, expr),
            _ => Err(ctx.stop(SEXP_NIL, "not a delayed binding")),
        })
    }
}

/// Get the evaluation environment of a delayed binding.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_DelayedBindingEnvironment(sym: Sexp, rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| match inspect_binding(ctx, sym, rho)? {
            BindingValue::Delayed { env, .. } => Ok(Sexp::from_id(env)),
            BindingValue::Unbound => Err(ctx.stop(SEXP_NIL, "unbound variable")),
            BindingValue::Active => {
                Err(ctx.stop(SEXP_NIL, "BINDING_OR_SYMBOL_VALUE called on active binding"))
            }
            _ => Err(ctx.stop(SEXP_NIL, "not a delayed binding")),
        })
    }
}

fn assign_promise(ctx: &mut Ctx, sym: Sym, env: SexpId, promise: SexpId) -> EvalResult<()> {
    if let Some(binding) = ctx.session().env_get(env, sym)
        && binding.active
        && !binding.locked
    {
        ctx.protect(binding.value);
        let value = ctx.force(promise)?;
        return ctx
            .apply(binding.value, &[(None, value)], SEXP_NIL, env)
            .map(|_| ());
    }
    ctx.assign(sym, promise, env)
}

/// Bind a symbol to a promise with a cached value.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_MakeForcedBinding(sym: Sexp, expr: Sexp, value: Sexp, rho: Sexp) {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api((), |ctx| {
            let (sym, env) = binding_api_key(ctx, sym, rho)?;
            let expr = root(ctx, expr)?;
            let value = root(ctx, value)?;
            ctx.safepoint();
            let session = ctx.session_mut();
            let promise = session.alloc_promise(expr, SEXP_NIL)?;
            if value != SEXP_UNBOUND {
                session.set_promise_state(promise, PromiseState::Forced, SEXP_NIL, value)?;
            }
            assign_promise(ctx, sym, env, promise)
        })
    }
}

/// Bind a symbol to a promise with an expression and an evaluation environment.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_MakeDelayedBinding(sym: Sexp, expr: Sexp, eval_env: Sexp, rho: Sexp) {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api((), |ctx| {
            let (sym, env) = binding_api_key(ctx, sym, rho)?;
            let expr = root(ctx, expr)?;
            let eval_env = binding_api_env(ctx, eval_env)?;
            ctx.safepoint();
            let promise = ctx.session_mut().alloc_promise(expr, eval_env)?;
            assign_promise(ctx, sym, env, promise)
        })
    }
}

/// Bind a symbol to a missing argument.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_MakeMissingBinding(sym: Sexp, rho: Sexp) {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api((), |ctx| {
            let (sym, env) = binding_api_key(ctx, sym, rho)?;
            ctx.assign(sym, SEXP_MISSING, env)
        })
    }
}

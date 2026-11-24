//! C entry points for evaluation, closures, promises, and options.

#![allow(non_snake_case)]

use std::ffi::c_int;
use std::sync::atomic::{AtomicI32, Ordering};

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{BASE_ENV, GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi};

use crate::abi::Sexp;

use super::env::{api, environment, root, symbol};

/// Visibility of the most recent evaluation through the C API.
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static R_Visible: AtomicI32 = AtomicI32::new(1);

pub(super) fn eval(ctx: &mut Ctx, expression: SexpId, env: SexpId) -> EvalResult {
    let kind = ctx.session().type_of(expression)?;
    if matches!(kind, SexpType::Lang | SexpType::Sym | SexpType::Prom) {
        let rho_type = ctx.session().type_of(env)?;
        if rho_type != SexpType::Env {
            let name = rho_type.type_name(ctx.session().is_s4(env)?);
            return Err(ctx.stop(
                SEXP_NIL,
                format!("'rho' must be an environment not {name}: detected in C-level eval"),
            ));
        }
    }
    let result = ctx.eval(expression, env);
    sync_visibility(ctx);
    result
}

pub(super) fn sync_visibility(ctx: &Ctx) {
    R_Visible.store(i32::from(ctx.is_visible()), Ordering::Relaxed);
}

pub(super) fn current_env(ctx: &Ctx) -> SexpId {
    ctx.current_frame_id()
        .and_then(|id| ctx.frame_info(id))
        .map_or(GLOBAL_ENV, |(_, env, _, _)| env)
}

/// Evaluate an expression in an environment.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_eval(expression: Sexp, rho: Sexp) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            let expression = root(ctx, expression)?;
            let env = root(ctx, rho)?;
            eval(ctx, expression, env).map(Sexp::from_id)
        })
    }
}

/// Find a callable binding in an environment chain.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_findFun(name: Sexp, rho: Sexp) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            let name = symbol(ctx, name)?;
            let env = environment(ctx, rho)?;
            ctx.find_fun(name, env).map(Sexp::from_id)
        })
    }
}

/// Return the current closure frame or the global environment.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetCurrentEnv() -> Sexp {
    unsafe { api(Sexp::NULL, |ctx| Ok(Sexp::from_id(current_env(ctx)))) }
}

/// Allocate a closure with the specified formals, body, and environment.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_mkClosure(formals: Sexp, body: Sexp, rho: Sexp) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            let formals = root(ctx, formals)?;
            let body = root(ctx, body)?;
            let env = environment(ctx, rho)?;
            if !matches!(
                ctx.session().type_of(formals)?,
                SexpType::Nil | SexpType::List
            ) {
                return Err(ctx.stop(SEXP_NIL, "invalid closure formals"));
            }
            ctx.safepoint();
            Ok(Sexp::from_id(
                ctx.session_mut().alloc_closure(formals, body, env)?,
            ))
        })
    }
}

macro_rules! accessor {
    ($name:ident, $parts:ident, $part:tt, $doc:literal) => {
        #[doc = $doc]
        ///
        /// # Safety
        /// Call inside an active native trampoline.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(value: Sexp) -> Sexp {
            unsafe {
                api(Sexp::NULL, |ctx| {
                    let value = root(ctx, value)?;
                    Ok(Sexp::from_id(ctx.session().$parts(value)?.$part))
                })
            }
        }
    };
}

accessor!(
    R_ClosureFormals,
    closure_parts,
    0,
    "Return the closure's formal arguments."
);
accessor!(R_ClosureBody, closure_parts, 1, "Return the closure body.");
accessor!(
    R_ClosureEnv,
    closure_parts,
    2,
    "Return the closure's enclosing environment."
);
accessor!(
    R_ClosureExpr,
    closure_parts,
    1,
    "Return the source expression of the closure body."
);

/// Return an expression that has no bytecode wrapper.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_BytecodeExpr(code: Sexp) -> Sexp {
    // The evaluator currently stores interpreted expressions as closure bodies.
    unsafe { api(Sexp::NULL, |ctx| root(ctx, code).map(Sexp::from_id)) }
}

accessor!(
    R_PromiseExpr,
    promise_parts,
    0,
    "Return the expression stored in a promise."
);
accessor!(
    PRCODE,
    promise_parts,
    0,
    "Return the expression stored in a promise."
);
accessor!(
    PRENV,
    promise_parts,
    1,
    "Return the evaluation environment of a promise."
);
accessor!(
    PRVALUE,
    promise_parts,
    2,
    "Return the stored value or the unbound marker."
);

/// Read an option by symbol.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_GetOption1(tag: Sexp) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            let tag = symbol(ctx, tag)?;
            let visible = ctx.is_visible();
            let name = ctx.session().sym_name(tag)?.to_owned();
            let name = ctx.session_mut().scalar_str(&name)?;
            let result = ctx.call_function("getOption", &[(None, name)], BASE_ENV);
            ctx.set_visible(visible);
            result.map(Sexp::from_id)
        })
    }
}

/// Return the current print width.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_GetOptionWidth() -> c_int {
    unsafe { api(0, |ctx| Ok(ctx.opts().width as c_int)) }
}

/// Evaluate a call and force its first `count` arguments before entering the closure.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_forceAndCall(call: Sexp, count: c_int, rho: Sexp) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            let call = root(ctx, call)?;
            let env = environment(ctx, rho)?;
            if ctx.session().type_of(call)? != SexpType::Lang {
                return Err(ctx.stop(SEXP_NIL, "expected a language call"));
            }
            let force = ctx.session_mut().intern("forceAndCall")?;
            let force = ctx.find_fun(force, BASE_ENV)?;
            ctx.protect(force);
            let session = ctx.session_mut();
            let (head, args, _) = session.cons_parts(call)?;
            let count = session.scalar_int(count)?;
            let args = session.cons(head, args, SEXP_NIL)?;
            let args = session.cons(count, args, SEXP_NIL)?;
            let forced = session.lcons(force, args, SEXP_NIL)?;
            eval(ctx, forced, env).map(Sexp::from_id)
        })
    }
}

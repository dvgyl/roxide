//! C entry points for protected evaluation, cleanup, and condition handlers.

#![allow(non_snake_case)]

use std::cell::Cell;
use std::ffi::{c_int, c_void};

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, CtxExt, Dispatch, EvalResult, Module, PrimDef, PrimKind,
    Unwind, Visibility,
};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{BASE_ENV, CharId, SEXP_MISSING, SEXP_NIL, ValueApi};

use crate::abi::Sexp;
use crate::jump::{BodyFn, HandlerFn, VoidFn, enter_trampoline};

use super::env::{api, root};
use super::eval::{eval, sync_visibility};

#[repr(C)]
struct BodyCall {
    fun: BodyFn,
    data: *mut c_void,
    result: Sexp,
}

#[repr(C)]
struct HandlerCall {
    fun: HandlerFn,
    condition: Sexp,
    data: *mut c_void,
    result: Sexp,
}

unsafe extern "C" {
    fn rox_capi_call_body(data: *mut c_void);
    fn rox_capi_call_handler(data: *mut c_void);
}

fn body(ctx: &mut Ctx, fun: BodyFn, data: *mut c_void) -> EvalResult {
    let mut call = BodyCall {
        fun,
        data,
        result: Sexp::NULL,
    };
    // SAFETY: The C API caller supplies the callback and data. The adapter
    // uses the descriptor only for this call.
    unsafe {
        enter_trampoline(
            ctx,
            rox_capi_call_body,
            std::ptr::from_mut(&mut call).cast(),
        )?
    };
    root(ctx, call.result)
}

fn handler(ctx: &mut Ctx, fun: HandlerFn, cond: SexpId, data: *mut c_void) -> EvalResult {
    ctx.protect(cond);
    let mut call = HandlerCall {
        fun,
        condition: Sexp::from_id(cond),
        data,
        result: Sexp::NULL,
    };
    // SAFETY: The C API caller supplies the callback and data. The adapter
    // uses the descriptor only for this call.
    unsafe {
        enter_trampoline(
            ctx,
            rox_capi_call_handler,
            std::ptr::from_mut(&mut call).cast(),
        )?
    };
    root(ctx, call.result)
}

unsafe fn try_eval(expression: Sexp, rho: Sexp, error: *mut c_int, silent: bool) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            let expression = root(ctx, expression)?;
            let env = root(ctx, rho)?;
            let result = ctx.with_native_toplevel(|ctx| {
                let evaluate = |ctx: &mut Ctx| match eval(ctx, expression, env) {
                    Err(Unwind::Internal(err)) => Err(ctx.stop(SEXP_NIL, err.message)),
                    Err(Unwind::Value(err)) => Err(ctx.stop(SEXP_NIL, err.to_string())),
                    result => result,
                };
                if silent {
                    ctx.with_native_error_handler(evaluate, |_, _| Err(Unwind::TopLevel))
                } else {
                    evaluate(ctx)
                }
            });
            if !error.is_null() {
                error.write(i32::from(result.is_err()));
            }
            Ok(result.map_or(Sexp::NULL, Sexp::from_id))
        })
    }
}

/// Evaluate with an isolated top-level context. Report errors to the console.
///
/// # Safety
/// Call inside an active native trampoline. A non-null `error` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_tryEval(expression: Sexp, rho: Sexp, error: *mut c_int) -> Sexp {
    unsafe { try_eval(expression, rho, error, false) }
}

/// Evaluate with an isolated top-level context and suppress error output.
///
/// # Safety
/// Call inside an active native trampoline. A non-null `error` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_tryEvalSilent(expression: Sexp, rho: Sexp, error: *mut c_int) -> Sexp {
    unsafe { try_eval(expression, rho, error, true) }
}

fn with_cleanup(
    ctx: &mut Ctx,
    clean: Option<VoidFn>,
    data: *mut c_void,
    run: impl FnOnce(&mut Ctx) -> EvalResult,
) -> EvalResult {
    let result = run(ctx);
    if let Some(clean) = clean {
        ctx.protect_eval_result(&result);
        let visible = ctx.is_visible();
        // SAFETY: The C API caller supplies valid callback data.
        let cleanup = unsafe { enter_trampoline(ctx, clean, data) };
        ctx.set_visible(visible);
        sync_visibility(ctx);
        cleanup?;
    }
    result
}

/// Run a callback and run cleanup on both normal return and a non-local transfer.
///
/// # Safety
/// Call inside an active native trampoline. Callbacks and data must be valid
/// for the call. Callbacks must obey the R rules for non-local transfers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ExecWithCleanup(
    fun: Option<BodyFn>,
    data: *mut c_void,
    clean: Option<VoidFn>,
    clean_data: *mut c_void,
) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            let fun = fun.ok_or_else(|| ctx.stop(SEXP_NIL, "invalid NULL callback"))?;
            with_cleanup(ctx, clean, clean_data, |ctx| body(ctx, fun, data)).map(Sexp::from_id)
        })
    }
}

#[derive(Clone, Copy)]
struct Callbacks {
    token: SexpId,
    body: BodyFn,
    data: *mut c_void,
    handler: HandlerFn,
    handler_data: *mut c_void,
}

thread_local! {
    static CALLBACKS: Cell<Option<Callbacks>> = const { Cell::new(None) };
}

struct RestoreCallbacks(Option<Callbacks>);

impl Drop for RestoreCallbacks {
    fn drop(&mut self) {
        CALLBACKS.set(self.0);
    }
}

#[derive(Default)]
struct BridgeRegistered(bool);

impl CtxExt for BridgeRegistered {}

impl Trace for BridgeRegistered {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

const BRIDGE: &str = "roxideNativeCondition";

const MODULE: Module = Module {
    prims: &[PrimDef {
        name: BRIDGE,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Range(1, 2),
        dispatch: Dispatch::None,
        visibility: Visibility::SetByFn,
        fun: invoke_callback,
    }],
    prelude: "",
};

fn invoke_callback(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let Some(callbacks) = CALLBACKS.get().filter(|call| call.token == args.get(0)) else {
        return Err(ctx.stop(call.err_call(), "invalid native condition callback"));
    };
    if args.len() == 1 {
        body(ctx, callbacks.body, callbacks.data)
    } else {
        handler(ctx, callbacks.handler, args.get(1), callbacks.handler_data)
    }
}

fn callback_expr(ctx: &mut Ctx, internal: SexpId, tail: SexpId) -> EvalResult {
    let session = ctx.session_mut();
    let bridge = session.intern(BRIDGE)?.as_sexp();
    let expr = session.lcons(bridge, tail, SEXP_NIL)?;
    let args = session.cons(expr, SEXP_NIL, SEXP_NIL)?;
    Ok(session.lcons(internal, args, SEXP_NIL)?)
}

fn with_handler(
    ctx: &mut Ctx,
    callbacks: Callbacks,
    classes: &[String],
    calling: bool,
) -> EvalResult {
    if !ctx.ext::<BridgeRegistered>().0 {
        if let Err(err) = ctx.register_module(&MODULE) {
            return Err(ctx.stop(SEXP_NIL, err.message));
        }
        ctx.ext::<BridgeRegistered>().0 = true;
    }
    let name = if calling {
        "withCallingHandlers"
    } else {
        "tryCatch"
    };
    let name = ctx.session_mut().intern(name)?;
    let dispatcher = ctx.find_fun(name, BASE_ENV)?;
    ctx.protect(dispatcher);
    let internal = ctx.session_mut().intern(".Internal")?;
    let internal = ctx.find_fun(internal, BASE_ENV)?;
    ctx.protect(internal);

    let session = ctx.session_mut();
    let token = session.alloc_extptr(0, SEXP_NIL, SEXP_NIL)?;
    let body_args = session.cons(token, SEXP_NIL, SEXP_NIL)?;
    let body_expr = callback_expr(ctx, internal, body_args)?;

    let session = ctx.session_mut();
    let cond = session.intern("condition")?.as_sexp();
    let formals = session.cons(SEXP_MISSING, SEXP_NIL, cond)?;
    let cond_arg = session.cons(cond, SEXP_NIL, SEXP_NIL)?;
    let handler_args = session.cons(token, cond_arg, SEXP_NIL)?;
    let handler_expr = callback_expr(ctx, internal, handler_args)?;

    let session = ctx.session_mut();
    let handler = session.alloc_closure(formals, handler_expr, BASE_ENV)?;
    let mut args = SEXP_NIL;
    for class in classes.iter().rev() {
        let tag = session.intern(class)?.as_sexp();
        args = session.cons(handler, args, tag)?;
    }
    let expr_tag = session.intern("expr")?.as_sexp();
    let args = session.cons(body_expr, args, expr_tag)?;
    let expression = session.lcons(dispatcher, args, SEXP_NIL)?;
    let _restore = RestoreCallbacks(CALLBACKS.replace(Some(Callbacks { token, ..callbacks })));
    eval(ctx, expression, BASE_ENV)
}

/// Run a body with an exiting handler for the specified condition classes.
///
/// # Safety
/// Call inside an active native trampoline. Callbacks and data must be valid
/// for the call. Callbacks must obey the R rules for non-local transfers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_tryCatch(
    fun: Option<BodyFn>,
    data: *mut c_void,
    classes: Sexp,
    hndlr: Option<HandlerFn>,
    handler_data: *mut c_void,
    clean: Option<VoidFn>,
    clean_data: *mut c_void,
) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            let classes = root(ctx, classes)?;
            let fun = fun.ok_or_else(|| ctx.stop(SEXP_NIL, "must supply a body function"))?;
            with_cleanup(ctx, clean, clean_data, |ctx| {
                let Some(hndlr) = hndlr.filter(|_| classes != SEXP_NIL) else {
                    return body(ctx, fun, data);
                };
                let mut names = Vec::new();
                for &class in ctx.session().str_slice(classes)? {
                    if class == CharId::NA {
                        return Err(ctx.stop(SEXP_NIL, "invalid condition class"));
                    }
                    let name =
                        String::from_utf8_lossy(ctx.session().char_bytes(class)?).into_owned();
                    if matches!(name.as_str(), "" | "expr") {
                        return Err(ctx.stop(
                            SEXP_NIL,
                            "condition handlers must be specified with a condition class",
                        ));
                    }
                    // R-level tryCatch reserves this argument for cleanup.
                    if name != "finally" {
                        names.push(name);
                    }
                }
                if names.is_empty() {
                    return body(ctx, fun, data);
                }
                with_handler(
                    ctx,
                    Callbacks {
                        token: SEXP_NIL,
                        body: fun,
                        data,
                        handler: hndlr,
                        handler_data,
                    },
                    &names,
                    false,
                )
            })
            .map(Sexp::from_id)
        })
    }
}

/// Run a body with a calling handler for error conditions.
///
/// # Safety
/// Call inside an active native trampoline. Callbacks and data must be valid
/// for the call. Callbacks must obey the R rules for non-local transfers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_withCallingErrorHandler(
    fun: Option<BodyFn>,
    data: *mut c_void,
    hndlr: Option<HandlerFn>,
    handler_data: *mut c_void,
) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            let fun = fun.ok_or_else(|| ctx.stop(SEXP_NIL, "invalid NULL body callback"))?;
            let hndlr = hndlr.ok_or_else(|| ctx.stop(SEXP_NIL, "invalid NULL handler callback"))?;
            with_handler(
                ctx,
                Callbacks {
                    token: SEXP_NIL,
                    body: fun,
                    data,
                    handler: hndlr,
                    handler_data,
                },
                &[String::from("error")],
                true,
            )
            .map(Sexp::from_id)
        })
    }
}

//! Foreign calls, protected objects, and native control transfer.

#![allow(non_snake_case)]

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

use roxide_eval::{Ctx, CtxExt, EvalResult, InternalError, Transfer, Unwind};
use roxide_gc::{ProtectIndex, SexpId, Trace, Tracer};
use roxide_value::{AltNeed, SEXP_NIL, SexpType, ValueApi, ValueError};

use crate::abi::{Rboolean, Sexp};
use crate::api::memory::{release_transient_frame, save_transient_frame};
use crate::cx::{self, ActiveFrame, Mode};
use crate::globals::R_NilValue;

/// Foreign callback with no return value.
pub type VoidFn = unsafe extern "C" fn(*mut c_void);
/// Foreign callback that returns an R object.
pub type BodyFn = unsafe extern "C" fn(*mut c_void) -> Sexp;
/// Foreign cleanup callback that receives a jump flag.
pub type CleanupFn = unsafe extern "C" fn(*mut c_void, Rboolean);
/// Foreign handler that receives an R condition.
pub type HandlerFn = unsafe extern "C" fn(Sexp, *mut c_void) -> Sexp;

/// Kind of native trampoline frame controlling protect-stack handling on normal return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    /// Foreign routine entry point (`.Call`, `.External`, `.C`, `.Fortran`, `R_init_*`).
    /// Restores the protect stack on normal return and warns on imbalance.
    Entry {
        /// Display name reported in stack-imbalance warnings (e.g. `".Call"`).
        label: &'static str,
    },
    /// Nested callback (`R_UnwindProtect` body/cleanup, `R_ToplevelExec`, `R_tryCatchError`).
    /// Inherits the enclosing protect floor and keeps protections made before normal return.
    Callback,
    /// ALTREP method with its own protect floor.
    /// Restores the protect stack without a warning. Its transient allocations
    /// belong to the caller's frame.
    Method,
}

#[repr(C)]
struct BodyCall {
    fun: BodyFn,
    data: *mut c_void,
    result: Sexp,
}

#[repr(C)]
struct CleanupCall {
    fun: CleanupFn,
    data: *mut c_void,
    jumping: Rboolean,
}

#[repr(C)]
struct HandlerCall {
    fun: HandlerFn,
    condition: Sexp,
    data: *mut c_void,
    result: Sexp,
}

#[repr(C)]
struct UnwindProtectCall {
    fun: Option<BodyFn>,
    data: *mut c_void,
    clean: Option<CleanupFn>,
    clean_data: *mut c_void,
    cont: Sexp,
    result: Sexp,
}

unsafe extern "C" {
    fn rox_capi_trampoline(
        fun: VoidFn,
        data: *mut c_void,
        expected_depth: c_int,
        out_code: *mut c_int,
        cpp_err_buf: *mut u8,
        cpp_err_cap: usize,
    ) -> c_int;
    fn rox_capi_longjmp(target_depth: c_int, code: c_int) -> !;
    fn rox_capi_call_body(data: *mut c_void);
    fn rox_capi_call_cleanup(data: *mut c_void);
    fn rox_capi_call_handler(data: *mut c_void);
}

fn internal(message: impl Into<String>) -> Unwind {
    Unwind::Internal(InternalError {
        message: message.into(),
    })
}

fn catch_api_panic<R>(f: impl FnOnce() -> EvalResult<R>) -> EvalResult<R> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|panic| {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown Rust panic");
        let error = internal(format!("internal error in Roxide C API: {message}"));
        // A custom panic payload can panic again when dropped.
        if let Err(secondary) = catch_unwind(AssertUnwindSafe(|| drop(panic))) {
            std::mem::forget(secondary);
        }
        Err(error)
    })
}

/// Resolve a foreign ALTREP length or payload request for a rooted object.
pub type AltrepResolver = fn(&mut Ctx, SexpId, AltNeed) -> EvalResult<()>;

#[derive(Default)]
struct AltrepRetry(Option<AltrepResolver>);

impl Trace for AltrepRetry {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for AltrepRetry {}

/// Set the resolver for foreign ALTREP requests at the C API boundary.
pub fn set_altrep_resolver(ctx: &mut Ctx, resolver: Option<AltrepResolver>) {
    ctx.ext::<AltrepRetry>().0 = resolver;
}

fn altrep_ready(ctx: &Ctx, id: SexpId, need: AltNeed) -> EvalResult<bool> {
    Ok(ctx
        .session()
        .foreign_alt(id)?
        .is_some_and(|info| match need {
            AltNeed::Length => info.len.is_some(),
            AltNeed::Payload => info.owned,
        }))
}

fn resolve_altrep(ctx: &mut Ctx, id: SexpId, need: AltNeed) -> EvalResult<bool> {
    let Some(resolve) = ctx.ext::<AltrepRetry>().0 else {
        return Ok(false);
    };
    if altrep_ready(ctx, id, need)? {
        return Ok(false);
    }
    ctx.scoped(|ctx| {
        ctx.session_mut()
            .heap
            .try_protect(id)
            .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))?;
        resolve(ctx, id, need)?;
        altrep_ready(ctx, id, need)
    })
}

#[inline(never)]
fn run<R: Copy>(mut f: impl FnMut(&mut Ctx) -> EvalResult<R>) -> Result<R, c_int> {
    catch_api_panic(move || {
        cx::with_full(move |ctx| {
            let result = f(ctx);
            let Err(Unwind::Value(ValueError::ForeignAltrep { id, need })) = result else {
                return result;
            };
            let result = match resolve_altrep(ctx, id, need) {
                Ok(true) => f(ctx),
                Ok(false) => Err(ValueError::ForeignAltrep { id, need }.into()),
                Err(err) => Err(err),
            };
            match result {
                Err(Unwind::Value(err)) => Err(ctx.stop(SEXP_NIL, err.to_string())),
                result => result,
            }
        })
    })
    .map_err(cx::pend)
}

/// Run one C API operation. On error, release Rust borrows and jump to the
/// current foreign trampoline.
///
/// The operation can run twice if it requests a foreign ALTREP length or
/// payload. Return that request before any effects that must occur only once.
///
/// # Safety
///
/// Call from foreign code inside an active trampoline. The caller must not
/// leave any Rust or C++ values that need destruction across the jump.
#[inline(always)]
pub unsafe fn boundary<R: Copy>(
    _default_on_jump: R,
    f: impl FnMut(&mut Ctx) -> EvalResult<R>,
) -> R {
    match run(f) {
        Ok(value) => value,
        // SAFETY: run has dropped the closure, borrows and owned values, and
        // verified that an active foreign trampoline sits directly below.
        Err(depth) => unsafe { rox_capi_longjmp(depth, 1) },
    }
}

/// Call a native void callback through a `.Call` jump target and C++ exception handler.
///
/// # Safety
///
/// The callback and data must be valid for the call. The callback must obey
/// the R API rules for non-local jumps and must not jump into Rust.
pub unsafe fn enter_trampoline(ctx: &mut Ctx, fun: VoidFn, data: *mut c_void) -> EvalResult<()> {
    // SAFETY: Caller upholds callback and data validity.
    unsafe { enter_trampoline_with(ctx, FrameKind::Entry { label: ".Call" }, fun, data) }
}

/// Call a native void callback through a jump target and C++ exception handler with
/// the protect-stack and allocation rules for the specified frame kind.
///
/// # Safety
///
/// The callback and data must be valid for the call. The callback must obey
/// the R API rules for non-local jumps and must not jump into Rust.
pub unsafe fn enter_trampoline_with(
    ctx: &mut Ctx,
    kind: FrameKind,
    fun: VoidFn,
    data: *mut c_void,
) -> EvalResult<()> {
    let expected_depth = cx::depth() as c_int;
    let entry = matches!(kind, FrameKind::Entry { .. });
    let inherit_floor = matches!(kind, FrameKind::Callback);
    let Some(frame) = ActiveFrame::push(&ctx.session().heap, inherit_floor) else {
        return Err(ctx.signal_error_at(SEXP_NIL, "native trampoline depth limit exceeded"));
    };
    let depth = cx::depth();
    if entry {
        save_transient_frame(ctx);
    }
    let mut jump_code = 0;
    let mut exception = [0u8; 512];
    let status = cx::with_released(ctx, Mode::Full, || {
        // SAFETY: The caller supplies the callback and data. The output
        // buffers are live until the C++ trampoline returns.
        unsafe {
            rox_capi_trampoline(
                fun,
                data,
                expected_depth,
                &mut jump_code,
                exception.as_mut_ptr(),
                exception.len(),
            )
        }
    });
    let (mark, before) = frame.saved_mark();
    let pending = frame.take_unwind();
    drop(frame);
    ctx.ext::<UnwindConts>().drop_owner(depth);
    if entry {
        release_transient_frame(ctx);
    }

    match status {
        0 => {
            match kind {
                FrameKind::Entry { label } => {
                    let heap = &mut ctx.session_mut().heap;
                    let after = heap.protect_depth();
                    heap.unprotect_to(mark);
                    if before != after {
                        let _ = ctx.host().write_err(
                            format!(
                                "Warning: stack imbalance in '{label}', {before} then {after}\n"
                            )
                            .as_bytes(),
                        );
                    }
                }
                FrameKind::Callback => {}
                FrameKind::Method => ctx.session_mut().heap.unprotect_to(mark),
            }
            Ok(())
        }
        1 if jump_code == 1 => {
            ctx.session_mut().heap.unprotect_to(mark);
            Err(pending.unwrap_or_else(|| internal("native jump without a pending unwind")))
        }
        2 => {
            ctx.session_mut().heap.unprotect_to(mark);
            let bytes = exception.split(|&byte| byte == 0).next().unwrap_or(&[]);
            Err(ctx.signal_error_at(
                SEXP_NIL,
                &format!(
                    "C++ exception escaped native routine: {}",
                    String::from_utf8_lossy(bytes)
                ),
            ))
        }
        _ => {
            ctx.session_mut().heap.unprotect_to(mark);
            Err(internal("invalid native trampoline status"))
        }
    }
}

pub(crate) fn call_body(ctx: &mut Ctx, fun: BodyFn, data: *mut c_void) -> EvalResult {
    let mut call = BodyCall {
        fun,
        data,
        result: Sexp::NULL,
    };
    // SAFETY: The C caller supplies the callback and data. The C++ adapter
    // uses this descriptor only while the trampoline is active.
    unsafe {
        enter_trampoline_with(
            ctx,
            FrameKind::Callback,
            rox_capi_call_body,
            std::ptr::from_mut(&mut call).cast(),
        )?
    };
    id(ctx, call.result, "native callback")
}

pub(crate) fn call_handler(
    ctx: &mut Ctx,
    fun: HandlerFn,
    condition: SexpId,
    data: *mut c_void,
) -> EvalResult {
    let mut call = HandlerCall {
        fun,
        condition: Sexp::from_id(condition),
        data,
        result: Sexp::NULL,
    };
    // SAFETY: The C caller supplies the callback and data. The adapter uses
    // the descriptor only during this call.
    unsafe {
        enter_trampoline_with(
            ctx,
            FrameKind::Callback,
            rox_capi_call_handler,
            std::ptr::from_mut(&mut call).cast(),
        )?
    };
    id(ctx, call.result, "native condition handler")
}

fn id(ctx: &mut Ctx, value: Sexp, name: &str) -> EvalResult {
    let Some(id) = value.to_id() else {
        let reason = if value == Sexp::NULL {
            "NULL"
        } else {
            "malformed"
        };
        return Err(ctx.stop(SEXP_NIL, format!("invalid {reason} SEXP passed to {name}")));
    };
    if ctx.session().heap.get(id).is_err() {
        return Err(ctx.stop(SEXP_NIL, format!("stale SEXP passed to {name}")));
    }
    Ok(id)
}

fn protect(ctx: &mut Ctx, value: Sexp, name: &str) -> EvalResult<ProtectIndex> {
    let value = id(ctx, value, name)?;
    ctx.session_mut()
        .heap
        .try_protect(value)
        .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))
}

/// Push an object onto the protect stack.
///
/// # Safety
///
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_protect(value: Sexp) -> Sexp {
    // SAFETY: The foreign caller must provide a trampoline.
    unsafe {
        boundary(Sexp::NULL, |ctx| {
            protect(ctx, value, "Rf_protect").map(|_| value)
        })
    }
}

/// Pop objects protected in this native frame.
///
/// # Safety
///
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_unprotect(count: c_int) {
    // SAFETY: The foreign caller must provide a trampoline.
    unsafe {
        boundary((), |ctx| {
            let depth = ctx.session().heap.protect_depth();
            let available = depth.saturating_sub(cx::protect_floor());
            let n =
                match usize::try_from(count) {
                    Ok(n) if n <= available => n,
                    _ => return Err(ctx.stop(
                        SEXP_NIL,
                        format!(
                            "unprotect stack imbalance: tried to pop {count} with depth {available}"
                        ),
                    )),
                };
            ctx.session_mut()
                .heap
                .unprotect(n)
                .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))
        });
    }
}

/// Remove the topmost matching object protected in this native frame.
///
/// # Safety
///
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_unprotect_ptr(value: Sexp) {
    // SAFETY: The foreign caller must provide a trampoline.
    unsafe {
        boundary((), |ctx| {
            let value = id(ctx, value, "Rf_unprotect_ptr")?;
            ctx.session_mut()
                .heap
                .unprotect_ptr_from(value, cx::protect_floor())
                .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))
        });
    }
}

unsafe fn protect_with_index(ctx: &mut Ctx, value: Sexp, index: *mut c_int) -> EvalResult<()> {
    if index.is_null() {
        return Err(ctx.stop(SEXP_NIL, "invalid NULL index passed to R_ProtectWithIndex"));
    }
    let raw = protect(ctx, value, "R_ProtectWithIndex")?.as_raw();
    let raw = c_int::try_from(raw)
        .map_err(|_| ctx.stop(SEXP_NIL, "protect stack index exceeds C int range"))?;
    // SAFETY: The C caller guarantees writable storage for a non-null index.
    unsafe { index.write(raw) };
    Ok(())
}

/// Protect an object and write its stack index to `index`.
///
/// # Safety
///
/// Call inside an active trampoline. A non-null `index` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ProtectWithIndex(value: Sexp, index: *mut c_int) {
    // SAFETY: The caller supplies the trampoline and writable output.
    unsafe { boundary((), |ctx| protect_with_index(ctx, value, index)) }
}

/// Alias for [`R_ProtectWithIndex`].
///
/// # Safety
///
/// Call inside an active trampoline. A non-null `index` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_protect_with_index(value: Sexp, index: *mut c_int) {
    // SAFETY: The caller supplies the trampoline and writable output.
    unsafe { boundary((), |ctx| protect_with_index(ctx, value, index)) }
}

/// Replace the object at a protect-stack index.
///
/// # Safety
///
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Reprotect(value: Sexp, index: c_int) {
    // SAFETY: The foreign caller must provide a trampoline.
    unsafe {
        boundary((), |ctx| {
            let value = id(ctx, value, "R_Reprotect")?;
            let heap = &mut ctx.session_mut().heap;
            let allowed = cx::protect_floor()..heap.protect_depth();
            let index = match usize::try_from(index) {
                Ok(index) if allowed.contains(&index) => index,
                _ => return Err(ctx.stop(SEXP_NIL, "R_Reprotect: invalid protect index")),
            };
            heap.reprotect(ProtectIndex::from_raw(index), value);
            Ok(())
        });
    }
}

struct UnwindCont {
    owner: usize,
    pending: Option<Unwind>,
}

#[derive(Default)]
struct UnwindConts {
    tokens: HashMap<SexpId, Vec<UnwindCont>>,
    /// Return values pinned across normal-return `R_UnwindProtect` cleanup callbacks
    /// without perturbing the C protect stack.
    pinned: Vec<SexpId>,
}

impl UnwindConts {
    fn drop_owner(&mut self, owner: usize) {
        self.tokens.retain(|_, stack| {
            stack.retain(|cont| cont.owner != owner);
            !stack.is_empty()
        });
    }
}

impl CtxExt for UnwindConts {}

impl Trace for UnwindConts {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for (&token, stack) in &self.tokens {
            tracer.edge(token);
            for cont in stack {
                if let Some(Unwind::Transfer { kind, .. }) = &cont.pending {
                    match *kind {
                        Transfer::ExitingHandler { cond, .. } | Transfer::Return(cond) => {
                            tracer.edge(cond);
                        }
                        Transfer::Restart { restart, args } => {
                            tracer.edge(restart);
                            tracer.edge(args);
                        }
                        Transfer::Break | Transfer::Next => {}
                    }
                }
            }
        }
        tracer.edges(&self.pinned);
    }
}

fn make_cont(ctx: &mut Ctx) -> EvalResult {
    ctx.safepoint();
    Ok(ctx.session_mut().cons(SEXP_NIL, SEXP_NIL, SEXP_NIL)?)
}

/// Allocate a `LISTSXP` continuation token for `R_UnwindProtect` (`CONS(R_NilValue, R_NilValue)`).
///
/// # Safety
///
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_MakeUnwindCont() -> Sexp {
    // SAFETY: The foreign caller must provide a trampoline.
    unsafe { boundary(Sexp::NULL, |ctx| make_cont(ctx).map(Sexp::from_id)) }
}

/// Resume the unwind stored in a continuation token.
///
/// # Safety
///
/// Call inside the native trampoline that owns this continuation. The jump
/// must not cross any Rust or C++ values that still need destruction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ContinueUnwind(cont: Sexp) -> ! {
    // SAFETY: The foreign caller must provide a trampoline.
    unsafe {
        boundary((), |ctx| {
            let table = &mut ctx.ext::<UnwindConts>().tokens;
            if let Some(token) = cont.to_id()
                && let Some(stack) = table.get_mut(&token)
                && stack.last().is_some_and(|top| top.owner == cx::depth())
                && let Some(unwind) = stack.last_mut().and_then(|top| top.pending.take())
            {
                stack.pop();
                if stack.is_empty() {
                    table.remove(&token);
                }
                return Err(unwind);
            }
            Err(ctx.stop(
                SEXP_NIL,
                "R_ContinueUnwind: invalid or inactive continuation",
            ))
        });
    }
    std::process::abort()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rox_capi_unwind_protect(call: *mut UnwindProtectCall) -> c_int {
    // SAFETY: The C++ wrapper supplies a live call descriptor and trampoline.
    unsafe {
        boundary(0, |ctx| {
            let call = &mut *call;
            let fun = checked_callback(ctx, call.fun, "R_UnwindProtect")?;
            let token = if call.cont == Sexp::NULL {
                make_cont(ctx)?
            } else {
                let token = id(ctx, call.cont, "R_UnwindProtect")?;
                if ctx.session().type_of(token)? != SexpType::List {
                    return Err(ctx.stop(SEXP_NIL, "R_UnwindProtect: invalid continuation"));
                }
                token
            };
            ctx.ext::<UnwindConts>()
                .tokens
                .entry(token)
                .or_default()
                .push(UnwindCont {
                    owner: cx::depth(),
                    pending: None,
                });
            let result = call_body(ctx, fun, call.data);
            match result {
                Ok(value) => {
                    let tokens = &mut ctx.ext::<UnwindConts>().tokens;
                    if let Some(stack) = tokens.get_mut(&token) {
                        stack.pop();
                        if stack.is_empty() {
                            tokens.remove(&token);
                        }
                    }
                    call.result = Sexp::from_id(value);
                    if let Some(clean) = call.clean {
                        let mut cleanup = CleanupCall {
                            fun: clean,
                            data: call.clean_data,
                            jumping: 0,
                        };
                        ctx.ext::<UnwindConts>().pinned.push(value);
                        let clean_res = enter_trampoline_with(
                            ctx,
                            FrameKind::Callback,
                            rox_capi_call_cleanup,
                            std::ptr::from_mut(&mut cleanup).cast(),
                        );
                        ctx.ext::<UnwindConts>().pinned.pop();
                        clean_res?;
                    }
                    Ok(0)
                }
                Err(unwind) => {
                    let Some(entry) = ctx
                        .ext::<UnwindConts>()
                        .tokens
                        .get_mut(&token)
                        .and_then(|stack| stack.last_mut())
                    else {
                        return Err(internal("native unwind continuation disappeared"));
                    };
                    entry.pending = Some(unwind);
                    call.cont = Sexp::from_id(token);
                    Ok(1)
                }
            }
        })
    }
}

fn checked_callback<T>(ctx: &mut Ctx, fun: Option<T>, name: &str) -> EvalResult<T> {
    fun.ok_or_else(|| ctx.stop(SEXP_NIL, format!("invalid NULL callback passed to {name}")))
}

/// Run a native callback with an isolated top-level condition context.
///
/// # Safety
///
/// Call inside an active trampoline. The callback and data must be valid
/// for the call and must follow the R API rules for jumps.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ToplevelExec(fun: Option<VoidFn>, data: *mut c_void) -> Rboolean {
    // SAFETY: The caller supplies the trampoline and callback.
    unsafe {
        boundary(0, |ctx| {
            let fun = checked_callback(ctx, fun, "R_ToplevelExec")?;
            ctx.with_native_toplevel(|ctx| {
                let result =
                    catch_api_panic(|| enter_trampoline_with(ctx, FrameKind::Callback, fun, data));
                match result {
                    Ok(()) => Ok(1),
                    Err(quit @ Unwind::Quit { .. }) => Err(quit),
                    Err(Unwind::Internal(err)) => {
                        let _ = ctx.signal_error_at(SEXP_NIL, &err.message);
                        Ok(0)
                    }
                    Err(Unwind::Value(err)) => {
                        let _ = ctx.signal_error_at(SEXP_NIL, &err.to_string());
                        Ok(0)
                    }
                    Err(_) => Ok(0),
                }
            })
        })
    }
}

/// Run a native body with an exiting R error handler.
///
/// # Safety
///
/// Call inside an active trampoline. Both callbacks and their data must be
/// valid for the call and must follow the R API rules for jumps.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_tryCatchError(
    fun: Option<BodyFn>,
    data: *mut c_void,
    handler: Option<HandlerFn>,
    handler_data: *mut c_void,
) -> Sexp {
    // SAFETY: The caller supplies the trampoline and callbacks.
    unsafe {
        boundary(Sexp::NULL, |ctx| {
            let fun = checked_callback(ctx, fun, "R_tryCatchError")?;
            let handler = checked_callback(ctx, handler, "R_tryCatchError")?;
            ctx.with_native_error_handler(
                |ctx| catch_api_panic(|| call_body(ctx, fun, data)),
                |ctx, cond| call_handler(ctx, handler, cond, handler_data),
            )
            .map(Sexp::from_id)
        })
    }
}

fn warning_length_max_bytes(ctx: &mut Ctx) -> usize {
    let raw = ctx
        .option_value("warning.length")
        .ok()
        .flatten()
        .and_then(|opt| match ctx.session().type_of(opt).ok()? {
            SexpType::Int => ctx.session().int(opt).ok()?.iter().next().map(i64::from),
            SexpType::Real => ctx
                .session()
                .real(opt)
                .ok()?
                .iter()
                .next()
                .filter(|v| v.is_finite())
                .map(|v| v as i64),
            _ => None,
        })
        .unwrap_or(1000);
    (raw.clamp(100, 8170) as usize).saturating_sub(1)
}

fn truncate_utf8_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

// The C shim releases its va_list and buffer before it jumps.
unsafe fn formatted_condition(
    call: Sexp,
    ptr: *const c_char,
    len: usize,
    name: &str,
    signal: fn(&mut Ctx, SexpId, &str) -> EvalResult<()>,
) -> c_int {
    run(|ctx| {
        let id = id(ctx, call, name)?;
        // SAFETY: The shim keeps these bytes readable until this function returns.
        let bytes = unsafe { std::slice::from_raw_parts(ptr.cast(), len) };
        let owned = String::from_utf8_lossy(bytes);
        let max_bytes = warning_length_max_bytes(ctx);
        let truncated = truncate_utf8_bytes(&owned, max_bytes);
        signal(ctx, id, truncated)
    })
    .err()
    .unwrap_or(-1)
}

fn signal_error(ctx: &mut Ctx, call: SexpId, text: &str) -> EvalResult<()> {
    Err(ctx.signal_error_at(call, text))
}

fn signal_warning(ctx: &mut Ctx, call: SexpId, text: &str) -> EvalResult<()> {
    ctx.warning_at(call, text)
}

fn signal_warning_immediate(ctx: &mut Ctx, call: SexpId, text: &str) -> EvalResult<()> {
    let saved = ctx.opts().warn;
    if saved == 0 {
        ctx.opts_mut().warn = 1;
    }
    let result = ctx.warning_at(call, text);
    ctx.opts_mut().warn = saved;
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rox_capi_error_str(ptr: *const c_char, len: usize) -> c_int {
    // SAFETY: The C shim supplies a live buffer.
    unsafe { formatted_condition(R_NilValue, ptr, len, "Rf_error", signal_error) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rox_capi_errorcall_str(call: Sexp, ptr: *const c_char, len: usize) -> c_int {
    // SAFETY: The C shim supplies a live buffer.
    unsafe { formatted_condition(call, ptr, len, "Rf_errorcall", signal_error) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rox_capi_warning_str(ptr: *const c_char, len: usize) -> c_int {
    // SAFETY: The C shim supplies a live buffer.
    unsafe { formatted_condition(R_NilValue, ptr, len, "Rf_warning", signal_warning) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rox_capi_warningcall_str(call: Sexp, ptr: *const c_char, len: usize) -> c_int {
    // SAFETY: The C shim supplies a live buffer.
    unsafe { formatted_condition(call, ptr, len, "Rf_warningcall", signal_warning) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rox_capi_warningcall_immediate_str(
    call: Sexp,
    ptr: *const c_char,
    len: usize,
) -> c_int {
    // SAFETY: The C shim supplies a live buffer.
    unsafe {
        formatted_condition(
            call,
            ptr,
            len,
            "Rf_warningcall_immediate",
            signal_warning_immediate,
        )
    }
}

macro_rules! print_bridge {
    ($name:ident, $write:ident) => {
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $name(ptr: *const c_char, len: usize) -> c_int {
            run(|ctx| {
                // SAFETY: The C shim keeps `len` bytes readable during this call.
                let bytes = unsafe { std::slice::from_raw_parts(ptr.cast(), len) };
                ctx.host()
                    .$write(bytes)
                    .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))
            })
            .err()
            .unwrap_or(-1)
        }
    };
}

print_bridge!(rox_capi_rprintf_str, write_out);
print_bridge!(rox_capi_reprintf_str, write_err);

//! Non-variadic condition and diagnostic entry points.

#![allow(non_snake_case)]

use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char};
use std::ptr::{copy_nonoverlapping, null};

use roxide_eval::Ctx;
use roxide_value::SEXP_NIL;

use super::api;

const ERROR_BUFFER_SIZE: usize = 8192;

thread_local! {
    static ERROR_BUFFER: UnsafeCell<[u8; ERROR_BUFFER_SIZE]> = const { UnsafeCell::new([0; ERROR_BUFFER_SIZE]) };
}

/// Return a C string with the latest evaluator error message.
///
/// # Safety
/// Call inside an active native trampoline. Use the pointer only on this thread.
/// Each call replaces the contents.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_curErrorBuf() -> *const c_char {
    let read = |ctx: &mut Ctx| {
        let bytes = ctx.last_error_message().as_bytes();
        let len = bytes
            .iter()
            .take(ERROR_BUFFER_SIZE - 1)
            .position(|&byte| byte == 0)
            .unwrap_or(bytes.len().min(ERROR_BUFFER_SIZE - 1));
        Ok(ERROR_BUFFER.with(|buffer| {
            let dest = buffer.get().cast::<u8>();
            // SAFETY: The thread-local buffer is disjoint from bytes.
            // The copy and terminator fit in the buffer.
            unsafe {
                copy_nonoverlapping(bytes.as_ptr(), dest, len);
                dest.add(len).write(0);
            }
            dest.cast_const().cast()
        }))
    };
    // SAFETY: The caller provides an active native trampoline.
    unsafe { api(null(), read) }
}

/// Write a message to the error stream.
///
/// # Safety
/// Call inside an active native trampoline. `message` must point to a C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ShowMessage(message: *const c_char) {
    // SAFETY: The caller provides a trampoline and a C string.
    unsafe {
        api((), |ctx| {
            if message.is_null() {
                return Err(ctx.stop(SEXP_NIL, "invalid NULL message"));
            }
            let bytes = CStr::from_ptr(message).to_bytes();
            let result = ctx
                .host()
                .write_err(bytes)
                .and_then(|()| ctx.host().write_err(b"\n"));
            result.map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))
        })
    }
}

/// Signal an interrupt condition with a resume restart.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_onintr() {
    // SAFETY: The caller provides an active native trampoline.
    unsafe { api((), |ctx| ctx.signal_native_interrupt(true)) }
}

/// Signal an interrupt and leave the current computation without resuming it.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_onintrNoResume() {
    // SAFETY: The caller provides an active native trampoline.
    unsafe { api((), |ctx| ctx.signal_native_interrupt(false)) }
}

/// Signal an interrupt condition if the host has a pending user interrupt.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_CheckUserInterrupt() {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api((), |ctx| {
            if ctx.host().poll_interrupt() {
                ctx.signal_native_interrupt(true)?;
            }
            Ok(())
        })
    }
}

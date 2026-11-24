//! Non-variadic condition and diagnostic entry points.

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::ffi::{CStr, c_char};
use std::ptr::null;

use roxide_eval::Unwind;
use roxide_value::{BASE_ENV, Encoding, SEXP_NIL, SYM_CLASS, SYM_NAMES, ValueApi};

use super::env::api;

thread_local! {
    static ERROR_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Return a C string with the latest evaluator error message.
///
/// # Safety
/// Call inside an active native trampoline. The pointer is valid on this thread
/// until the next call to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_curErrorBuf() -> *const c_char {
    unsafe {
        api(null(), |ctx| {
            let visible = ctx.is_visible();
            let value = ctx.call_function("geterrmessage", &[], BASE_ENV);
            ctx.set_visible(visible);
            let value = value?;
            let session = ctx.session();
            let chars = session.str_slice(value)?;
            let bytes = match chars.first() {
                Some(&text) => session.char_bytes(text)?,
                None => &[],
            };
            Ok(ERROR_BUFFER.with_borrow_mut(|buffer| {
                buffer.clear();
                buffer.extend(bytes.iter().copied().take_while(|&byte| byte != 0));
                buffer.push(0);
                buffer.as_ptr().cast()
            }))
        })
    }
}

/// Write a message to the error stream.
///
/// # Safety
/// Call inside an active native trampoline. `message` must point to a C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ShowMessage(message: *const c_char) {
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

/// Signal an interrupt condition and leave the current computation.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_onintr() {
    unsafe {
        api((), |ctx| {
            let session = ctx.session_mut();
            let message = session.scalar_str("")?;
            let condition = session.alloc_list(2)?;
            session.list_set(condition, 0, message)?;
            let message_name = session.mk_char(b"message", Encoding::Native)?;
            let call_name = session.mk_char(b"call", Encoding::Native)?;
            let names = session.alloc_str_from(&[message_name, call_name])?;
            session.set_attrib(condition, SYM_NAMES, names)?;
            let interrupt = session.mk_char(b"interrupt", Encoding::Native)?;
            let condition_name = session.mk_char(b"condition", Encoding::Native)?;
            let classes = session.alloc_str_from(&[interrupt, condition_name])?;
            session.set_attrib(condition, SYM_CLASS, classes)?;
            ctx.call_function("signalCondition", &[(None, condition)], BASE_ENV)?;
            Err(Unwind::TopLevel)
        })
    }
}

/// Signal an interrupt and leave the current computation without resuming it.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_onintrNoResume() {
    unsafe { Rf_onintr() }
}

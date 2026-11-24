//! C entry points for value printing and console operations.

#![allow(non_snake_case)]

use roxide_host::{HostErrorKind, StdStream};
use roxide_value::{BASE_ENV, SEXP_NIL, ValueApi};

use crate::abi::Sexp;

use super::env::{api, root};
use super::eval::current_env;

/// Print an R value to the console.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_PrintValue(value: Sexp) {
    unsafe {
        api((), |ctx| {
            let value = root(ctx, value)?;
            let env = current_env(ctx);
            let visible = ctx.is_visible();
            let result = (|| {
                if ctx.session().is_s4(value)? && ctx.show_s4(value, env)? {
                    return Ok(());
                }
                let print = ctx.session_mut().intern("print")?;
                let print = ctx.find_fun(print, BASE_ENV)?;
                ctx.apply(print, &[(None, value)], SEXP_NIL, env)?;
                Ok(())
            })();
            ctx.set_visible(visible);
            result
        })
    }
}

/// Flush standard output.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_FlushConsole() {
    unsafe {
        api((), |ctx| {
            ctx.flush_stream_sinks()?;
            ctx.host()
                .flush(StdStream::Stdout)
                .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))
        })
    }
}

/// Process a pending host interrupt.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ProcessEvents() {
    unsafe { api((), |ctx| ctx.check_user_interrupt()) }
}

/// Remove the session temporary directory and its contents.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_CleanTempDir() {
    unsafe {
        api((), |ctx| {
            let path = ctx.host().session_temp_dir().to_owned();
            match ctx.host().remove(&path, true) {
                Ok(()) => Ok(()),
                Err(err) if err.kind == HostErrorKind::NotFound => Ok(()),
                Err(err) => Err(ctx.stop(SEXP_NIL, err.to_string())),
            }
        })
    }
}

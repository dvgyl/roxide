//! Vector allocation, element access, data pointers, and type predicates.

mod access;
mod alloc;
mod wrap;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SexpType};

use crate::abi::Sexp;

pub use super::altrep::{ALTREP, R_tryWrap};
pub use access::*;
pub use alloc::*;
pub use wrap::*;

macro_rules! api {
    ($(#[doc = $doc:expr] fn $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty =
        $default:expr; |$ctx:ident| $body:block)*) => {$(
        #[doc = $doc]
        ///
        /// # Safety
        /// Call inside an active native trampoline. Supply valid storage for
        /// all C buffers and follow the R protection rules for returned values.
        #[allow(non_snake_case)]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name($($arg: $ty),*) -> $ret {
            // SAFETY: The caller supplies the trampoline and C buffers.
            unsafe {
                $crate::jump::boundary($default, |$ctx| {
                    $crate::api::vector::signal_value_error($ctx, |$ctx| $body)
                })
            }
        }
    )*};
}
pub(crate) use api;

pub(crate) fn signal_value_error<T>(
    ctx: &mut Ctx,
    f: impl FnOnce(&mut Ctx) -> EvalResult<T>,
) -> EvalResult<T> {
    match f(ctx) {
        request @ Err(roxide_eval::Unwind::Value(roxide_value::ValueError::ForeignAltrep {
            ..
        })) => request,
        Err(roxide_eval::Unwind::Value(err)) => {
            Err(ctx.signal_error_at(SEXP_NIL, &err.to_string()))
        }
        result => result,
    }
}

pub(crate) fn id(ctx: &mut Ctx, x: Sexp, name: &str) -> EvalResult<SexpId> {
    let Some(value) = x.to_id() else {
        let reason = if x == Sexp::NULL { "NULL" } else { "malformed" };
        return Err(ctx.stop(SEXP_NIL, format!("invalid {reason} SEXP passed to {name}")));
    };
    if ctx.session().heap.get(value).is_err() {
        return Err(ctx.stop(SEXP_NIL, format!("stale SEXP passed to {name}")));
    }
    Ok(value)
}

pub(crate) fn allocating<T>(
    ctx: &mut Ctx,
    inputs: &[SexpId],
    f: impl FnOnce(&mut Ctx) -> EvalResult<T>,
) -> EvalResult<T> {
    ctx.scoped(|ctx| {
        for &x in inputs {
            ctx.session_mut()
                .heap
                .try_protect(x)
                .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))?;
        }
        ctx.safepoint();
        f(ctx)
    })
}

pub(crate) fn is_atomic(ty: SexpType) -> bool {
    use SexpType::*;
    matches!(ty, Lgl | Int | Real | Cplx | Str | Raw)
}

fn is_vector_list(ty: SexpType) -> bool {
    matches!(ty, SexpType::Vec | SexpType::Expr)
}

pub(crate) fn is_vector(ty: SexpType) -> bool {
    is_atomic(ty) || is_vector_list(ty)
}

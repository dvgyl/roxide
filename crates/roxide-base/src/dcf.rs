//! Read and write Debian Control File records.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod read;
mod write;

use roxide_eval::{
    ArgEval, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{CharId, RSession, SexpType, ValueApi};

/// Register the base DCF functions.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_rox_read_dcf", 4, read::read_dcf, Visibility::On),
        internal("C_rox_write_dcf", 7, write::write_dcf, Visibility::Off),
    ],
    prelude: r#"
read.dcf <- function(file, fields = NULL, all = FALSE, keep.white = NULL)
    .Internal(C_rox_read_dcf(file, fields, all, keep.white))
write.dcf <- function(x, file = "", append = FALSE, useBytes = FALSE,
                      indent = 0.1 * getOption("width"), width = 0.9 * getOption("width"),
                      keep.white = NULL)
    .Internal(C_rox_write_dcf(x, file, append, useBytes, indent, width, keep.white))
"#,
};

const fn internal(
    name: &'static str,
    count: usize,
    fun: PrimFn,
    visibility: Visibility,
) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(count),
        dispatch: Dispatch::None,
        visibility,
        fun,
    }
}

fn flag(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<bool> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
    let values = ctx.session().lgl(value)?;
    if let Some(flag) = values.iter().next().and_then(|v| v.get()) {
        Ok(flag)
    } else {
        Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"))
    }
}

fn set(s: &mut RSession, object: SexpId, name: &str, value: SexpId) -> EvalResult<()> {
    let key = s.intern(name)?;
    Ok(s.set_attrib(object, key, value)?)
}

fn attr(s: &mut RSession, object: SexpId, name: &str) -> EvalResult {
    let key = s.intern(name)?;
    Ok(s.attr(object, key))
}

fn keys(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<Option<Vec<u8>>>> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Str)?;
    let limit = ctx.host().limits().max_vector_bytes;
    let s = ctx.session();
    let ids = s.str_slice(value)?;
    let mut used = ids.len().saturating_mul(size_of::<Option<Vec<u8>>>());
    let mut out = Vec::new();
    for &id in ids {
        let bytes = if id == CharId::NA {
            None
        } else {
            Some(s.char_bytes(id)?)
        };
        used = used.saturating_add(bytes.map_or(0, <[u8]>::len));
        if used > limit {
            return Err(ctx.stop(roxide_value::SEXP_NIL, "vector size limit exceeded"));
        }
        out.push(bytes.map(<[u8]>::to_vec));
    }
    Ok(out)
}

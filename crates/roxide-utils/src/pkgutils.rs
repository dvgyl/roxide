//! Package metadata, S3 lookup, and utility functions used by package code.

use std::fmt::Write as _;

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind,
    Visibility,
};
use roxide_value::{CharId, Encoding, SEXP_NIL, ValueApi};

/// Register package utilities in the utils namespace.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_rox_get_s3_method", 4, get_s3_method),
        internal("C_rox_url_encode", 3, url_encode),
    ],
    prelude: include_str!("pkgutils.R"),
};

pub(crate) const PACKAGE_TOOLS: Module = Module {
    prims: &[],
    prelude: include_str!("package_tools.R"),
};

pub(crate) const NAMESPACE_TOOLS: Module = Module {
    prims: &[],
    prelude: include_str!("namespace_tools.R"),
};

pub(crate) const fn internal(name: &'static str, n: usize, fun: PrimFn) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(n),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
    }
}

fn get_s3_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let s = ctx.session();
    let Some(&generic) = s.str_slice(args.get(0)).ok().and_then(|x| x.first()) else {
        return Err(ctx.stop(call.err_call(), "invalid 'f' argument"));
    };
    let Ok([class]) = s.str_slice(args.get(1)) else {
        return Err(ctx.stop(call.err_call(), "invalid 'class' argument"));
    };
    if generic == CharId::NA || *class == CharId::NA {
        return Err(ctx.stop(call.err_call(), "invalid 'f' or 'class' argument"));
    }
    let name = String::from_utf8_lossy(s.char_bytes(generic)?).into_owned();
    let class_name = String::from_utf8_lossy(s.char_bytes(*class)?).into_owned();
    let env = args.get(3);
    let optional = s.lgl(args.get(2))?.iter().next().and_then(|v| v.get()) == Some(true);
    let s = ctx.session_mut();
    let generic = s.intern(&name)?;
    let generic = ctx.find_fun(generic, env)?;
    ctx.protect(generic);
    let s = ctx.session_mut();
    let definition = s
        .closure_parts(generic)
        .map_or(roxide_value::BASE_ENV, |(_, _, env)| env);
    let sample = s.alloc_int_from(&[])?;
    let class = s.intern("class")?;
    s.set_attrib(sample, class, args.get(1))?;
    if let Some(method) = ctx.explicit_s3_method(&name, sample, env, definition)? {
        return Ok(method);
    }
    if optional {
        Ok(SEXP_NIL)
    } else {
        Err(ctx.stop(
            call.err_call(),
            format!("S3 method '{name}.{class_name}' not found"),
        ))
    }
}

fn url_encode(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let s = ctx.session();
    let reserved = s.lgl(args.get(1))?.iter().next().and_then(|v| v.get()) == Some(true);
    let repeated = s.lgl(args.get(2))?.iter().next().and_then(|v| v.get()) == Some(true);
    let values = s.str_slice(args.get(0))?.to_vec();
    let max = ctx.host().limits().max_vector_bytes;
    let mut total = 0usize;
    let mut ids = Vec::with_capacity(values.len());
    for id in values {
        if id == CharId::NA {
            ids.push(id);
            continue;
        }
        let s = ctx.session();
        let raw = s.char_bytes(id)?;
        let text;
        let bytes = if s.char_encoding(id)? == Encoding::Latin1 {
            text = raw.iter().map(|&b| char::from(b)).collect::<String>();
            text.as_bytes()
        } else {
            raw
        };
        if !repeated
            && bytes
                .windows(3)
                .any(|w| w[0] == b'%' && w[1..].iter().all(u8::is_ascii_hexdigit))
        {
            ids.push(id);
            continue;
        }
        let mut encoded = String::new();
        for &b in bytes {
            if b.is_ascii_alphanumeric()
                || b"-_.~".contains(&b)
                || (!reserved && b":/?#[]@!$&'()*+,;=".contains(&b))
            {
                encoded.push(char::from(b));
            } else {
                let _ = write!(encoded, "%{b:02X}");
            }
            if total.saturating_add(encoded.len()) > max {
                return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
            }
        }
        total += encoded.len();
        ids.push(
            ctx.session_mut()
                .mk_char(encoded.as_bytes(), Encoding::Native)?,
        );
    }
    let s = ctx.session_mut();
    let result = s.alloc_str_from(&ids)?;
    let name = s.intern("names")?;
    let names = s.attr(args.get(0), name);
    s.set_attrib(result, name, names)?;
    Ok(result)
}

//! Access to named grid fields and recycled vectors.

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, NA_INTEGER, SEXP_NIL, SexpType, ValueApi};

pub(super) fn attr(ctx: &mut Ctx, value: SexpId, name: &str) -> EvalResult {
    let sym = ctx.session_mut().intern(name)?;
    Ok(ctx.session().attr(value, sym))
}

pub(super) fn get(ctx: &mut Ctx, value: SexpId, name: &str) -> EvalResult {
    match ctx.session().type_of(value)? {
        SexpType::Env => {
            let sym = ctx.session_mut().intern(name)?;
            Ok(ctx
                .session()
                .env_get(value, sym)
                .map_or(SEXP_NIL, |b| b.value))
        }
        SexpType::Vec => {
            let names = attr(ctx, value, "names")?;
            if names != SEXP_NIL {
                for (i, &id) in ctx.session().str_slice(names)?.iter().enumerate() {
                    if id != CharId::NA && ctx.session().char_bytes(id)? == name.as_bytes() {
                        return Ok(ctx.session().list_get(value, i)?);
                    }
                }
            }
            Ok(SEXP_NIL)
        }
        _ => Ok(SEXP_NIL),
    }
}

pub(super) fn put(ctx: &mut Ctx, env: SexpId, name: &str, value: SexpId) -> EvalResult<()> {
    let sym = ctx.session_mut().intern(name)?;
    Ok(ctx.session_mut().env_define(env, sym, value)?)
}

pub(super) fn attrib(ctx: &mut Ctx, obj: SexpId, name: &str, value: SexpId) -> EvalResult<()> {
    let sym = ctx.session_mut().intern(name)?;
    Ok(ctx.session_mut().set_attrib(obj, sym, value)?)
}

pub(super) fn strings(ctx: &mut Ctx, values: &[&str]) -> EvalResult {
    let ids = values
        .iter()
        .map(|s| {
            ctx.session_mut()
                .mk_char(s.as_bytes(), roxide_value::Encoding::Native)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ctx.session_mut().alloc_str_from(&ids)?)
}

pub(super) fn list(ctx: &mut Ctx, values: &[SexpId]) -> EvalResult {
    let out = ctx.session_mut().alloc_list(values.len())?;
    for (i, &value) in values.iter().enumerate() {
        ctx.session_mut().list_set(out, i, value)?;
    }
    Ok(out)
}

pub(super) fn named(ctx: &mut Ctx, entries: &[(&str, SexpId)]) -> EvalResult {
    let out = list(
        ctx,
        &entries.iter().map(|&(_, value)| value).collect::<Vec<_>>(),
    )?;
    let names = strings(
        ctx,
        &entries.iter().map(|&(name, _)| name).collect::<Vec<_>>(),
    )?;
    attrib(ctx, out, "names", names)?;
    Ok(out)
}

pub(super) fn real(ctx: &mut Ctx, value: SexpId, index: usize, default: f64) -> EvalResult<f64> {
    let len = ctx.session().xlength(value)?;
    if len == 0 {
        return Ok(default);
    }
    let i = index % len;
    Ok(match ctx.session().type_of(value)? {
        SexpType::Real => ctx.session().real(value)?.get(i),
        SexpType::Int => {
            let v = ctx.session().int(value)?.get(i);
            if v == NA_INTEGER {
                roxide_value::NA_REAL
            } else {
                f64::from(v)
            }
        }
        SexpType::Lgl => ctx
            .session()
            .lgl(value)?
            .get(i)
            .get()
            .map_or(roxide_value::NA_REAL, |v| f64::from(u8::from(v))),
        _ => {
            let (number, _) = ctx.session_mut().coerce(value, SexpType::Real)?;
            ctx.session().real(number)?.get(i)
        }
    })
}

pub(super) fn num(
    ctx: &mut Ctx,
    value: SexpId,
    name: &str,
    i: usize,
    default: f64,
) -> EvalResult<f64> {
    let field = get(ctx, value, name)?;
    real(ctx, field, i, default)
}

pub(super) fn text(ctx: &Ctx, value: SexpId, i: usize) -> EvalResult<Option<String>> {
    if value == SEXP_NIL {
        return Ok(None);
    }
    let chars = ctx.session().str_slice(value)?;
    let Some(&id) = chars.get(i.checked_rem(chars.len()).unwrap_or(0)) else {
        return Ok(None);
    };
    if id == CharId::NA {
        return Ok(None);
    }
    let bytes = ctx.session().char_bytes(id)?;
    Ok(Some(
        if ctx.session().char_encoding(id)? == roxide_value::Encoding::Latin1 {
            bytes.iter().copied().map(char::from).collect()
        } else {
            String::from_utf8_lossy(bytes).into_owned()
        },
    ))
}

pub(super) fn inherits(ctx: &mut Ctx, value: SexpId, name: &str) -> EvalResult<bool> {
    let class = attr(ctx, value, "class")?;
    if class != SEXP_NIL {
        for &id in ctx.session().str_slice(class)? {
            if id != CharId::NA && ctx.session().char_bytes(id)? == name.as_bytes() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

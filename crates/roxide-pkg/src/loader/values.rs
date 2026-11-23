use roxide_eval::{
    ArgEval, Arity, Ctx, Dispatch, EvalResult, PrimDef, PrimFn, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{BASE_ENV, CharId, Encoding, SEXP_NIL, SexpType, ValueApi};

pub(crate) const fn internal(
    name: &'static str,
    arity: usize,
    fun: PrimFn,
    visibility: Visibility,
) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(arity),
        dispatch: Dispatch::None,
        visibility,
        fun,
    }
}

pub(crate) fn local(ctx: &mut Ctx, env: SexpId, name: &str) -> EvalResult<Option<SexpId>> {
    let key = ctx.session_mut().intern(name)?;
    ctx.get_local(key, env)
}

pub(crate) fn define(ctx: &mut Ctx, env: SexpId, name: &str, value: SexpId) -> EvalResult<()> {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session_mut().env_define(env, key, value)?)
}

pub(crate) fn attr(ctx: &mut Ctx, obj: SexpId, name: &str) -> EvalResult<SexpId> {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session().attr(obj, key))
}

pub(crate) fn set_attr(ctx: &mut Ctx, obj: SexpId, name: &str, value: SexpId) -> EvalResult<()> {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session_mut().set_attrib(obj, key, value)?)
}

pub(crate) fn names(ctx: &Ctx, value: SexpId) -> EvalResult<Vec<Option<String>>> {
    ctx.session()
        .str_slice(value)?
        .iter()
        .map(|&id| {
            if id == CharId::NA {
                return Ok(None);
            }
            Ok(Some(
                String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned(),
            ))
        })
        .collect()
}

pub(crate) fn chars(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<String>> {
    if value == SEXP_NIL {
        return Ok(Vec::new());
    }
    if ctx.session().type_of(value)? != SexpType::Str {
        return Err(ctx.stop(SEXP_NIL, "invalid character argument"));
    }
    names(ctx, value)?
        .into_iter()
        .map(|item| item.ok_or_else(|| ctx.stop(SEXP_NIL, "invalid character argument")))
        .collect()
}

pub(crate) fn string(ctx: &mut Ctx, value: SexpId) -> EvalResult<String> {
    match chars(ctx, value)?.as_mut_slice() {
        [name] => Ok(std::mem::take(name)),
        _ => Err(ctx.stop(SEXP_NIL, "invalid character argument")),
    }
}

pub(crate) fn strings<'a>(
    ctx: &mut Ctx,
    values: impl IntoIterator<Item = &'a str>,
) -> EvalResult<SexpId> {
    optional_strings(ctx, values.into_iter().map(Some))
}

pub(crate) fn optional_strings<'a>(
    ctx: &mut Ctx,
    values: impl IntoIterator<Item = Option<&'a str>>,
) -> EvalResult<SexpId> {
    let ids = values
        .into_iter()
        .map(|value| {
            value.map_or(Ok(CharId::NA), |text| {
                ctx.session_mut().mk_char(
                    text.as_bytes(),
                    if text.is_ascii() {
                        Encoding::Native
                    } else {
                        Encoding::Utf8
                    },
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ctx.session_mut().alloc_str_from(&ids)?)
}

pub(crate) fn named(ctx: &mut Ctx, value: SexpId, keys: &[&str]) -> EvalResult<SexpId> {
    let labels = strings(ctx, keys.iter().copied())?;
    set_attr(ctx, value, "names", labels)?;
    Ok(value)
}

pub(crate) fn list(ctx: &mut Ctx, values: &[SexpId]) -> EvalResult<SexpId> {
    let out = ctx.session_mut().alloc_list(values.len())?;
    for (i, &value) in values.iter().enumerate() {
        ctx.session_mut().list_set(out, i, value)?;
    }
    Ok(out)
}

pub(crate) fn field(ctx: &mut Ctx, value: SexpId, name: &str) -> EvalResult<Option<SexpId>> {
    let labels = attr(ctx, value, "names")?;
    if labels == SEXP_NIL {
        return Ok(None);
    }
    let index = names(ctx, labels)?
        .iter()
        .position(|x| x.as_deref() == Some(name));
    index
        .map(|i| Ok(ctx.session().list_get(value, i)?))
        .transpose()
}

pub(crate) fn flag(ctx: &mut Ctx, value: SexpId) -> EvalResult<bool> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
    let value = ctx
        .session()
        .lgl(value)?
        .iter()
        .next()
        .and_then(|item| item.get());
    value.ok_or_else(|| ctx.stop(SEXP_NIL, "missing value where TRUE/FALSE needed"))
}

pub(crate) fn function(ctx: &Ctx, value: SexpId) -> EvalResult<bool> {
    Ok(matches!(
        ctx.session().type_of(value)?,
        SexpType::Clo | SexpType::Builtin | SexpType::Special
    ))
}

pub(crate) fn base_call(ctx: &mut Ctx, name: &str, values: &[SexpId]) -> EvalResult<SexpId> {
    let sym = ctx.session_mut().intern(name)?;
    let fun = ctx.find_fun(sym, BASE_ENV)?;
    ctx.apply(
        fun,
        &values
            .iter()
            .copied()
            .map(|value| (None, value))
            .collect::<Vec<_>>(),
        SEXP_NIL,
        BASE_ENV,
    )
}

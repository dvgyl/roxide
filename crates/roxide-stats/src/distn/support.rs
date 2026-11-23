use roxide_eval::{
    ArgEval, Arity, Call, Ctx, Dispatch, EvalResult, PrimDef, PrimFn, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{NA_INTEGER, NA_REAL, SEXP_NIL, SexpType, ValueApi};

/// Declare an internal function with positional arguments.
pub(crate) const fn internal(name: &'static str, arity: usize, fun: PrimFn) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(arity),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
        fun,
    }
}

/// Check the host vector size limit for these elements.
pub(crate) fn limit<T>(ctx: &mut Ctx, call: &Call, len: usize) -> EvalResult<()> {
    if len > ctx.host().limits().max_vector_bytes / std::mem::size_of::<T>() {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    Ok(())
}

/// Check for a logical, integer, or double vector.
pub(crate) fn numeric(ctx: &Ctx, x: SexpId) -> EvalResult<bool> {
    Ok(matches!(
        ctx.session().type_of(x)?,
        SexpType::Lgl | SexpType::Int | SexpType::Real
    ))
}

/// Copy a vector to doubles after checking its size.
pub(crate) fn numbers(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Vec<f64>> {
    limit::<f64>(ctx, call, ctx.session().xlength(x)?)?;
    match ctx.session().type_of(x)? {
        SexpType::Lgl => Ok(ctx
            .session()
            .lgl(x)?
            .iter()
            .map(|v| v.get().map_or(NA_REAL, |v| f64::from(u8::from(v))))
            .collect()),
        SexpType::Int => Ok(ctx
            .session()
            .int(x)?
            .iter()
            .map(|v| {
                if v == NA_INTEGER {
                    NA_REAL
                } else {
                    f64::from(v)
                }
            })
            .collect()),
        SexpType::Real => Ok(ctx.session().real(x)?.iter().collect()),
        _ => {
            let (v, _) = ctx.session_mut().coerce(x, SexpType::Real)?;
            Ok(ctx.session().real(v)?.iter().collect())
        }
    }
}

/// Get the first element after coercion to double.
pub(crate) fn number(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<f64> {
    limit::<f64>(ctx, call, ctx.session().xlength(x)?)?;
    let (v, _) = ctx.session_mut().coerce(x, SexpType::Real)?;
    Ok(ctx.session().real(v)?.iter().next().unwrap_or(NA_REAL))
}

/// Get the first element after coercion to logical.
pub(crate) fn logical(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Option<bool>> {
    limit::<roxide_value::RLogical>(ctx, call, ctx.session().xlength(x)?)?;
    let (x, _) = ctx.session_mut().coerce(x, SexpType::Lgl)?;
    Ok(ctx.session().lgl(x)?.iter().next().and_then(|x| x.get()))
}

/// Get an attribute by name.
pub(crate) fn attr(ctx: &mut Ctx, x: SexpId, name: &str) -> EvalResult {
    let sym = ctx.session_mut().intern(name)?;
    Ok(ctx.session().attr(x, sym))
}

/// Set an attribute by name.
pub(crate) fn set_attr(ctx: &mut Ctx, x: SexpId, name: &str, value: SexpId) -> EvalResult<()> {
    let sym = ctx.session_mut().intern(name)?;
    ctx.session_mut().set_attrib(x, sym, value)?;
    Ok(())
}

/// Set the row and column names when at least one is present.
pub(crate) fn set_dimnames(ctx: &mut Ctx, x: SexpId, row: SexpId, col: SexpId) -> EvalResult<()> {
    if row != SEXP_NIL || col != SEXP_NIL {
        let names = ctx.session_mut().alloc_list(2)?;
        ctx.protect(names);
        ctx.session_mut().list_set(names, 0, row)?;
        ctx.session_mut().list_set(names, 1, col)?;
        set_attr(ctx, x, "dimnames", names)?;
    }
    Ok(())
}

/// Set both matrix dimensions.
pub(crate) fn set_dim(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    rows: usize,
    cols: usize,
) -> EvalResult<()> {
    let (Ok(rows), Ok(cols)) = (i32::try_from(rows), i32::try_from(cols)) else {
        return Err(ctx.stop(call.err_call(), "matrix size limit exceeded"));
    };
    let dim = ctx.session_mut().alloc_int_from(&[rows, cols])?;
    ctx.protect(dim);
    set_attr(ctx, x, "dim", dim)
}

/// Check the explicit class vector for a class name.
pub(crate) fn has_class(ctx: &mut Ctx, x: SexpId, class: &str) -> EvalResult<bool> {
    let classes = attr(ctx, x, "class")?;
    Ok(ctx
        .session()
        .str_slice(classes)
        .unwrap_or_default()
        .iter()
        .any(|&id| ctx.session().char_bytes(id).ok() == Some(class.as_bytes())))
}

/// Copy the attribute list while the caller protects the output.
pub(crate) fn copy_attrs(ctx: &mut Ctx, source: SexpId, result: SexpId) -> EvalResult<()> {
    for (key, value) in ctx.session().attrs(source)? {
        ctx.session_mut().set_attrib(result, key, value)?;
    }
    Ok(())
}

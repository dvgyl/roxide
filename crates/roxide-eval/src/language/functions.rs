use roxide_value::{GLOBAL_ENV, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use crate::{Args, Call, Ctx, EvalResult};

pub(super) fn closure(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    Ok(ctx
        .session
        .alloc_closure(args.get(0), args.get(1), call.env)?)
}

pub(super) fn function_parts(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let kind = ctx.prims[usize::from(call.prim.0)].name;
    let mut fun = args.get(0);
    if matches!(ctx.session.type_of(fun)?, SexpType::Str | SexpType::Sym) {
        let name = ctx.name(fun, call.err_call())?;
        let tag = ctx.session.intern(&name)?;
        fun = ctx.find_fun_at(tag, call.env, call.err_call())?;
    }
    let (formals, body) = if let Ok((formals, body, _)) = ctx.session.closure_parts(fun) {
        (formals, body)
    } else if kind == "args"
        && let Some(name) = ctx.primitive_name(fun)
    {
        let signature: &[&str] = match name {
            "sum" | "min" | "max" | "prod" | "any" | "all" => &["...", "na.rm"],
            "c" => &["..."],
            "rep" => &["x", "..."],
            "seq.int" => &["from", "to", "by", "length.out", "along.with", "..."],
            "log" => &["x", "base"],
            "round" | "signif" => &["x", "digits", "..."],
            "anyNA" => &["x", "recursive"],
            "length" | "sqrt" | "abs" | "sin" | "cos" | "tan" | "exp" | "is.null" | "is.na"
            | "names" | "is.finite" | "is.infinite" => &["x"],
            _ => return Ok(SEXP_NIL),
        };
        let mut pairs = Vec::with_capacity(signature.len());
        for &name in signature {
            let default = match name {
                "na.rm" | "recursive" => roxide_value::FALSE_SCALAR,
                "base" => {
                    let head = ctx.session.intern("exp")?.as_sexp();
                    let value = ctx.session.scalar_real(1.0)?;
                    ctx.make_call(head, &Args::from_pairs(vec![(None, value)]))?
                }
                "digits" if ctx.primitive_name(fun) == Some("round") => {
                    ctx.session.scalar_real(0.0)?
                }
                "digits" => ctx.session.scalar_real(6.0)?,
                _ => SEXP_MISSING,
            };
            pairs.push((Some(ctx.session.intern(name)?), default));
        }
        (ctx.pairlist(&Args::from_pairs(pairs), false)?, SEXP_NIL)
    } else {
        return Ok(SEXP_NIL);
    };
    match kind {
        "args" => Ok(ctx.session.alloc_closure(formals, SEXP_NIL, GLOBAL_ENV)?),
        "formals" => Ok(formals),
        _ => Ok(body),
    }
}

pub(super) fn set_function_parts(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["fun", "envir", "value"], call.err_call())?;
    let fun = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let (mut formals, mut body, env) = ctx
        .session
        .closure_parts(fun)
        .map_err(|_| ctx.stop(call.err_call(), "argument is not a function"))?;
    let (env, value) = if args.len() == 2 && args.named(&ctx.session, "value").is_none() {
        (env, args.get(1))
    } else {
        (
            ctx.matched_arg(args, &matched, 1).unwrap_or(env),
            ctx.matched_arg(args, &matched, 2).unwrap_or(SEXP_NIL),
        )
    };
    ctx.require_env(env, call.err_call())?;
    if ctx.prims[usize::from(call.prim.0)].name == "formals<-" {
        let pairs = ctx.list_args(value)?;
        if pairs.iter().any(|(tag, _)| tag.is_none())
            || !matches!(
                ctx.session.type_of(value)?,
                SexpType::Vec | SexpType::List | SexpType::Nil
            )
        {
            return Err(ctx.stop(
                call.err_call(),
                "invalid formal argument list for 'function'",
            ));
        }
        formals = ctx.pairlist(&pairs, false)?;
    } else {
        body = value;
    }
    Ok(ctx.session.alloc_closure(formals, body, env)?)
}

pub(super) fn as_function(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["x", "envir"], call.err_call())?;
    let value = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    if matches!(
        ctx.session.type_of(value)?,
        SexpType::Clo | SexpType::Builtin | SexpType::Special
    ) {
        return Ok(value);
    }
    let env = ctx.matched_arg(args, &matched, 1).unwrap_or(call.env);
    ctx.require_env(env, call.err_call())?;
    let mut pairs: Vec<_> = ctx.list_args(value)?.iter().collect();
    let Some((_, body)) = pairs.pop() else {
        return Err(ctx.stop(call.err_call(), "list argument expected"));
    };
    if pairs.iter().any(|(tag, _)| tag.is_none()) {
        return Err(ctx.stop(
            call.err_call(),
            "invalid formal argument list for 'function'",
        ));
    }
    let formals = ctx.pairlist(&Args::from_pairs(pairs), false)?;
    Ok(ctx.session.alloc_closure(formals, body, env)?)
}

pub(super) fn match_fun(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["FUN", "descend"], call.err_call())?;
    let fun = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    if matches!(
        ctx.session.type_of(fun)?,
        SexpType::Clo | SexpType::Builtin | SexpType::Special
    ) {
        return Ok(fun);
    }
    let name = ctx.name(fun, call.err_call())?;
    let tag = ctx.session.intern(&name)?;
    if let Some(descend) = ctx.matched_arg(args, &matched, 1)
        && !ctx.truth(descend, call.err_call())?
    {
        return ctx
            .get_var(tag, call.env)?
            .ok_or_else(|| ctx.stop(call.err_call(), format!("object '{name}' not found")));
    }
    ctx.find_fun_at(tag, call.env, call.err_call())
}

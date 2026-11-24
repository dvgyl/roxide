use roxide_gc::SexpId;
use roxide_value::{GLOBAL_ENV, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use crate::{Args, Call, Ctx, EvalResult};

pub(super) fn quote(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if let Some(tag) = args.name(0) {
        let name = ctx.session.sym_name(tag)?;
        if name != "expr" {
            return Err(ctx.stop(
                call.err_call(),
                format!("supplied argument name '{name}' does not match 'expr'"),
            ));
        }
    }
    Ok(args.get(0))
}

pub(super) fn substitute(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.is_empty() {
        return Ok(SEXP_MISSING);
    }
    let matched = ctx.match_names(args, &["expr", "env"], call.err_call())?;
    let expr = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_MISSING);
    let env = ctx.eval_arg(args, &matched, 1, call, call.env)?;
    let frame = match ctx.session.type_of(env)? {
        SexpType::Env => env,
        SexpType::Vec | SexpType::List => ctx.data_env(env, roxide_value::EMPTY_ENV)?,
        SexpType::Nil => roxide_value::EMPTY_ENV,
        _ => return Err(ctx.stop(call.err_call(), "invalid environment specified")),
    };
    substitute_expr(ctx, expr, frame, 0, call.err_call())
}

fn substitute_expr(
    ctx: &mut Ctx,
    expr: SexpId,
    frame: SexpId,
    depth: usize,
    call: SexpId,
) -> EvalResult {
    if depth >= 128 {
        return Err(ctx.stop(call, "substitute depth limit exceeded"));
    }
    match ctx.session.type_of(expr)? {
        SexpType::Prom => ctx.promise_expr(expr),
        SexpType::Sym if expr != SEXP_MISSING && frame != GLOBAL_ENV => {
            let sym = ctx.sym_from_sexp(expr)?;
            let Some(binding) = ctx.session.env_get(frame, sym) else {
                return Ok(expr);
            };
            let name = ctx.session.sym_name(sym)?;
            if name == "..." {
                return if binding.value == SEXP_MISSING {
                    Ok(SEXP_NIL)
                } else {
                    let first = ctx
                        .session
                        .pairlist_iter(binding.value)
                        .next()
                        .map_or(SEXP_NIL, |(_, val)| val);
                    ctx.promise_expr(first)
                };
            }
            ctx.promise_expr(binding.value)
        }
        ty @ (SexpType::Lang | SexpType::List) => {
            let (head, tail, _) = ctx.session.cons_parts(expr)?;
            let (head, tail) = if ty == SexpType::Lang {
                (
                    Some(substitute_expr(ctx, head, frame, depth + 1, call)?),
                    tail,
                )
            } else {
                (None, expr)
            };
            let args = Args::from_pairs(ctx.session.pairlist_iter(tail).collect());
            let dots = ctx.session.intern("...")?;
            let mut pairs = Vec::new();
            for (tag, expr) in args.iter() {
                if expr == dots.as_sexp()
                    && frame != GLOBAL_ENV
                    && let Some(binding) = ctx.session.env_get(frame, dots)
                {
                    for (tag, value) in ctx.session.pairlist_iter(binding.value) {
                        pairs.push((tag, ctx.promise_expr(value)?));
                    }
                } else {
                    pairs.push((tag, substitute_expr(ctx, expr, frame, depth + 1, call)?));
                }
            }
            let pairs = Args::from_pairs(pairs);
            if let Some(head) = head {
                ctx.make_call(head, &pairs)
            } else {
                ctx.pairlist(&pairs, false)
            }
        }
        _ => Ok(expr),
    }
}

pub(super) fn bquote(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["expr", "where", "splice"], call.err_call())?;
    let target = ctx.eval_arg(args, &matched, 1, call, call.env)?;
    let env = match ctx.session.type_of(target)? {
        SexpType::Env => target,
        SexpType::Vec | SexpType::List | SexpType::Nil => ctx.data_env(target, call.env)?,
        _ => return Err(ctx.stop(call.err_call(), "invalid 'where' argument")),
    };
    ctx.protect(env);
    let splice = ctx.eval_flag(args, &matched, 2, call, false)?;
    let expr = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_MISSING);
    let expr = ctx.promise_expr(expr)?;
    bquote_expr(ctx, expr, env, splice, 0)
}

fn bquote_expr(ctx: &mut Ctx, expr: SexpId, env: SexpId, splice: bool, depth: usize) -> EvalResult {
    if depth >= ctx.opts.expressions.min(256) {
        return Err(ctx.stop(SEXP_NIL, "bquote expression is too deeply nested"));
    }
    let ty = ctx.session.type_of(expr)?;
    if !matches!(ty, SexpType::Lang | SexpType::List) {
        return Ok(expr);
    }
    if ty == SexpType::Lang {
        let (head, tail, _) = ctx.session.cons_parts(expr)?;
        match ctx.session.sym_name_of(head).ok() {
            Some(".") => return ctx.eval(ctx.session.cons_parts(tail)?.0, env),
            Some("..") if splice => {
                return Err(ctx.stop(SEXP_NIL, "can only splice inside a call"));
            }
            _ => {}
        }
    }
    let items: Vec<_> = ctx.session.pairlist_iter(expr).collect();
    let mut pairs = Vec::new();
    for (i, (tag, item)) in items.into_iter().enumerate() {
        if splice && (i > 0 || ty == SexpType::List) && ctx.session.type_of(item)? == SexpType::Lang
        {
            let (head, tail, _) = ctx.session.cons_parts(item)?;
            if ctx.session.sym_name_of(head).ok() == Some("..") {
                let value = ctx.eval(ctx.session.cons_parts(tail)?.0, env)?;
                ctx.protect(value);
                let value = match ctx.session.type_of(value)? {
                    SexpType::Vec | SexpType::Expr | SexpType::Nil => value,
                    SexpType::Lgl
                    | SexpType::Int
                    | SexpType::Real
                    | SexpType::Cplx
                    | SexpType::Str
                    | SexpType::Raw => ctx.session.coerce(value, SexpType::Vec)?.0,
                    _ => return Err(ctx.stop(SEXP_NIL, "can only splice vectors")),
                };
                ctx.protect(value);
                pairs.extend(ctx.list_args(value)?.iter());
                continue;
            }
        }
        let value = bquote_expr(ctx, item, env, splice, depth + 1)?;
        ctx.protect(value);
        pairs.push((tag, value));
    }
    let args = Args::from_pairs(pairs);
    if ty == SexpType::Lang {
        ctx.make_call(
            args.get(0),
            &Args::from_pairs(args.iter().skip(1).collect()),
        )
    } else {
        ctx.pairlist(&args, false)
    }
}

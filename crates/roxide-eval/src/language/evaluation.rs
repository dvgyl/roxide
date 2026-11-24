use roxide_gc::SexpId;
use roxide_value::{BASE_ENV, CharId, DeparseOpts, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use crate::control::logical;
use crate::ctx::Frame;
use crate::eval::ArgInput;
use crate::{Args, Call, Ctx, EvalResult};

pub(super) fn with_visible(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = ctx.session.alloc_env(BASE_ENV)?;
    let frame = evaluation_frame(ctx, call, args, env);
    let value = ctx.run_frame(frame, |ctx| ctx.eval(args.get(0), call.env))?;
    ctx.protect(value);
    let visible = ctx.session.scalar_lgl(logical(ctx.visible))?;
    let value_tag = ctx.session.intern("value")?;
    let visible_tag = ctx.session.intern("visible")?;
    ctx.list(&Args::from_pairs(vec![
        (Some(value_tag), value),
        (Some(visible_tag), visible),
    ]))
}

pub(super) fn evaluate(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let kind = ctx.prims[usize::from(call.prim.0)].name;
    if kind == "local" {
        return evaluate_in_frame(ctx, call, args, kind, call.env);
    }
    let env = ctx.session.alloc_env(BASE_ENV)?;
    let frame = evaluation_frame(ctx, call, args, env);
    ctx.run_frame(frame, |ctx| evaluate_in_frame(ctx, call, args, kind, env))
}

fn evaluate_in_frame(
    ctx: &mut Ctx,
    call: &Call,
    args: &Args,
    kind: &str,
    caller: SexpId,
) -> EvalResult {
    let mut names = vec!["expr", "envir"];
    if kind != "local" {
        names.push("enclos");
    }
    let matched = ctx.match_names(args, &names, call.err_call())?;
    let Some(mut expr) = ctx.matched_arg(args, &matched, 0) else {
        return Err(ctx.stop(
            call.err_call(),
            "argument \"expr\" is missing, with no default",
        ));
    };
    if kind == "eval" {
        expr = ctx.eval(expr, call.env)?;
    }
    ctx.protect(expr);
    let env = if kind == "local" && ctx.matched_arg(args, &matched, 1).is_none() {
        ctx.session.alloc_env(call.env)?
    } else {
        ctx.eval_arg(args, &matched, 1, call, call.env)?
    };
    ctx.protect(env);
    let parent = if kind == "local" {
        call.env
    } else {
        ctx.eval_arg(args, &matched, 2, call, call.env)?
    };
    let parent = if parent == SEXP_NIL { BASE_ENV } else { parent };
    let env = match ctx.session.type_of(env)? {
        SexpType::Nil | SexpType::Vec | SexpType::List => ctx.data_env(env, parent)?,
        SexpType::Int | SexpType::Real if kind != "local" => ctx.eval_env_at(env, call)?,
        _ => ctx.require_env(env, call.err_call())?,
    };
    ctx.protect(env);
    let mut frame = evaluation_frame(ctx, call, args, env);
    frame.caller = caller;
    ctx.run_frame(frame, |ctx| {
        if ctx.session.type_of(expr)? == SexpType::Expr {
            let mut value = SEXP_NIL;
            for i in 0..ctx.session.xlength(expr)? {
                value = ctx.eval(ctx.session.list_get(expr, i)?, env)?;
            }
            Ok(value)
        } else {
            ctx.eval(expr, env)
        }
    })
}

fn evaluation_frame(ctx: &mut Ctx, call: &Call, args: &Args, env: SexpId) -> Frame {
    Frame {
        id: ctx.next_frame(),
        cloenv: env,
        call: call.expr,
        on_exit: Vec::new(),
        fun: SEXP_NIL,
        caller: call.env,
        actuals: args.clone(),
        supplied: Vec::new(),
        dispatch: None,
    }
}

pub(super) fn do_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["what", "args", "quote", "envir"], call.err_call())?;
    let what = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let list = ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL);
    let env = ctx.matched_arg(args, &matched, 3).unwrap_or(call.env);
    ctx.require_env(env, call.err_call())?;
    let quote = match ctx.matched_arg(args, &matched, 2) {
        Some(value) => ctx.truth(value, call.err_call())?,
        None => false,
    };
    if !matches!(ctx.session.type_of(list)?, SexpType::Vec | SexpType::Nil) {
        return Err(ctx.stop(call.err_call(), "second argument must be a list"));
    }
    let args = ctx.list_args(list)?;
    let head = if ctx.session.type_of(what)? == SexpType::Str {
        let name = ctx.string(what, call.err_call())?;
        ctx.session.intern(&name)?.as_sexp()
    } else {
        what
    };
    let args = if quote {
        let namespace = ctx.session.intern("::")?.as_sexp();
        let base = ctx.session.intern("base")?.as_sexp();
        let quote = ctx.session.intern("quote")?.as_sexp();
        let quote = ctx.make_call(
            namespace,
            &Args::from_pairs(vec![(None, base), (None, quote)]),
        )?;
        let pairs = args
            .iter()
            .map(|(tag, value)| {
                Ok((
                    tag,
                    ctx.make_call(quote, &Args::from_pairs(vec![(None, value)]))?,
                ))
            })
            .collect::<EvalResult<Vec<_>>>()?;
        Args::from_pairs(pairs)
    } else {
        args
    };
    let expr = ctx.make_call(head, &args)?;
    ctx.eval(expr, env)
}

pub(super) fn internal(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let expr = args.get(0);
    if ctx.session.type_of(expr)? != SexpType::Lang {
        return Err(ctx.stop(call.err_call(), "invalid .Internal() argument"));
    }
    let (head, tail, _) = ctx.session.cons_parts(expr)?;
    let name = ctx.session.sym_name_of(head)?.to_owned();
    let Some(&prim) = ctx.internals.get(name.as_str()) else {
        return Err(ctx.stop(
            call.err_call(),
            format!("there is no .Internal function '{name}'"),
        ));
    };
    let user_call = ctx
        .frame(call.env)
        .map_or(call.err_call(), |frame| frame.call);
    let call = Call {
        expr,
        prim,
        env: call.env,
        user_call,
    };
    ctx.run_primitive(
        call,
        Args::from_pairs(ctx.session.pairlist_iter(tail).collect()),
        ArgInput::Expressions,
        true,
    )
}

pub(super) fn recall(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let Some(frame) = ctx
        .frame(call.env)
        .cloned()
        .filter(|frame| frame.fun != SEXP_NIL)
    else {
        return Err(ctx.stop(call.err_call(), "'Recall' called from outside a closure"));
    };
    let args = ctx.promise_args(args, call.env, call.err_call())?;
    ctx.invoke_closure(frame.fun, args, frame.call, call.env, frame.dispatch)
}

pub(super) fn eval_parent(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["expr", "n"], call.err_call())?;
    let expr = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let n = ctx.matched_arg(args, &matched, 1);
    let env = if n.is_some_and(|value| ctx.integer(value, call.err_call()).ok() == Some(0)) {
        call.env
    } else {
        let args = n.map_or_else(Vec::new, |value| vec![(None, value)]);
        let fun = ctx.session.intern("parent.frame")?;
        let fun = ctx.find_fun(fun, BASE_ENV)?;
        ctx.apply(fun, &args, call.err_call(), call.env)?
    };
    if ctx.session.type_of(expr)? == SexpType::Expr {
        let mut result = SEXP_NIL;
        for i in 0..ctx.session.xlength(expr)? {
            result = ctx.eval(ctx.session.list_get(expr, i)?, env)?;
        }
        Ok(result)
    } else {
        ctx.eval(expr, env)
    }
}

pub(super) fn force_and_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.len() < 2 {
        return Err(ctx.stop(call.err_call(), "at least two arguments are required"));
    }
    let n = ctx.eval(args.get(0), call.env)?;
    let n = ctx.integer(n, call.err_call())?.max(0) as usize;
    let head = args.get(1);
    let fun = if ctx.session.type_of(head)? == SexpType::Sym {
        let tag = ctx.sym_from_sexp(head)?;
        ctx.find_fun_at(tag, call.env, call.err_call())?
    } else {
        ctx.eval(head, call.env)?
    };
    ctx.protect(fun);
    let args = Args::from_pairs(args.iter().skip(2).collect());
    let expr = ctx.make_call(head, &args)?;
    ctx.protect(expr);
    if ctx.session.type_of(fun)? != SexpType::Clo {
        return ctx.invoke_raw(fun, ctx.session.cons_parts(expr)?.1, expr, call.env);
    }
    let promises = ctx.promise_args(&args, call.env, call.err_call())?;
    for &value in promises.values() {
        ctx.protect(value);
    }
    for &value in promises.values().iter().take(n) {
        if value == SEXP_MISSING {
            return Err(ctx.stop(call.err_call(), "argument is missing, with no default"));
        }
        ctx.force(value)?;
    }
    ctx.invoke_closure(fun, promises, expr, call.env, None)
}

pub(super) fn switch(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let args = ctx.expand_dots(args, call.env, call.err_call())?;
    for &value in args.values() {
        ctx.protect(value);
    }
    if args.is_empty() {
        return Err(ctx.stop(call.err_call(), "'EXPR' is missing"));
    }
    let expr = ctx.eval(args.get(0), call.env)?;
    ctx.resolve_altrep(expr)?;
    if ctx.session.xlength(expr)? != 1 {
        return Err(ctx.stop(call.err_call(), "EXPR must be a length 1 vector"));
    }
    let mut selected = None;
    if args.len() == 1 {
        ctx.warn_now(call.err_call(), "'switch' with no alternatives")?;
    } else if ctx.session.type_of(expr)? == SexpType::Str {
        let id = ctx.session.str_slice(expr)?[0];
        let mut defaults = args
            .iter()
            .skip(1)
            .enumerate()
            .filter_map(|(i, (name, value))| name.is_none().then_some((i + 1, value)));
        for (i, (tag, _)) in args.iter().enumerate().skip(1) {
            if let Some(tag) = tag
                && id != CharId::NA
                && ctx.session.sym_name(tag)?.as_bytes() == ctx.session.char_bytes(id)?
            {
                selected = args.values()[i..]
                    .iter()
                    .copied()
                    .find(|&value| value != SEXP_MISSING);
                break;
            }
        }
        if let Some((_, first)) = defaults.next() {
            if let Some((_, second)) = defaults.next() {
                let deparse = |value| {
                    ctx.deparse_lines(value, DeparseOpts::default())
                        .map(|v| v.join(" "))
                };
                let message = format!(
                    "duplicate 'switch' defaults: '{}' and '{}'",
                    deparse(first)?,
                    deparse(second)?
                );
                return Err(ctx.stop(call.err_call(), message));
            }
            selected = selected.or(Some(first));
        }
    } else {
        let integer = ctx.session.coerce(expr, SexpType::Int)?.0;
        if let Some(index) = ctx.session.int(integer)?.iter().next()
            && index > 0
            && (index as usize) < args.len()
        {
            selected = Some(args.get(index as usize));
        }
    }
    if let Some(expr) = selected {
        if expr == SEXP_MISSING {
            return Err(ctx.stop(call.err_call(), "empty alternative in numeric switch"));
        }
        return ctx.eval(expr, call.env);
    }
    ctx.visible = false;
    Ok(SEXP_NIL)
}

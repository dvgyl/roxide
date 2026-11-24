use roxide_value::{BASE_ENV, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use crate::eval::ArgInput;
use crate::{Args, Call, Ctx, EvalResult, Transfer, Unwind};

use super::MethodFrame;

pub(super) fn use_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let Some(frame) = ctx.frame(call.env).cloned() else {
        return Err(ctx.stop(
            call.err_call(),
            "'UseMethod' used in an inappropriate fashion",
        ));
    };
    let matched = ctx.match_names(args, &["generic", "object"], call.err_call())?;
    let generic = ctx.eval_arg(args, &matched, 0, call, SEXP_NIL)?;
    let generic = ctx.string(generic, call.err_call())?;
    let (formals, _, definition) = ctx.session.closure_parts(frame.fun)?;
    let formals = Args::from_pairs(ctx.session.pairlist_iter(formals).collect());
    let object = if let Some(expr) = ctx.matched_arg(args, &matched, 1) {
        ctx.eval(expr, call.env)?
    } else {
        let matches = ctx.match_args(&formals, &frame.actuals, frame.call, false)?;
        let slot = matches.slots.first().copied().flatten().or_else(|| {
            let first_dots = formals
                .name(0)
                .is_some_and(|tag| ctx.session.sym_name(tag).ok() == Some("..."));
            if first_dots {
                matches.dots.first().copied()
            } else {
                None
            }
        });
        match slot.map(|i| frame.actuals.get(i)) {
            Some(value) if value != SEXP_MISSING => ctx.eval(value, frame.caller)?,
            _ => SEXP_NIL,
        }
    };
    let classes = ctx.classes(object, true)?;
    let mut method = MethodFrame {
        generic,
        group: None,
        group_method: false,
        names: vec![String::new()],
        classes,
        class: SEXP_NIL,
        offset: 0,
        side: 0,
        caller: frame.caller,
        definition,
        primitive: None,
        is_default: false,
    };
    let Some(candidate) = ctx.find_method(&method, 0, true)? else {
        let classes = if method.classes.len() == 1 {
            format!("\"{}\"", method.classes[0])
        } else {
            format!("c(\"{}\")", method.classes.join("\", \""))
        };
        return Err(ctx.stop(
            call.err_call(),
            format!(
                "no applicable method for '{}' applied to an object of class {classes}",
                method.generic
            ),
        ));
    };
    method.select(ctx, &candidate, SEXP_NIL)?;
    let value = ctx.invoke_method(
        candidate.fun,
        &candidate.name,
        frame.actuals,
        frame.caller,
        method,
    )?;
    Err(Unwind::Transfer {
        target: frame.id,
        kind: Transfer::Return(value),
    })
}

pub(super) fn next_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let Some(frame) = ctx.frame(call.env).cloned() else {
        return Err(ctx.stop(
            call.err_call(),
            "'NextMethod' called from outside a function",
        ));
    };
    let Some(mut method) = frame.dispatch else {
        return Err(ctx.stop(
            call.err_call(),
            "'NextMethod' called from an anonymous function",
        ));
    };
    let args = ctx.promise_args(args, call.env, call.err_call())?;
    for &value in args.values() {
        ctx.protect(value);
    }
    let matched = ctx.match_names(&args, &["generic", "object", "..."], call.err_call())?;
    if let Some(generic) = ctx.matched_arg(&args, &matched, 0) {
        let generic = ctx.eval(generic, call.env)?;
        if generic != SEXP_NIL {
            method.generic = ctx.string(generic, call.err_call())?;
        }
    }
    let (formals, _, _) = ctx.session.closure_parts(frame.fun)?;
    let formals = Args::from_pairs(ctx.session.pairlist_iter(formals).collect());
    let formal_match = ctx.match_args(&formals, &frame.actuals, frame.call, false)?;
    let mut forwarded: Vec<_> = frame.actuals.iter().collect();
    for (position, &i) in formal_match.slots.iter().enumerate() {
        if let Some(i) = i
            && let Some(tag) = formals.name(position)
            && frame.actuals.get(i) != SEXP_MISSING
            && let Some(binding) = ctx.session.env_get(call.env, tag)
        {
            let old = frame.actuals.get(i);
            if old != binding.value {
                let expr = ctx.promise_expr(old)?;
                forwarded[i].1 = if ctx.session.type_of(binding.value)? == SexpType::Prom {
                    binding.value
                } else {
                    ctx.value_promise(expr, binding.value)?
                };
            }
        }
    }
    for i in matched.dots {
        let tag = args.name(i);
        let value = args.get(i);
        if let Some(slot) =
            tag.and_then(|tag| forwarded.iter_mut().find(|(name, _)| *name == Some(tag)))
        {
            slot.1 = value;
        } else {
            forwarded.push((tag, value));
        }
    }
    let forwarded = Args::from_pairs(forwarded);
    for &value in forwarded.values() {
        ctx.protect(value);
    }
    if !method.is_default
        && let Some(candidate) = ctx.find_method(&method, method.offset + 1, true)?
    {
        let old = method.class;
        method.select(ctx, &candidate, old)?;
        return ctx.invoke_method(
            candidate.fun,
            &candidate.name,
            forwarded,
            method.caller,
            method,
        );
    }
    if let Some(primitive) = method.primitive {
        let mut values = Vec::with_capacity(forwarded.len());
        for (tag, value) in forwarded.iter() {
            let value = if value == SEXP_MISSING {
                value
            } else {
                ctx.eval(value, call.env)?
            };
            ctx.protect(value);
            values.push((tag, value));
        }
        return ctx.run_primitive(primitive, Args::from_pairs(values), ArgInput::Values, false);
    }
    let sym = ctx.session.intern(&method.generic)?;
    if let Some(binding) = ctx.session.env_get(BASE_ENV, sym)
        && matches!(
            ctx.session.type_of(binding.value)?,
            SexpType::Builtin | SexpType::Special
        )
    {
        let (prim, _) = ctx.session.prim_parts(binding.value)?;
        return ctx.run_primitive(
            Call { prim, ..*call },
            forwarded,
            ArgInput::Expressions,
            false,
        );
    }
    Err(ctx.stop(call.err_call(), "no method to invoke"))
}

use std::collections::HashSet;

use roxide_gc::SexpId;
use roxide_value::{CharId, PromiseState, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use crate::closure::{Matches, dots_index};
use crate::control::logical;
use crate::{Args, Call, Ctx, EvalResult};

impl Ctx {
    /// Match supplied arguments against the given formal names.
    pub(crate) fn match_names(
        &mut self,
        args: &Args,
        names: &[&str],
        call: SexpId,
    ) -> EvalResult<Matches> {
        let formals = names
            .iter()
            .map(|name| Ok((Some(self.session.intern(name)?), SEXP_MISSING)))
            .collect::<EvalResult<Vec<_>>>()?;
        self.match_args(&Args::from_pairs(formals), args, call, false)
    }

    /// Return the supplied value for a formal slot if it is present.
    pub(crate) fn matched_arg(
        &self,
        args: &Args,
        matched: &Matches,
        slot: usize,
    ) -> Option<SexpId> {
        matched.slots[slot]
            .map(|i| args.get(i))
            .filter(|&value| value != SEXP_MISSING)
    }

    /// Evaluate the supplied argument or return the default value.
    pub(crate) fn eval_arg(
        &mut self,
        args: &Args,
        matched: &Matches,
        slot: usize,
        call: &Call,
        default: SexpId,
    ) -> EvalResult {
        match self.matched_arg(args, matched, slot) {
            Some(expr) => self.eval(expr, call.env),
            None => Ok(default),
        }
    }

    /// Evaluate the supplied argument as a logical flag or return the default.
    pub(crate) fn eval_flag(
        &mut self,
        args: &Args,
        matched: &Matches,
        slot: usize,
        call: &Call,
        default: bool,
    ) -> EvalResult<bool> {
        let Some(expr) = self.matched_arg(args, matched, slot) else {
            return Ok(default);
        };
        let value = self.eval(expr, call.env)?;
        self.truth(value, call.err_call())
    }
}

pub(super) fn dots_length(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    let Some(dots) = ctx.find_dots(call.env)? else {
        return Err(ctx.stop(
            call.err_call(),
            "incorrect context: the current call has no '...' to look in",
        ));
    };
    Ok(ctx
        .session
        .scalar_int(ctx.session.pairlist_iter(dots).count() as i32)?)
}

pub(super) fn dots_element(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let (index, _) = ctx.session.coerce(args.get(0), SexpType::Int)?;
    if ctx.session.xlength(index)? != 1 {
        return Err(ctx.stop(call.err_call(), "invalid integer argument"));
    }
    let index = ctx.session.int(index)?.get(0);
    if index <= 0 {
        return Err(ctx.stop(
            call.err_call(),
            format!("indexing '...' with non-positive index {index}"),
        ));
    }
    let value = ctx.dots_elt(call.env, index as usize, call.err_call())?;
    ctx.eval(value, call.env)
}

pub(super) fn missing(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let expr = args.get(0);
    if !matches!(ctx.session.type_of(expr)?, SexpType::Sym | SexpType::Str) || expr == SEXP_MISSING
    {
        return Err(ctx.stop(call.err_call(), "invalid use of 'missing'"));
    }
    let name = ctx.name(expr, call.err_call())?;
    let sym = ctx.session.intern(&name)?;
    let mut env = call.env;
    let mut expr = sym.as_sexp();
    let mut visited = HashSet::new();
    let mut first = true;
    let absent = loop {
        if !visited.insert((env, expr)) {
            break true;
        }
        let name = ctx.session.sym_name_of(expr)?;
        let binding = if let Some(index) = dots_index(name) {
            let dots = ctx.session.intern("...")?;
            let Some(binding) = ctx.session.env_get(env, dots) else {
                if first {
                    return Err(
                        ctx.stop(call.err_call(), "'missing(...)' did not find an argument")
                    );
                }
                break false;
            };
            if index == 0 {
                break false;
            }
            let Some((_, value)) = ctx.session.pairlist_iter(binding.value).nth(index - 1) else {
                break true;
            };
            roxide_value::Binding { value, ..binding }
        } else {
            let sym = ctx.sym_from_sexp(expr)?;
            let Some(binding) = ctx.session.env_get(env, sym) else {
                if first {
                    return Err(ctx.stop(call.err_call(), "'missing(x)' did not find an argument"));
                }
                break false;
            };
            binding
        };
        if binding.value == SEXP_MISSING || (first && binding.missing != 0) {
            break true;
        }
        if ctx.session.type_of(binding.value)? != SexpType::Prom {
            break false;
        }
        let (original, promise_env, _, state) = ctx.session.promise_parts(binding.value)?;
        if state == PromiseState::Forced || ctx.session.type_of(original)? != SexpType::Sym {
            break false;
        }
        expr = original;
        env = promise_env;
        first = false;
    };
    Ok(ctx.session.scalar_lgl(logical(absent))?)
}

pub(super) fn match_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(
        args,
        &["definition", "call", "expand.dots", "envir"],
        call.err_call(),
    )?;
    let frame = ctx.frame(call.env).cloned();
    let mut definition = ctx.eval_arg(args, &matched, 0, call, SEXP_NIL)?;
    ctx.protect(definition);
    let call_expr = ctx.eval_arg(
        args,
        &matched,
        1,
        call,
        frame.as_ref().map_or(SEXP_NIL, |frame| frame.call),
    )?;
    ctx.protect(call_expr);
    let expand = ctx.eval_flag(args, &matched, 2, call, true)?;
    let env = ctx.eval_arg(
        args,
        &matched,
        3,
        call,
        frame.as_ref().map_or(call.env, |frame| frame.caller),
    )?;
    if ctx.session.type_of(call_expr)? != SexpType::Lang {
        return Err(ctx.stop(call.err_call(), "invalid 'call' argument"));
    }
    let (head, tail, _) = ctx.session.cons_parts(call_expr)?;
    if definition == SEXP_NIL {
        definition = match frame.as_ref() {
            Some(frame)
                if frame.fun != SEXP_NIL && ctx.matched_arg(args, &matched, 1).is_none() =>
            {
                frame.fun
            }
            _ => ctx.eval(head, env)?,
        };
    }
    if ctx.session.type_of(definition)? != SexpType::Clo {
        return Err(ctx.stop(call.err_call(), "invalid 'definition' argument"));
    }
    ctx.protect(definition);
    let mut actuals = Vec::new();
    let raw = Args::from_pairs(ctx.session.pairlist_iter(tail).collect());
    for (tag, expr) in raw.iter() {
        if ctx.session.sym_name_of(expr).ok() == Some("...") {
            let dots = ctx.dots(env, call.err_call())?;
            let entries: Vec<_> = ctx.session.pairlist_iter(dots).collect();
            for (i, (tag, value)) in entries.into_iter().enumerate() {
                let expr = ctx.promise_expr(value)?;
                let expr = if matches!(
                    ctx.session.type_of(expr)?,
                    SexpType::Sym | SexpType::Lang | SexpType::Nil
                ) {
                    ctx.session.intern(&format!("..{}", i + 1))?.as_sexp()
                } else {
                    expr
                };
                actuals.push((tag, expr));
            }
        } else {
            actuals.push((tag, expr));
        }
    }
    let actuals = Args::from_pairs(actuals);
    let (formals, _, _) = ctx.session.closure_parts(definition)?;
    let formals = Args::from_pairs(ctx.session.pairlist_iter(formals).collect());
    let slots = ctx.match_args(&formals, &actuals, call_expr, false)?;
    let dots = ctx.session.intern("...")?;
    let mut pairs = Vec::new();
    for (slot, (tag, _)) in formals.iter().enumerate() {
        if tag == Some(dots) && !slots.dots.is_empty() {
            let expanded = slots
                .dots
                .iter()
                .map(|&i| Ok((actuals.name(i), ctx.promise_expr(actuals.get(i))?)))
                .collect::<EvalResult<Vec<_>>>()?;
            if expand {
                pairs.extend(expanded);
            } else {
                let pairlist = ctx.pairlist(&Args::from_pairs(expanded), false)?;
                pairs.push((tag, pairlist));
            }
        } else if let Some(i) = slots.slots[slot] {
            let expr = ctx.promise_expr(actuals.get(i))?;
            if expr != SEXP_MISSING {
                pairs.push((tag, expr));
            }
        }
    }
    ctx.make_call(head, &Args::from_pairs(pairs))
}

pub(super) fn match_arg(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["arg", "choices", "several.ok"], call.err_call())?;
    let value = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let choices = if let Some(choices) = ctx.matched_arg(args, &matched, 1) {
        choices
    } else {
        let expr = ctx.session.cons_parts(call.expr)?.1;
        let raw = Args::from_pairs(ctx.session.pairlist_iter(expr).collect());
        let raw = ctx.matched_arg(&raw, &matched, 0).unwrap_or(SEXP_MISSING);
        let Ok(sym) = ctx.sym_from_sexp(raw) else {
            return Err(ctx.stop(
                call.err_call(),
                "argument \"choices\" is missing, with no default",
            ));
        };
        let Some(frame) = ctx.frame(call.env) else {
            return Err(ctx.stop(
                call.err_call(),
                "argument \"choices\" is missing, with no default",
            ));
        };
        let (formals, _, _) = ctx.session.closure_parts(frame.fun)?;
        let default = ctx
            .session
            .pairlist_iter(formals)
            .find_map(|(tag, value)| (tag == Some(sym)).then_some(value))
            .unwrap_or(SEXP_MISSING);
        ctx.eval(default, call.env)?
    };
    ctx.protect(choices);
    let several = match ctx.matched_arg(args, &matched, 2) {
        Some(several) => ctx.truth(several, call.err_call())?,
        None => false,
    };
    if ctx.session.type_of(choices)? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "'choices' must be a character vector"));
    }
    let choices = ctx.session.str_slice(choices)?.to_vec();
    let values = if value == SEXP_NIL {
        Vec::new()
    } else if ctx.session.type_of(value)? == SexpType::Str {
        ctx.session.str_slice(value)?.to_vec()
    } else {
        return Err(ctx.stop(call.err_call(), "'arg' must be NULL or a character vector"));
    };
    if value == SEXP_NIL || (!several && values == choices) {
        return Ok(ctx
            .session
            .alloc_str_from(&choices[..choices.len().min(1)])?);
    }
    if !several && values.len() != 1 {
        return Err(ctx.stop(call.err_call(), "'arg' must be of length 1"));
    }
    if several && values.is_empty() {
        return Err(ctx.stop(call.err_call(), "'arg' must be of length >= 1"));
    }
    let mut selected = Vec::new();
    for value in values {
        if choices.contains(&value) {
            selected.push(value);
        } else if value != CharId::NA {
            let prefix = ctx.session.char_bytes(value)?;
            let mut matches = choices.iter().filter(|&&id| {
                !prefix.is_empty()
                    && ctx
                        .session
                        .char_bytes(id)
                        .is_ok_and(|choice| choice.starts_with(prefix))
            });
            if let Some(&exact) = matches.next()
                && matches.next().is_none()
            {
                selected.push(exact);
            }
        }
    }
    if selected.is_empty() {
        let labels = choices
            .iter()
            .map(|&id| {
                Ok(format!(
                    "“{}”",
                    String::from_utf8_lossy(ctx.session.char_bytes(id)?)
                ))
            })
            .collect::<EvalResult<Vec<_>>>()?;
        return Err(ctx.stop(
            call.err_call(),
            format!("'arg' should be one of {}", labels.join(", ")),
        ));
    }
    Ok(ctx.session.alloc_str_from(&selected)?)
}

pub(super) fn nargs(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    let Some(frame) = ctx.frame(call.env).filter(|frame| frame.fun != SEXP_NIL) else {
        return Ok(SEXP_NIL);
    };
    Ok(ctx.session.scalar_int(frame.actuals.len() as i32)?)
}

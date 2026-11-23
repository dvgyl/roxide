use roxide_gc::SexpId;
use roxide_value::{Binding, PromiseState, SEXP_MISSING, SEXP_NIL, SexpType, Sym, ValueApi};

use crate::ctx::{Frame, FrameId};
use crate::dispatch::MethodFrame;
use crate::{Args, Ctx, EvalResult, Transfer, Unwind};

pub(crate) struct Matches {
    pub(crate) slots: Vec<Option<usize>>,
    pub(crate) dots: Vec<usize>,
}

pub(crate) fn dots_index(name: &str) -> Option<usize> {
    let digits = name.strip_prefix("..")?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().unwrap_or(usize::MAX))
}

impl Ctx {
    pub(crate) fn pairlist(&mut self, args: &Args, dots: bool) -> EvalResult {
        let mut tail = SEXP_NIL;
        for (i, &value) in args.values().iter().enumerate().rev() {
            let tag = args.name(i).map_or(SEXP_NIL, Sym::as_sexp);
            tail = if dots {
                self.session.dcons(value, tail, tag)?
            } else {
                self.session.cons(value, tail, tag)?
            };
        }
        Ok(tail)
    }

    pub(crate) fn make_call(&mut self, head: SexpId, args: &Args) -> EvalResult {
        let tail = self.pairlist(args, false)?;
        Ok(self.session.lcons(head, tail, SEXP_NIL)?)
    }

    pub(crate) fn frame(&self, env: SexpId) -> Option<&Frame> {
        self.frames.iter().rfind(|frame| frame.cloenv == env)
    }

    pub(crate) fn find_dots(&mut self, env: SexpId) -> EvalResult<Option<SexpId>> {
        let sym = self.session.intern("...")?;
        let mut cursor = env;
        while cursor != SEXP_NIL {
            if let Some(binding) = self.session.env_get(cursor, sym) {
                if binding.value == SEXP_MISSING
                    || self.session.type_of(binding.value)? == SexpType::Dot
                {
                    return Ok(Some(binding.value));
                }
                break;
            }
            cursor = self.session.env_enclos(cursor)?;
        }
        Ok(None)
    }

    pub(crate) fn dots(&mut self, env: SexpId, call: SexpId) -> EvalResult {
        self.find_dots(env)?
            .ok_or_else(|| self.stop(call, "'...' used in an incorrect context"))
    }

    pub(crate) fn dots_elt(&mut self, env: SexpId, index: usize, call: SexpId) -> EvalResult {
        let Some(dots) = self.find_dots(env)? else {
            return Err(self.stop(
                call,
                format!("..{index} used in an incorrect context, no ... to look in"),
            ));
        };
        if index == 0 {
            return Err(self.stop(call, "attempt to select less than one element in get1index"));
        }
        if let Some((_, value)) = self.session.pairlist_iter(dots).nth(index - 1) {
            return Ok(value);
        }
        Err(self.stop(
            call,
            format!("the ... list contains fewer than {index} elements"),
        ))
    }

    pub(crate) fn value_promise(&mut self, expr: SexpId, value: SexpId) -> EvalResult {
        if value == SEXP_MISSING {
            return Ok(value);
        }
        let promise = self.session.alloc_promise(expr, SEXP_NIL)?;
        self.session
            .set_promise_state(promise, PromiseState::Forced, SEXP_NIL, value)?;
        Ok(promise)
    }

    pub(crate) fn expand_dots(
        &mut self,
        args: &Args,
        env: SexpId,
        call: SexpId,
    ) -> EvalResult<Args> {
        let mut result = Vec::new();
        for (tag, expr) in args.iter() {
            if self.session.sym_name_of(expr).ok() == Some("...") {
                let dots = self.dots(env, call)?;
                result.extend(self.session.pairlist_iter(dots));
            } else {
                result.push((tag, expr));
            }
        }
        Ok(Args::from_pairs(result))
    }

    pub(crate) fn promise_args(
        &mut self,
        args: &Args,
        env: SexpId,
        call: SexpId,
    ) -> EvalResult<Args> {
        let mut result = Vec::new();
        for (tag, expr) in args.iter() {
            if self.session.sym_name_of(expr).ok() == Some("...") {
                let dots = self.dots(env, call)?;
                result.extend(self.session.pairlist_iter(dots));
            } else {
                let value = if expr == SEXP_MISSING || self.session.type_of(expr)? == SexpType::Prom
                {
                    expr
                } else {
                    self.session.alloc_promise(expr, env)?
                };
                result.push((tag, value));
            }
        }
        Ok(Args::from_pairs(result))
    }

    pub(crate) fn promise_expr(&self, mut value: SexpId) -> EvalResult {
        while self.session.type_of(value)? == SexpType::Prom {
            value = self.session.promise_parts(value)?.0;
        }
        Ok(value)
    }

    pub(crate) fn match_args(
        &mut self,
        formals: &Args,
        actuals: &Args,
        call: SexpId,
        warn: bool,
    ) -> EvalResult<Matches> {
        let dots_symbol = self.session.intern("...")?;
        let dots_at = formals.iter().position(|(tag, _)| tag == Some(dots_symbol));
        let before_dots = dots_at.unwrap_or(formals.len());
        let mut slots = vec![None; formals.len()];
        let mut used = vec![false; actuals.len()];

        for (index, (tag, _)) in actuals.iter().enumerate() {
            let Some(tag) = tag else { continue };
            if tag == dots_symbol {
                continue;
            }
            if let Some(formal) = formals.iter().position(|(name, _)| name == Some(tag)) {
                self.match_slot(formals, call, &mut slots[formal], formal, index)?;
                used[index] = true;
            }
        }

        let exact: Vec<_> = slots.iter().map(Option::is_some).collect();
        for (index, (tag, _)) in actuals.iter().enumerate() {
            let Some(tag) = tag.filter(|_| !used[index]) else {
                continue;
            };
            let name = self.session.sym_name(tag)?.to_owned();
            let mut candidates =
                formals
                    .iter()
                    .take(before_dots)
                    .enumerate()
                    .filter_map(|(i, (tag, _))| {
                        let formal = self.session.sym_name(tag?).ok()?;
                        (!exact[i] && formal.starts_with(&name)).then_some(i)
                    });
            if let Some(formal) = candidates.next() {
                if candidates.next().is_some() {
                    return Err(self.stop(
                        call,
                        format!("argument {} matches multiple formal arguments", index + 1),
                    ));
                }
                self.match_slot(formals, call, &mut slots[formal], formal, index)?;
                used[index] = true;
                if warn
                    && self.opts.warn_partial_match_args
                    && let Some(tag) = formals.name(formal)
                {
                    let full = self.session.sym_name(tag)?;
                    self.warn_now(
                        call,
                        format!("partial argument match of '{name}' to '{full}'"),
                    )?;
                }
            }
        }

        let mut positional = actuals
            .iter()
            .enumerate()
            .filter_map(|(i, (tag, _))| (tag.is_none() && !used[i]).then_some(i));
        for slot in &mut slots[..before_dots] {
            if slot.is_none_or(|i| actuals.get(i) == SEXP_MISSING)
                && let Some(i) = positional.next()
            {
                *slot = Some(i);
            }
        }
        for i in slots.iter().flatten() {
            used[*i] = true;
        }
        let dots: Vec<_> = used
            .iter()
            .enumerate()
            .filter_map(|(i, used)| (!used).then_some(i))
            .collect();
        if dots_at.is_none() && !dots.is_empty() {
            let mut unused = Vec::new();
            for &i in &dots {
                let expr = self.promise_expr(actuals.get(i))?;
                let mut value = if expr == SEXP_MISSING {
                    String::new()
                } else {
                    self.deparse_lines(expr, Default::default())?.join(" ")
                };
                if let Some(tag) = actuals.name(i) {
                    value = format!("{} = {value}", self.session.sym_name(tag)?);
                }
                unused.push(value);
            }
            let arguments = if dots.len() == 1 {
                "argument"
            } else {
                "arguments"
            };
            return Err(self.stop(call, format!("unused {arguments} ({})", unused.join(", "))));
        }
        Ok(Matches { slots, dots })
    }

    fn match_slot(
        &mut self,
        formals: &Args,
        call: SexpId,
        slot: &mut Option<usize>,
        formal: usize,
        index: usize,
    ) -> EvalResult<()> {
        if slot.is_some() {
            let name = formals
                .name(formal)
                .and_then(|tag| self.session.sym_name(tag).ok())
                .unwrap_or("");
            return Err(self.stop(
                call,
                format!("formal argument \"{name}\" matched by multiple actual arguments"),
            ));
        }
        *slot = Some(index);
        Ok(())
    }

    pub(crate) fn invoke_closure(
        &mut self,
        fun: SexpId,
        actuals: Args,
        call: SexpId,
        caller: SexpId,
        dispatch: Option<MethodFrame>,
    ) -> EvalResult {
        self.scoped(|ctx| {
            for root in [fun, call, caller] {
                ctx.protect(root);
            }
            for &value in actuals.values() {
                ctx.protect(value);
            }
            if let Some(method) = &dispatch {
                method.protect(ctx);
            }
            let (formals, body, enclos) = ctx.session.closure_parts(fun)?;
            let formals = Args::from_pairs(ctx.session.pairlist_iter(formals).collect());
            let matched = ctx.match_args(&formals, &actuals, call, true)?;
            let env = ctx.session.alloc_env(enclos)?;
            ctx.protect(env);
            let mut supplied_formals = vec![false; formals.len()];
            for (i, (tag, default)) in formals.iter().enumerate() {
                let Some(tag) = tag else { continue };
                let supplied = matched.slots[i].map_or(SEXP_MISSING, |i| actuals.get(i));
                let (value, missing) = if ctx.session.sym_name(tag)? == "..." {
                    if matched.dots.is_empty() {
                        (SEXP_MISSING, 1)
                    } else {
                        let pairs = matched
                            .dots
                            .iter()
                            .map(|&i| (actuals.name(i), actuals.get(i)))
                            .collect();
                        (ctx.pairlist(&Args::from_pairs(pairs), true)?, 0)
                    }
                } else if supplied != SEXP_MISSING {
                    (supplied, 0)
                } else if default == SEXP_MISSING {
                    (SEXP_MISSING, 1)
                } else {
                    (ctx.session.alloc_promise(default, env)?, 1)
                };
                supplied_formals[i] = missing == 0;
                ctx.session.env_define_binding(
                    env,
                    tag,
                    Binding {
                        value,
                        missing,
                        locked: false,
                        active: false,
                    },
                )?;
            }

            if let Some(method) = &dispatch {
                method.bind(ctx, env)?;
            }
            let id = ctx.next_frame();
            let frame = Frame {
                id,
                call,
                cloenv: env,
                on_exit: Vec::new(),
                fun,
                caller,
                actuals,
                supplied: supplied_formals,
                dispatch,
            };
            let actuals = frame.actuals.values().to_vec();
            for &value in &actuals {
                ctx.session.heap.inc_ref(value);
            }
            let result = ctx.run_frame(frame, |ctx| ctx.eval(body, env));
            for &value in &actuals {
                ctx.session.heap.dec_ref(value);
            }
            ctx.session
                .release_call_frame(env, &actuals, &result_roots(&result));
            result
        })
    }

    /// Read the active bindings of a generic call `frame` for forwarding to [`Ctx::apply_matched`].
    ///
    /// Read current values only for formals supplied at entry. Keep omitted formals missing.
    pub fn generic_frame_args(&mut self, frame: FrameId) -> EvalResult<Vec<(Sym, SexpId)>> {
        let Some(frame) = self.frames.iter().find(|entry| entry.id == frame) else {
            return Err(self.stop(SEXP_NIL, "unknown call frame"));
        };
        let (formals, _, _) = self.session.closure_parts(frame.fun)?;
        Ok(self
            .session
            .pairlist_iter(formals)
            .zip(&frame.supplied)
            .filter_map(|((tag, _), &supplied)| {
                let tag = tag?;
                let value = if supplied {
                    self.session
                        .env_get(frame.cloenv, tag)
                        .map_or(SEXP_MISSING, |binding| binding.value)
                } else {
                    SEXP_MISSING
                };
                Some((tag, value))
            })
            .collect())
    }

    /// Invoke closure `fun` with pre-matched formal bindings (preserving promises and
    /// `SEXP_MISSING` markers) plus extra frame variables (`.Generic`, `.Method`, `.defined`, `.target`).
    pub fn apply_matched(
        &mut self,
        fun: SexpId,
        matched: &[(Sym, SexpId)],
        call: SexpId,
        caller: SexpId,
        frame_vars: &[(Sym, SexpId)],
    ) -> EvalResult {
        self.scoped(|ctx| {
            for root in [fun, call, caller] {
                ctx.protect(root);
            }
            for &(_, value) in matched {
                ctx.protect(value);
            }
            for &(_, value) in frame_vars {
                ctx.protect(value);
            }
            let (formals, body, enclos) = ctx.session.closure_parts(fun)?;
            let formals = Args::from_pairs(ctx.session.pairlist_iter(formals).collect());
            let env = ctx.session.alloc_env(enclos)?;
            ctx.protect(env);
            let actual_pairs: Vec<_> = matched
                .iter()
                .filter(|&&(_, v)| v != SEXP_MISSING)
                .map(|&(sym, val)| (Some(sym), val))
                .collect();
            let actuals = Args::from_pairs(actual_pairs);

            let mut supplied_formals = vec![false; formals.len()];
            for (i, (tag, default)) in formals.iter().enumerate() {
                let Some(tag) = tag else { continue };
                let supplied = matched.iter().find(|&&(k, _)| k == tag).map(|&(_, v)| v);
                let (value, missing) = if ctx.session.sym_name(tag)? == "..." {
                    match supplied {
                        Some(v) if v != SEXP_MISSING => (v, 0),
                        _ => (SEXP_MISSING, 1),
                    }
                } else {
                    match supplied {
                        Some(v) if v != SEXP_MISSING => (v, 0),
                        _ if default == SEXP_MISSING => (SEXP_MISSING, 1),
                        _ => (ctx.session.alloc_promise(default, env)?, 1),
                    }
                };
                supplied_formals[i] = missing == 0;
                ctx.session.env_define_binding(
                    env,
                    tag,
                    Binding {
                        value,
                        missing,
                        locked: false,
                        active: false,
                    },
                )?;
            }

            for &(sym, val) in frame_vars {
                ctx.session.env_define_binding(
                    env,
                    sym,
                    Binding {
                        value: val,
                        missing: 0,
                        locked: false,
                        active: false,
                    },
                )?;
            }

            let id = ctx.next_frame();
            let frame = Frame {
                id,
                call,
                cloenv: env,
                on_exit: Vec::new(),
                fun,
                caller,
                actuals,
                supplied: supplied_formals,
                dispatch: None,
            };
            let actual_vals = frame.actuals.values().to_vec();
            for &value in &actual_vals {
                ctx.session.heap.inc_ref(value);
            }
            let result = ctx.run_frame(frame, |ctx| ctx.eval(body, env));
            for &value in &actual_vals {
                ctx.session.heap.dec_ref(value);
            }
            ctx.session
                .release_call_frame(env, &actual_vals, &result_roots(&result));
            result
        })
    }

    pub(crate) fn run_frame(
        &mut self,
        frame: Frame,
        run: impl FnOnce(&mut Ctx) -> EvalResult,
    ) -> EvalResult {
        let (id, env) = (frame.id, frame.cloenv);
        self.frames.push(frame);
        let call = self.frames.last().map_or(SEXP_NIL, |frame| frame.call);
        let result = self.with_call(call, env, |ctx| {
            let result = run(ctx);
            ctx.finish_closure(env, result)
        });
        self.frames.pop();
        match result {
            Err(Unwind::Transfer {
                target,
                kind: Transfer::Return(value),
            }) if target == id => Ok(value),
            other => other,
        }
    }

    fn finish_closure(&mut self, env: SexpId, mut result: EvalResult) -> EvalResult {
        let mut visible = self.visible;
        let handlers = self
            .frames
            .last_mut()
            .map(|frame| std::mem::take(&mut frame.on_exit))
            .unwrap_or_default();
        for &expr in &handlers {
            self.protect(expr);
        }
        for expr in handlers {
            self.protect_result(&result);
            if let Err(err) = self.eval(expr, env) {
                visible = self.visible;
                result = Err(err);
            }
        }
        self.visible = visible;
        result
    }

    pub(crate) fn protect_result(&mut self, result: &EvalResult) {
        match result {
            Ok(value)
            | Err(Unwind::Transfer {
                kind: Transfer::Return(value),
                ..
            })
            | Err(Unwind::Transfer {
                kind: Transfer::ExitingHandler { cond: value, .. },
                ..
            }) => {
                self.protect(*value);
            }
            Err(Unwind::Transfer {
                kind: Transfer::Restart { restart, args },
                ..
            }) => {
                self.protect(*restart);
                self.protect(*args);
            }
            _ => {}
        }
    }
}

fn result_roots(result: &EvalResult) -> Vec<SexpId> {
    match result {
        Ok(value)
        | Err(Unwind::Transfer {
            kind: Transfer::Return(value),
            ..
        })
        | Err(Unwind::Transfer {
            kind: Transfer::ExitingHandler { cond: value, .. },
            ..
        }) => vec![*value],
        Err(Unwind::Transfer {
            kind: Transfer::Restart { restart, args },
            ..
        }) => vec![*restart, *args],
        _ => Vec::new(),
    }
}

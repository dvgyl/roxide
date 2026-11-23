use roxide_gc::SexpId;
use roxide_value::{RLogical, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi, ValueError};

use crate::eval::ArgInput;
use crate::{ArgEval, Args, Arity, Call, Ctx, EvalResult, Module, Transfer, Unwind};

pub(crate) const MODULE: Module = Module {
    prims: &[
        primitive!("<<-", assign, Unevaluated, Arity::Exact(2), Off),
        primitive!("if", if_expr, Unevaluated, Arity::Range(2, 3), SetByFn),
        primitive!("for", for_loop, Unevaluated, Arity::Exact(3), Off),
        primitive!("while", while_loop, Unevaluated, Arity::Exact(2), Off),
        primitive!("repeat", repeat_loop, Unevaluated, Arity::Exact(1), Off),
        primitive!("break", loop_jump, Unevaluated, Arity::Exact(0), Off),
        primitive!("next", loop_jump, Unevaluated, Arity::Exact(0), Off),
        primitive!(
            "return",
            return_expr,
            Unevaluated,
            Arity::Range(0, 1),
            SetByFn
        ),
        primitive!("on.exit", on_exit, Unevaluated, Arity::Range(0, 3), Off),
    ],
    prelude: "",
};

pub(crate) fn assign(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut lhs = args.get(0);
    let rhs_expr = args.get(1);
    let rhs = ctx.eval(rhs_expr, call.env)?;
    ctx.protect(rhs);
    let super_assign = ctx.prims[usize::from(call.prim.0)].name == "<<-";
    let mut layers = Vec::new();
    while ctx.session.type_of(lhs)? == SexpType::Lang {
        let (head, tail, _) = ctx.session.cons_parts(lhs)?;
        if tail == SEXP_NIL {
            return Err(ctx.stop(call.err_call(), "invalid function in complex assignment"));
        }
        let args = Args::from_pairs(ctx.session.pairlist_iter(tail).collect());
        layers.push((lhs, head, args));
        lhs = layers.last().map_or(SEXP_NIL, |(_, _, args)| args.get(0));
    }
    let sym = match ctx.session.type_of(lhs)? {
        SexpType::Sym if lhs != SEXP_MISSING => ctx.sym_from_sexp(lhs)?,
        SexpType::Str => {
            let name = ctx.string(lhs, call.err_call())?;
            ctx.session.intern(&name)?
        }
        _ => {
            return Err(ctx.stop(
                call.err_call(),
                "invalid (do_set) left-hand side to assignment",
            ));
        }
    };
    let dest = if super_assign {
        ctx.super_env(sym, call.env)?
    } else {
        call.env
    };
    if layers.is_empty() {
        ctx.assign(sym, rhs, dest)?;
        return Ok(rhs);
    }

    ctx.session.heap.inc_ref(rhs);
    let result = assign_replacement(ctx, call, layers, sym, dest, rhs_expr, rhs);
    ctx.session.heap.dec_ref(rhs);
    result
}

fn assign_replacement(
    ctx: &mut Ctx,
    call: &Call,
    mut layers: Vec<(SexpId, SexpId, Args)>,
    sym: roxide_value::Sym,
    dest: SexpId,
    rhs_expr: SexpId,
    rhs: SexpId,
) -> EvalResult {
    let mut target = ctx.eval(sym.as_sexp(), dest)?;
    if ctx.session.env_get(dest, sym).is_none() {
        target = ctx.session.shallow_duplicate(target)?;
    }
    let mut replacements = Vec::with_capacity(layers.len());
    let mut shared_parent = false;
    while let Some((source, head, args)) = layers.pop() {
        ctx.protect(target);
        if shared_parent {
            target = ctx.session.shallow_duplicate(target)?;
            ctx.protect(target);
        }
        if !layers.is_empty() {
            let next = invoke_assignment(ctx, head, source, call.env, &args, target, None)?;
            shared_parent |= ctx.session.heap.maybe_shared(target);
            replacements.push((head, args, target));
            target = next;
        } else {
            replacements.push((head, args, target));
        }
    }

    let value_tag = ctx.session.intern("value")?;
    let mut value = rhs;
    let mut original = rhs_expr;
    let simple = replacements.len() == 1
        && ctx
            .session
            .env_get(dest, sym)
            .is_some_and(|binding| !binding.active && !binding.locked);
    for (head, args, target) in replacements.into_iter().rev() {
        ctx.protect(value);
        let replacement = replacement_head(ctx, head, call.err_call())?;
        let raw = Args::from_pairs(args.iter().chain([(Some(value_tag), original)]).collect());
        let expr = ctx.make_call(replacement, &raw)?;
        ctx.protect(expr);
        let previous = std::mem::replace(&mut ctx.replacement_call, simple.then_some(expr));
        let result = invoke_assignment(ctx, replacement, expr, call.env, &raw, target, Some(value));
        ctx.replacement_call = previous;
        value = result?;
        original = expr;
    }
    ctx.assign(sym, value, dest)?;
    Ok(rhs)
}

fn invoke_assignment(
    ctx: &mut Ctx,
    head: SexpId,
    expr: SexpId,
    env: SexpId,
    args: &Args,
    target: SexpId,
    replacement: Option<SexpId>,
) -> EvalResult {
    let fun = if ctx.session.type_of(head)? == SexpType::Sym {
        let sym = ctx.sym_from_sexp(head)?;
        ctx.find_fun_at(sym, env, expr)?
    } else {
        ctx.eval(head, env)?
    };
    ctx.protect(fun);
    if let Ok((prim, _)) = ctx.session.prim_parts(fun) {
        let policy = ctx.prims[usize::from(prim.0)].args;
        if policy != ArgEval::Unevaluated {
            let mut values = vec![(args.name(0), target)];
            let end = args.len() - usize::from(replacement.is_some());
            let middle = Args::from_pairs(args.iter().skip(1).take(end - 1).collect());
            let middle = if policy == ArgEval::EvalDollar {
                middle
            } else {
                ctx.expand_dots(&middle, env, expr)?
            };
            for &arg in middle.values() {
                ctx.protect(arg);
            }
            ctx.session.heap.inc_ref(target);
            let result = (|| {
                for (tag, value) in middle.iter() {
                    let value = if policy == ArgEval::EvalDollar {
                        value
                    } else if value == SEXP_MISSING {
                        if policy != ArgEval::EvalKeepMissing {
                            return Err(ctx.stop(expr, "argument is missing, with no default"));
                        }
                        value
                    } else {
                        ctx.eval(value, env)?
                    };
                    ctx.protect(value);
                    values.push((tag, value));
                }
                Ok(())
            })();
            ctx.session.heap.dec_ref(target);
            result?;
            if let Some(value) = replacement {
                values.push((args.name(end), value));
            }
            let call = Call {
                expr,
                env,
                prim,
                user_call: expr,
            };
            let source = Args::from_pairs(
                args.iter()
                    .take(1)
                    .chain(middle.iter())
                    .chain(args.iter().skip(end))
                    .collect(),
            );
            return ctx.run_primitive(
                call,
                Args::from_pairs(values),
                ArgInput::Values(Some(&source)),
                true,
            );
        }
    }
    let mut raw: Vec<_> = args.iter().collect();
    raw[0].1 = ctx.value_promise(raw[0].1, target)?;
    if let Some(value) = replacement
        && let Some((_, last)) = raw.last_mut()
    {
        *last = ctx.value_promise(*last, value)?;
    }
    let tail = ctx.pairlist(&Args::from_pairs(raw), false)?;
    ctx.invoke_raw(fun, tail, expr, env)
}

fn replacement_head(ctx: &mut Ctx, head: SexpId, call: SexpId) -> EvalResult {
    if let Ok(name) = ctx.session.sym_name_of(head) {
        let replacement = format!("{name}<-");
        return Ok(ctx.session.intern(&replacement)?.as_sexp());
    }
    if ctx.session.type_of(head)? == SexpType::Lang {
        let (op, tail, _) = ctx.session.cons_parts(head)?;
        if matches!(ctx.session.sym_name_of(op), Ok("::" | ":::")) {
            let args = Args::from_pairs(ctx.session.pairlist_iter(tail).collect());
            let name = ctx.name(args.get(1), call)?;
            let replacement = ctx.session.intern(&format!("{name}<-"))?.as_sexp();
            return ctx.make_call(
                op,
                &Args::from_pairs(vec![(None, args.get(0)), (None, replacement)]),
            );
        }
    }
    Err(ctx.stop(call, "invalid function in complex assignment"))
}

impl Ctx {
    pub(crate) fn truth(&mut self, value: SexpId, call: SexpId) -> EvalResult<bool> {
        match self.session.xlength(value)? {
            0 => return Err(self.stop(call, "argument is of length zero")),
            1 => {}
            _ => return Err(self.stop(call, "the condition has length > 1")),
        }
        if !matches!(
            self.session.type_of(value)?,
            SexpType::Lgl
                | SexpType::Int
                | SexpType::Real
                | SexpType::Cplx
                | SexpType::Str
                | SexpType::Raw
        ) {
            return Err(self.stop(call, "argument is not interpretable as logical"));
        }
        let Ok((logical, _)) = self.session.coerce(value, SexpType::Lgl) else {
            return Err(self.stop(call, "argument is not interpretable as logical"));
        };
        let truth = self.session.lgl(logical)?.get(0).get();
        truth.ok_or_else(|| {
            let text = if self.session.type_of(value).ok() == Some(SexpType::Str) {
                "argument is not interpretable as logical"
            } else {
                "missing value where TRUE/FALSE needed"
            };
            self.stop(call, text)
        })
    }

    pub(crate) fn element(&mut self, x: SexpId, i: usize) -> EvalResult {
        Ok(match self.session.type_of(x)? {
            SexpType::Vec | SexpType::Expr => self.session.list_get(x, i)?,
            SexpType::Int => self.session.scalar_int(self.session.int(x)?.get(i))?,
            SexpType::Real => self.session.scalar_real(self.session.real(x)?.get(i))?,
            SexpType::Lgl => self.session.scalar_lgl(self.session.lgl(x)?.get(i))?,
            SexpType::Cplx => self
                .session
                .alloc_cplx_from(&[self.session.cplx(x)?.get(i)])?,
            SexpType::Raw => self
                .session
                .alloc_raw_from(&[self.session.raw(x)?.get(i)])?,
            SexpType::Str => {
                let chars = self.session.str_slice(x)?;
                let value = chars.get(i).copied().ok_or(ValueError::IndexOutOfBounds {
                    index: i,
                    len: chars.len(),
                })?;
                self.session.alloc_str_from(&[value])?
            }
            _ => return Err(self.stop(SEXP_NIL, "invalid for() loop sequence")),
        })
    }
}

fn if_expr(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = ctx.eval(args.get(0), call.env)?;
    if ctx.truth(value, call.err_call())? {
        ctx.eval(args.get(1), call.env)
    } else if args.len() == 3 {
        ctx.eval(args.get(2), call.env)
    } else {
        ctx.visible = false;
        Ok(SEXP_NIL)
    }
}

fn for_loop(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let var = ctx.sym_from_sexp(args.get(0))?;
    let mut sequence = ctx.eval(args.get(1), call.env)?;
    ctx.protect(sequence);
    if ctx
        .classes(sequence, false)?
        .iter()
        .any(|class| class == "factor")
    {
        ctx.check_vector_len(
            ctx.session.xlength(sequence)?,
            size_of::<roxide_value::CharId>(),
            call.err_call(),
        )?;
        let levels = ctx.session.intern("levels")?;
        let levels = ctx.session.attr(sequence, levels);
        let labels = ctx.session.str_slice(levels)?;
        let values: Vec<_> = ctx
            .session
            .int(sequence)?
            .iter()
            .map(|i| {
                labels
                    .get(i.saturating_sub(1) as usize)
                    .copied()
                    .unwrap_or(roxide_value::CharId::NA)
            })
            .collect();
        sequence = ctx.session.alloc_str_from(&values)?;
        ctx.protect(sequence);
    }
    let ty = ctx.session.type_of(sequence)?;
    let mut tail = if ty == SexpType::List {
        sequence
    } else {
        SEXP_NIL
    };
    let len = match ty {
        SexpType::List | SexpType::Nil => 0,
        SexpType::Vec
        | SexpType::Expr
        | SexpType::Int
        | SexpType::Real
        | SexpType::Lgl
        | SexpType::Cplx
        | SexpType::Raw
        | SexpType::Str => {
            sequence = ctx.session.shallow_duplicate(sequence)?;
            ctx.protect(sequence);
            ctx.session.xlength(sequence)?
        }
        _ => return Err(ctx.stop(call.err_call(), "invalid for() loop sequence")),
    };
    ctx.assign(var, SEXP_NIL, call.env)?;
    let mut indices = 0..len;
    run_loop(ctx, call, args.get(2), |ctx| {
        let value = if tail != SEXP_NIL {
            let (value, next, _) = ctx.session.cons_parts(tail)?;
            tail = next;
            value
        } else if let Some(i) = indices.next() {
            ctx.element(sequence, i)?
        } else {
            return Ok(false);
        };
        ctx.assign(var, value, call.env)?;
        Ok(true)
    })
}

fn while_loop(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    run_loop(ctx, call, args.get(1), |ctx| {
        let value = ctx.eval(args.get(0), call.env)?;
        ctx.truth(value, call.err_call())
    })
}

fn repeat_loop(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    run_loop(ctx, call, args.get(0), |_| Ok(true))
}

fn run_loop(
    ctx: &mut Ctx,
    call: &Call,
    body: SexpId,
    mut step: impl FnMut(&mut Ctx) -> EvalResult<bool>,
) -> EvalResult {
    let id = ctx.next_frame();
    ctx.loops.push((id, call.env));
    let result = loop {
        match step(ctx).and_then(|run| {
            if run {
                ctx.eval(body, call.env).map(|_| true)
            } else {
                Ok(false)
            }
        }) {
            Ok(true) => {}
            Ok(false) => break Ok(SEXP_NIL),
            Err(Unwind::Transfer {
                target,
                kind: Transfer::Next,
            }) if target == id => {}
            Err(Unwind::Transfer {
                target,
                kind: Transfer::Break,
            }) if target == id => break Ok(SEXP_NIL),
            Err(err) => break Err(err),
        }
    };
    ctx.loops.pop();
    result
}

fn loop_jump(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    let Some(&(target, _)) = ctx.loops.iter().rfind(|(_, env)| *env == call.env) else {
        return Err(ctx.stop(SEXP_NIL, "no loop for break/next, jumping to top level"));
    };
    let kind = if ctx.prims[usize::from(call.prim.0)].name == "break" {
        Transfer::Break
    } else {
        Transfer::Next
    };
    Err(Unwind::Transfer { target, kind })
}

fn return_expr(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let Some(target) = ctx.frame(call.env).map(|frame| frame.id) else {
        return Err(ctx.stop(SEXP_NIL, "no function to return from, jumping to top level"));
    };
    let value = ctx.eval(args.get(0), call.env)?;
    Err(Unwind::Transfer {
        target,
        kind: Transfer::Return(value),
    })
}

fn on_exit(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let Some(target) = ctx.frame(call.env).map(|frame| frame.id) else {
        return Ok(SEXP_NIL);
    };
    let matched = ctx.match_names(args, &["expr", "add", "after"], call.err_call())?;
    let expr = matched.slots[0].map_or(SEXP_MISSING, |i| args.get(i));
    let add = ctx.eval_flag(args, &matched, 1, call, false)?;
    let after = ctx.eval_flag(args, &matched, 2, call, true)?;
    let Some(frame) = ctx.frames.iter_mut().find(|frame| frame.id == target) else {
        return Ok(SEXP_NIL);
    };
    if !add {
        frame.on_exit.clear();
    }
    if expr != SEXP_MISSING {
        if after {
            frame.on_exit.push(expr);
        } else {
            frame.on_exit.insert(0, expr);
        }
    }
    Ok(SEXP_NIL)
}

pub(crate) fn logical(value: bool) -> RLogical {
    RLogical::from_option(Some(value))
}

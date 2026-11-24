//! Execution of Roxide register bytecode.

use std::cell::RefCell;

use roxide_gc::SexpId;
use roxide_value::{SEXP_MISSING, SEXP_NIL, Sym, ValueApi};

use crate::eval::ArgInput;
use crate::{ArgEval, Args, Call, Ctx, EvalResult, InternalError, Transfer, Unwind};

pub use super::jit::run_body;

use super::cache::Caches;
use super::ir::{Binary, CallSite, CompiledCode, Op, Reg, VERSION, VarSite};
use super::stack::{ForCursor, GuardFrame, LoopFrame, VmFrame};
use super::{compile_expr, guard, scalar, subscript};

/// Execute compiled code in `env` and preserve the result's R visibility.
pub fn run_compiled(ctx: &mut Ctx, code: &CompiledCode, env: SexpId) -> EvalResult {
    execute(ctx, code, env, &RefCell::new(Caches::new(code)), None)
}

pub(super) fn execute(
    ctx: &mut Ctx,
    code: &CompiledCode,
    env: SexpId,
    caches: &RefCell<Caches>,
    entry: Option<(SexpId, Call)>,
) -> EvalResult {
    if code.version != VERSION {
        return Err(invalid());
    }
    let vm = Execution {
        code,
        env,
        caches,
        entry,
    };
    let previous_env = std::mem::replace(&mut ctx.eval_env, env);
    let loop_depth = ctx.loops.len();
    let expr_depth = ctx.expr_depth;
    ctx.vm_stack.frames.push(VmFrame::new(code, env));
    let result = vm.run(ctx);
    let result = ctx.condition_result(entry.map_or(SEXP_NIL, |(_, call)| call.err_call()), result);
    if let Some(frame) = ctx.vm_stack.frames.pop() {
        for guard in frame.guards {
            if let Some(value) = guard.retained {
                ctx.session.heap.dec_ref(value);
            }
        }
    }
    ctx.loops.truncate(loop_depth);
    ctx.expr_depth = expr_depth;
    ctx.eval_env = previous_env;
    result
}

pub(crate) fn try_loop(ctx: &mut Ctx, call: &Call) -> Option<EvalResult> {
    if ctx.jit_level() < 3 || !ctx.vm_stack.frames.is_empty() || ctx.frame(call.env).is_some() {
        return None;
    }
    let (head, _, _) = ctx.session.cons_parts(call.expr).ok()?;
    let def = ctx.prims.get(usize::from(call.prim.0))?;
    if !matches!(def.name, "for" | "while" | "repeat")
        || ctx.session.sym_name_of(head).ok()? != def.name
    {
        return None;
    }
    let sym = ctx.session.sexp_as_sym(head).ok()?;
    let fun = ctx.session.env_get(roxide_value::BASE_ENV, sym)?.value;
    if ctx.session.prim_parts(fun).ok()?.0 != call.prim {
        return None;
    }
    let code = compile_expr(&mut ctx.session, call.expr).ok()?;
    if !code.has_loops {
        return None;
    }
    Some(execute(
        ctx,
        &code,
        call.env,
        &RefCell::new(Caches::new(&code)),
        Some((fun, *call)),
    ))
}

pub(super) struct Execution<'a> {
    code: &'a CompiledCode,
    env: SexpId,
    caches: &'a RefCell<Caches>,
    entry: Option<(SexpId, Call)>,
}

enum Step {
    Advance,
    Jump(usize),
    Done(SexpId),
}

impl Execution<'_> {
    fn run(&self, ctx: &mut Ctx) -> EvalResult {
        checkpoint(ctx)?;
        let mut pc = 0;
        loop {
            self.close_guards(ctx, pc)?;
            let op = self.code.code.get(pc).ok_or_else(invalid)?;
            #[cfg(test)]
            if ctx.vm_stack.collect_each_op {
                ctx.collect_gc();
            }
            let result = self.step(ctx, op, pc);
            let result = if matches!(result, Err(Unwind::Value(_)))
                && let Some(site) = frame(ctx)?.guards.last().map(|guard| guard.site)
            {
                let call = self.guarded_call(ctx, site)?;
                ctx.condition_result(call.err_call(), result)
            } else {
                result
            };
            match result {
                Ok(Step::Advance) => pc += 1,
                Ok(Step::Jump(target)) => {
                    if target <= pc {
                        checkpoint(ctx)?;
                    }
                    pc = target;
                }
                Ok(Step::Done(value)) => return Ok(value),
                Err(Unwind::Transfer { target, kind }) => {
                    let Some(frame) = frame(ctx)?.loops.last().filter(|frame| frame.id == target)
                    else {
                        return Err(Unwind::Transfer { target, kind });
                    };
                    pc = match kind {
                        Transfer::Break => frame.break_target,
                        Transfer::Next => frame.next_target,
                        _ => return Err(Unwind::Transfer { target, kind }),
                    };
                    checkpoint(ctx)?;
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn step(&self, ctx: &mut Ctx, op: &Op, pc: usize) -> EvalResult<Step> {
        match *op {
            Op::LoadConst { dst, const_idx } => {
                self.write(ctx, dst, self.constant(const_idx)?)?;
                ctx.visible = true;
            }
            Op::LoadNil(dst) => {
                self.write(ctx, dst, SEXP_NIL)?;
                ctx.visible = true;
            }
            Op::SetVisible(visible) => ctx.visible = visible,
            Op::LoadVar { dst, var } => {
                let (id, sym) = self.symbol(ctx, var)?;
                let binding = {
                    let mut caches = self.caches.borrow_mut();
                    let slot = caches
                        .vars
                        .get_mut(usize::from(var.cache_idx))
                        .ok_or_else(invalid)?;
                    guard::variable(&ctx.session, sym, self.env, slot)?
                };
                ctx.visible = true;
                let value = match binding {
                    Some((env, binding)) if binding.value != SEXP_MISSING && !binding.active => {
                        ctx.read_binding(binding, env)?
                    }
                    _ => ctx.eval(id, self.env)?,
                };
                self.write(ctx, dst, value)?;
            }
            Op::StoreVar { var, src } | Op::SuperStoreVar { var, src } => {
                let value = self.read(ctx, src)?;
                let (_, sym) = self.symbol(ctx, var)?;
                if matches!(op, Op::SuperStoreVar { .. }) {
                    ctx.assign_super(sym, value, self.env)?;
                } else {
                    ctx.assign(sym, value, self.env)?;
                }
                ctx.visible = false;
            }
            Op::GuardCall {
                site,
                dst,
                fallback,
                retain,
            } => {
                let expr = self.constant(site.call_const)?;
                let entry = self.entry.filter(|_| pc == 0);
                let (fun, counted) = if let Some((fun, _)) = entry {
                    (Some(fun), false)
                } else if ctx.expr_depth
                    < ctx.opts.expressions.min(ctx.host.limits().max_expressions)
                {
                    (self.resolve(ctx, expr, site.cache_idx)?, true)
                } else {
                    (None, false)
                };
                let end = fallback as usize;
                if let Some(fun) = fun {
                    frame(ctx)?.guards.push(GuardFrame {
                        start: pc,
                        end,
                        site,
                        fun,
                        retain,
                        retained: None,
                        counted,
                    });
                    ctx.expr_depth += usize::from(counted);
                    return Ok(Step::Advance);
                }
                let value = ctx.eval(expr, self.env)?;
                self.write(ctx, dst, value)?;
                return Ok(Step::Jump(end));
            }
            Op::Add(args)
            | Op::Sub(args)
            | Op::Mul(args)
            | Op::Div(args)
            | Op::Pow(args)
            | Op::Lt(args)
            | Op::Le(args)
            | Op::Gt(args)
            | Op::Ge(args)
            | Op::Eq(args)
            | Op::Ne(args) => self.arithmetic(ctx, op, args)?,
            Op::Pos(args) | Op::Neg(args) | Op::Not(args) => {
                let call = self.guarded_call(ctx, args.site)?;
                let src = self.read(ctx, args.src)?;
                let value = if let Some(number) = scalar::number(&ctx.session, src) {
                    ctx.visible = true;
                    scalar::unary(&mut ctx.session, op, number)?
                } else {
                    self.with_values(ctx, call, &[src])?
                };
                self.write(ctx, args.dst, value)?;
            }
            Op::Jump(target) => return Ok(Step::Jump(target as usize)),
            Op::JumpIfFalse {
                cond,
                target,
                call_const,
            }
            | Op::JumpIfTrue {
                cond,
                target,
                call_const,
            } => {
                let cond = self.read(ctx, cond)?;
                if ctx.truth(cond, self.constant(call_const)?)?
                    == matches!(op, Op::JumpIfTrue { .. })
                {
                    return Ok(Step::Jump(target as usize));
                }
            }
            Op::ShortCircuit {
                dst,
                lhs,
                decisive,
                target,
                site,
            } => {
                let lhs = self.read(ctx, lhs)?;
                let value = match scalar::short_operand(&ctx.session, lhs) {
                    Some(value) if value.get() == Some(decisive) => {
                        ctx.visible = true;
                        ctx.session.scalar_lgl(value)?
                    }
                    Some(_) => return Ok(Step::Advance),
                    None => {
                        let call = self.guarded_call(ctx, site)?;
                        self.with_values(ctx, call, &[lhs])?
                    }
                };
                self.write(ctx, dst, value)?;
                return Ok(Step::Jump(target as usize));
            }
            Op::FinishLogical { args, and } => {
                let call = self.guarded_call(ctx, args.site)?;
                let lhs = self.read(ctx, args.lhs)?;
                let rhs = self.read(ctx, args.rhs)?;
                let value = if let (Some(a), Some(b)) = (
                    scalar::short_operand(&ctx.session, lhs),
                    scalar::short_operand(&ctx.session, rhs),
                ) {
                    let result = if and { a.and(b) } else { a.or(b) };
                    ctx.visible = true;
                    ctx.session.scalar_lgl(result)?
                } else {
                    self.with_values(ctx, call, &[lhs, rhs])?
                };
                self.write(ctx, args.dst, value)?;
            }
            Op::Return(src) => return Ok(Step::Done(self.read(ctx, src)?)),
            Op::ReturnFromFunction(src) => {
                let value = self.read(ctx, src)?;
                let Some(target) = ctx.frame(self.env).map(|frame| frame.id) else {
                    return Err(
                        ctx.stop(SEXP_NIL, "no function to return from, jumping to top level")
                    );
                };
                return Err(Unwind::Transfer {
                    target,
                    kind: Transfer::Return(value),
                });
            }
            Op::ForInit {
                cursor,
                sequence,
                var,
                call_const,
            } => {
                let value = self.read(ctx, sequence)?;
                let expr = self.constant(call_const)?;
                let (_, sym) = self.symbol(ctx, var)?;
                if guard::lazy_foreign_altrep(&ctx.session, value) {
                    let active = frame(ctx)?.guards.last().ok_or_else(invalid)?;
                    let (fun, end) = (active.fun, active.end);
                    let (prim, _) = ctx.session.prim_parts(fun)?;
                    let call = Call {
                        expr,
                        env: self.env,
                        prim,
                        user_call: expr,
                    };
                    let body = subscript::arguments(ctx, expr)?.get(2);
                    let result = ctx
                        .scoped(|ctx| crate::control::for_sequence(ctx, &call, sym, body, value))?;
                    self.write(ctx, sequence, result)?;
                    ctx.visible = false;
                    return Ok(Step::Jump(end));
                }
                ctx.resolve_altrep(value)?;
                ctx.assign(sym, SEXP_NIL, self.env)?;
                let result = ForCursor::new(ctx, value, expr)?;
                *self.cursor(ctx, cursor)? = result;
            }
            Op::ForNext {
                cursor,
                var,
                target,
            } => {
                let mut state = *self.cursor(ctx, cursor)?;
                let value = state.advance(ctx)?;
                *self.cursor(ctx, cursor)? = state;
                let Some(value) = value else {
                    return Ok(Step::Jump(target as usize));
                };
                self.temp(ctx, value)?;
                let (_, sym) = self.symbol(ctx, var)?;
                ctx.assign(sym, value, self.env)?;
                self.temp(ctx, SEXP_NIL)?;
            }
            Op::LoopPush {
                break_target,
                next_target,
                cursor,
            } => {
                let id = ctx.next_frame();
                ctx.loops.push((id, self.env));
                frame(ctx)?.loops.push(LoopFrame {
                    id,
                    break_target: break_target as usize,
                    next_target: next_target as usize,
                    cursor,
                });
            }
            Op::LoopPop => {
                let loop_frame = frame(ctx)?.loops.pop().ok_or_else(invalid)?;
                if ctx.loops.pop() != Some((loop_frame.id, self.env)) {
                    return Err(invalid());
                }
                if let Some(cursor) = loop_frame.cursor {
                    *self.cursor(ctx, cursor)? = ForCursor::Empty;
                }
            }
            Op::Break | Op::Next => {
                let loop_frame = frame(ctx)?.loops.last().ok_or_else(invalid)?;
                let target = if matches!(op, Op::Break) {
                    loop_frame.break_target
                } else {
                    loop_frame.next_target
                };
                return Ok(Step::Jump(target));
            }
            Op::IndexGet {
                dst,
                target,
                index_const,
                site,
            } => {
                let call = self.guarded_call(ctx, site)?;
                let receiver = self.read(ctx, target)?;
                let result =
                    subscript::get(ctx, self, call, receiver, self.constant(index_const)?)?;
                self.write(ctx, dst, result)?;
                self.temp(ctx, SEXP_NIL)?;
            }
            Op::Index1Set(args) | Op::Index2Set(args) => {
                let call = self.guarded_call(ctx, args.site)?;
                let replacement =
                    self.resolve_symbol(ctx, args.replacement, args.repl_cache_idx, 0)?;
                let (_, sym) = self.symbol(ctx, args.var)?;
                let value = self.read(ctx, args.value)?;
                let result = subscript::set(
                    ctx,
                    call,
                    sym,
                    self.constant(args.index_const)?,
                    value,
                    replacement,
                )?;
                self.write(ctx, args.value, result)?;
            }
            Op::EvalAst { dst, ast_const } => {
                let value = ctx.eval(self.constant(ast_const)?, self.env)?;
                self.write(ctx, dst, value)?;
            }
            Op::CallFast { dst, site } => {
                let expr = self.constant(site.call_const)?;
                let value = match self.resolve(ctx, expr, site.cache_idx)? {
                    Some(fun)
                        if ctx.session.prim_parts(fun).ok().is_some_and(|(id, _)| {
                            ctx.prims[usize::from(id.0)].args == ArgEval::Eval
                        }) && ctx.expr_depth
                            < ctx.opts.expressions.min(ctx.host.limits().max_expressions) =>
                    {
                        let (_, tail, _) = ctx.session.cons_parts(expr)?;
                        ctx.expr_depth += 1;
                        let result = ctx.invoke_raw(fun, tail, expr, self.env);
                        ctx.expr_depth -= 1;
                        result?
                    }
                    _ => ctx.eval(expr, self.env)?,
                };
                self.write(ctx, dst, value)?;
            }
        }
        Ok(Step::Advance)
    }

    fn arithmetic(&self, ctx: &mut Ctx, op: &Op, args: Binary) -> EvalResult<()> {
        let call = self.guarded_call(ctx, args.site)?;
        let lhs = self.read(ctx, args.lhs)?;
        let rhs = self.read(ctx, args.rhs)?;
        if let (Some(a), Some(b)) = (
            scalar::number(&ctx.session, lhs),
            scalar::number(&ctx.session, rhs),
        ) {
            let (result, overflow) = scalar::binary(&mut ctx.session, op, a, b)?;
            self.write(ctx, args.dst, result)?;
            if overflow {
                ctx.warn_now(call.err_call(), roxide_diag::msg::INT_OVERFLOW)?;
            }
            ctx.visible = true;
        } else {
            let value = self.with_values(ctx, call, &[lhs, rhs])?;
            self.write(ctx, args.dst, value)?;
        }
        Ok(())
    }

    fn with_values(&self, ctx: &mut Ctx, call: Call, values: &[SexpId]) -> EvalResult {
        let args = subscript::arguments(ctx, call.expr)?;
        let args = args
            .iter()
            .enumerate()
            .map(|(i, (tag, original))| {
                let expr = match values.get(i) {
                    Some(&value) => ctx.value_promise(original, value)?,
                    None => original,
                };
                Ok((tag, expr))
            })
            .collect::<EvalResult<Vec<_>>>()?;
        ctx.run_primitive(call, Args::from_pairs(args), ArgInput::Expressions, true)
    }

    fn resolve(&self, ctx: &Ctx, expr: SexpId, cache: u16) -> EvalResult<Option<SexpId>> {
        let (head, _, _) = ctx.session.cons_parts(expr)?;
        let sym = ctx.session.sexp_as_sym(head)?;
        self.resolve_symbol(ctx, sym, cache, 1)
    }

    fn resolve_symbol(
        &self,
        ctx: &Ctx,
        sym: Sym,
        cache: u16,
        call_depth: usize,
    ) -> EvalResult<Option<SexpId>> {
        let limit = ctx.opts.expressions.min(ctx.host.limits().max_expressions);
        let can_force = ctx.expr_depth.saturating_add(call_depth) < limit;
        let mut caches = self.caches.borrow_mut();
        let slot = caches
            .calls
            .get_mut(usize::from(cache))
            .ok_or_else(invalid)?;
        let Some(fun) = guard::base_call(&ctx.session, sym, self.env, slot, can_force) else {
            return Ok(None);
        };
        let (id, _) = ctx.session.prim_parts(fun)?;
        let expected = ctx.session.sym_name(sym)?;
        Ok(ctx
            .prims
            .get(usize::from(id.0))
            .is_some_and(|def| def.name == expected)
            .then_some(fun))
    }

    fn guarded_call(&self, ctx: &mut Ctx, site: CallSite) -> EvalResult<Call> {
        let fun = frame(ctx)?
            .guards
            .last()
            .filter(|guard| guard.site == site)
            .ok_or_else(invalid)?
            .fun;
        let (prim, _) = ctx.session.prim_parts(fun)?;
        let expr = self.constant(site.call_const)?;
        let user_call = self.entry.map_or(expr, |(_, call)| {
            if call.expr == expr {
                call.err_call()
            } else {
                expr
            }
        });
        Ok(Call {
            expr,
            env: self.env,
            prim,
            user_call,
        })
    }

    fn close_guards(&self, ctx: &mut Ctx, pc: usize) -> EvalResult<()> {
        while let Some(guard) = frame(ctx)?.guards.last()
            && (pc <= guard.start || pc >= guard.end)
        {
            let guard = frame(ctx)?.guards.pop().ok_or_else(invalid)?;
            ctx.expr_depth -= usize::from(guard.counted);
            if let Some(value) = guard.retained {
                ctx.session.heap.dec_ref(value);
            }
        }
        Ok(())
    }

    fn constant(&self, index: u32) -> EvalResult {
        self.code
            .consts
            .get(index as usize)
            .copied()
            .ok_or_else(invalid)
    }

    fn symbol(&self, ctx: &Ctx, var: VarSite) -> EvalResult<(SexpId, Sym)> {
        let id = *self
            .code
            .syms
            .get(var.sym_idx as usize)
            .ok_or_else(invalid)?;
        Ok((id, ctx.session.sexp_as_sym(id)?))
    }

    fn read(&self, ctx: &mut Ctx, reg: Reg) -> EvalResult {
        frame(ctx)?
            .registers
            .get(usize::from(reg.0))
            .copied()
            .ok_or_else(invalid)
    }

    fn write(&self, ctx: &mut Ctx, reg: Reg, value: SexpId) -> EvalResult<()> {
        let frame = frame(ctx)?;
        *frame
            .registers
            .get_mut(usize::from(reg.0))
            .ok_or_else(invalid)? = value;
        if let Some(guard) = frame
            .guards
            .iter_mut()
            .rfind(|guard| guard.retain == Some(reg))
        {
            let previous = guard.retained.replace(value);
            ctx.session.heap.inc_ref(value);
            if let Some(previous) = previous {
                ctx.session.heap.dec_ref(previous);
            }
        }
        Ok(())
    }

    fn cursor<'a>(&self, ctx: &'a mut Ctx, reg: Reg) -> EvalResult<&'a mut ForCursor> {
        frame(ctx)?
            .cursors
            .get_mut(usize::from(reg.0))
            .ok_or_else(invalid)
    }

    pub(super) fn temp(&self, ctx: &mut Ctx, value: SexpId) -> EvalResult<()> {
        frame(ctx)?.temp = value;
        Ok(())
    }
}

fn checkpoint(ctx: &mut Ctx) -> EvalResult<()> {
    ctx.safepoint();
    ctx.run_pending_finalizers();
    ctx.check_user_interrupt()
}

fn frame(ctx: &mut Ctx) -> EvalResult<&mut VmFrame> {
    ctx.vm_stack.frames.last_mut().ok_or_else(invalid)
}

pub(super) fn invalid() -> Unwind {
    Unwind::Internal(InternalError {
        message: "invalid register bytecode".into(),
    })
}

#[cfg(test)]
mod tests {
    use roxide_gc::GcPolicy;
    use roxide_host::PureHost;
    use roxide_value::GLOBAL_ENV;

    use super::*;

    #[test]
    fn collection_between_instructions_preserves_live_operands_and_loop_values() {
        let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).unwrap();
        ctx.set_jit_level(3);
        ctx.vm_stack.collect_each_op = true;
        ctx.run_global(
            "<per-instruction GC>",
            r#"
            f <- function() {
                total <- 0L
                for (x in list(2L, 3L)) {
                    a <- c(x, x)
                    b <- a + { a[1L] <- 9L; 1L }
                    total <- total + b[1L] + b[2L]
                }
                total
            }
            result <- f()
            "#,
        )
        .unwrap();
        let name = ctx.session.intern("result").unwrap();
        let value = ctx.get_var(name, GLOBAL_ENV).unwrap().unwrap();
        assert_eq!(ctx.session.int(value).unwrap().get(0), 14);
    }
}

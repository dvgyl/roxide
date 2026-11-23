//! Core tree-walking evaluator behind `Ctx::eval` (ADR-0004 & ADR-0006).

use roxide_diag::msg;
use roxide_gc::SexpId;
use roxide_value::{
    PromiseState, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, Sym, ValueApi, ValueError,
};

use crate::api::{ArgEval, Arity, Call, EvalResult, InternalError, Unwind, Visibility};
use crate::ctx::{Args, Ctx};

const MAX_NATIVE_EVAL_DEPTH: usize = 192;

impl Ctx {
    /// Evaluate an R expression `expr` in environment `env`.
    pub fn eval(&mut self, expr: SexpId, env: SexpId) -> EvalResult {
        let result = self.scoped(|ctx| {
            ctx.protect(expr);
            ctx.protect(env);
            ctx.eval_protected(expr, env)
        });
        self.finish_error(result)
    }

    fn eval_protected(&mut self, expr: SexpId, env: SexpId) -> EvalResult {
        self.safepoint();
        if self.host.poll_interrupt() {
            return Err(self.stop(SEXP_NIL, "interrupted"));
        }
        // Leave space for Rust frames used by primitives and error handlers.
        if self.expr_depth
            >= self
                .opts
                .expressions
                .min(self.host.limits().max_expressions)
                .min(MAX_NATIVE_EVAL_DEPTH)
        {
            return Err(self.stop(
                SEXP_NIL,
                "evaluation nested too deeply: infinite recursion / options(expressions=)?",
            ));
        }
        self.expr_depth += 1;
        let previous_env = std::mem::replace(&mut self.eval_env, env);
        let result = self.eval_inner(expr, env);
        self.eval_env = previous_env;
        self.expr_depth -= 1;
        result
    }

    fn eval_inner(&mut self, expr: SexpId, env: SexpId) -> EvalResult {
        let ty = self.session.type_of(expr)?;
        match ty {
            SexpType::Nil
            | SexpType::Lgl
            | SexpType::Int
            | SexpType::Real
            | SexpType::Cplx
            | SexpType::Str
            | SexpType::Raw
            | SexpType::Vec
            | SexpType::Clo
            | SexpType::Builtin
            | SexpType::Special
            | SexpType::Env
            | SexpType::ExtPtr
            | SexpType::Obj => {
                self.visible = true;
                Ok(expr)
            }
            SexpType::Prom => self.force(expr),
            SexpType::Sym => {
                if expr == SEXP_MISSING {
                    return Err(self.stop(SEXP_NIL, "argument is missing, with no default"));
                }
                let sym = self.sym_from_sexp(expr)?;
                let name = self.session.sym_name(sym)?;
                if name == "..." {
                    return Err(self.stop(SEXP_NIL, "'...' used in an incorrect context"));
                }
                if let Some(index) = crate::closure::dots_index(name) {
                    let value = self.dots_elt(env, index, SEXP_NIL)?;
                    if value == SEXP_MISSING {
                        let name = self.session.sym_name(sym)?.to_owned();
                        return Err(self.stop(
                            SEXP_NIL,
                            format!("argument \"{name}\" is missing, with no default"),
                        ));
                    }
                    let value = self.eval(value, env)?;
                    self.visible = true;
                    return Ok(value);
                }
                let Some(val) = self.get_var(sym, env)? else {
                    let name = self.session.sym_name(sym)?.to_owned();
                    return Err(self.stop(SEXP_NIL, format!("object '{name}' not found")));
                };
                if val == SEXP_MISSING {
                    let name = self.session.sym_name(sym)?.to_owned();
                    let call = self.frame(env).map_or(SEXP_NIL, |frame| frame.call);
                    return Err(self.stop(
                        call,
                        format!("argument \"{name}\" is missing, with no default"),
                    ));
                }
                self.visible = true;
                Ok(val)
            }
            SexpType::Lang => self.eval_call(expr, env),
            _ => {
                self.visible = true;
                Ok(expr)
            }
        }
    }

    /// Force a lazy promise (`PROMSXP`) implementing ADR-0006's 4-state lifecycle.
    pub fn force(&mut self, promise: SexpId) -> EvalResult {
        self.scoped(|ctx| {
            ctx.protect(promise);
            ctx.force_protected(promise)
        })
    }

    fn force_protected(&mut self, promise: SexpId) -> EvalResult {
        self.safepoint();
        let (expr, penv, val, state) = self.session.promise_parts(promise)?;
        match state {
            PromiseState::Forced => Ok(val),
            PromiseState::Evaluating => {
                let call = self.frame(penv).map_or(SEXP_NIL, |frame| frame.call);
                Err(self.stop(call, msg::RECURSIVE_PROMISE))
            }
            PromiseState::Unforced | PromiseState::Interrupted => {
                if state == PromiseState::Interrupted {
                    self.warn_now(SEXP_NIL, msg::RESTART_PROMISE)?;
                }
                self.session.set_promise_state(
                    promise,
                    PromiseState::Evaluating,
                    penv,
                    SEXP_UNBOUND,
                )?;
                match self.eval(expr, penv) {
                    Ok(computed) => {
                        self.session.set_promise_state(
                            promise,
                            PromiseState::Forced,
                            SEXP_NIL,
                            computed,
                        )?;
                        Ok(computed)
                    }
                    Err(err) => {
                        let _ = self.session.set_promise_state(
                            promise,
                            PromiseState::Interrupted,
                            penv,
                            SEXP_UNBOUND,
                        );
                        Err(err)
                    }
                }
            }
        }
    }

    /// Look up symbol `sym` along the lexical environment chain starting at `env`, forcing promises.
    pub fn get_var(&mut self, sym: Sym, env: SexpId) -> EvalResult<Option<SexpId>> {
        let mut cur = env;
        while cur != SEXP_NIL {
            if let Some(binding) = self.session.env_get(cur, sym) {
                let val = binding.value;
                if val == SEXP_UNBOUND {
                    cur = self.session.env_enclos(cur)?;
                    continue;
                }
                if self.session.type_of(val)? == SexpType::Prom {
                    return self.force(val).map(Some);
                }
                return Ok(Some(val));
            }
            cur = self.session.env_enclos(cur)?;
        }
        Ok(None)
    }

    /// Look up a callable binding along the lexical chain (skipping non-function values like R).
    pub fn find_fun(&mut self, sym: Sym, env: SexpId) -> EvalResult<SexpId> {
        let mut cur = env;
        while cur != SEXP_NIL {
            if let Some(binding) = self.session.env_get(cur, sym) {
                let mut val = binding.value;
                if val == SEXP_UNBOUND {
                    cur = self.session.env_enclos(cur)?;
                    continue;
                }
                if self.session.type_of(val)? == SexpType::Prom {
                    val = self.force(val)?;
                }
                if val == SEXP_MISSING {
                    let name = self.session.sym_name(sym)?.to_owned();
                    let call = self.frame(env).map_or(SEXP_NIL, |frame| frame.call);
                    return Err(self.stop(
                        call,
                        format!("argument \"{name}\" is missing, with no default"),
                    ));
                }
                if matches!(
                    self.session.type_of(val)?,
                    SexpType::Clo | SexpType::Builtin | SexpType::Special
                ) {
                    return Ok(val);
                }
            }
            cur = self.session.env_enclos(cur)?;
        }
        let name = self.session.sym_name(sym)?.to_owned();
        Err(self.stop(SEXP_NIL, format!("could not find function \"{name}\"")))
    }

    /// Assign `val` to `sym` in `env`.
    pub fn assign(&mut self, sym: Sym, val: SexpId, env: SexpId) -> EvalResult<()> {
        match self.session.env_define(env, sym, val) {
            Ok(()) => Ok(()),
            Err(error @ ValueError::LockedBinding(_)) => {
                let call = self.frame(env).map_or(SEXP_NIL, |frame| frame.call);
                let message = if env == roxide_value::EMPTY_ENV {
                    String::from("cannot assign values in the empty environment")
                } else {
                    error.to_string()
                };
                Err(self.stop(call, message))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Super-assign (`<<-`) `val` to `sym` in the first enclosing environment binding `sym`,
    /// or `.GlobalEnv` if unbound in all enclosing frames.
    pub fn assign_super(&mut self, sym: Sym, val: SexpId, env: SexpId) -> EvalResult<()> {
        let dest = self.super_env(sym, env)?;
        self.assign(sym, val, dest)
    }

    pub(crate) fn super_env(&self, sym: Sym, env: SexpId) -> EvalResult {
        let mut cur = self.session.env_enclos(env)?;
        while cur != SEXP_NIL && cur != roxide_value::EMPTY_ENV {
            if self.session.env_get(cur, sym).is_some() {
                return Ok(cur);
            }
            cur = self.session.env_enclos(cur)?;
        }
        Ok(roxide_value::GLOBAL_ENV)
    }

    /// Apply a callable `fun` to already-evaluated `(Option<Sym>, SexpId)` arguments.
    pub fn apply(
        &mut self,
        fun: SexpId,
        args: &[(Option<Sym>, SexpId)],
        call_expr: SexpId,
        env: SexpId,
    ) -> EvalResult {
        let result = self.scoped(|ctx| {
            ctx.protect(fun);
            ctx.protect(call_expr);
            ctx.protect(env);
            for &(_, value) in args {
                ctx.protect(value);
            }
            ctx.safepoint();
            match ctx.session.type_of(fun)? {
                SexpType::Builtin | SexpType::Special => {
                    let (prim, _) = ctx.session.prim_parts(fun)?;
                    let call = Call {
                        expr: call_expr,
                        env,
                        prim,
                        user_call: call_expr,
                    };
                    ctx.run_primitive(call, Args::from_pairs(args.to_vec()), true, true)
                }
                SexpType::Clo => {
                    let pairs = args
                        .iter()
                        .map(|&(tag, value)| Ok((tag, ctx.value_promise(value, value)?)))
                        .collect::<EvalResult<Vec<_>>>()?;
                    ctx.invoke_closure(fun, Args::from_pairs(pairs), call_expr, env, None)
                }
                _ => Err(ctx.stop(call_expr, msg::NOT_A_FUNCTION)),
            }
        });
        self.finish_error(result)
    }

    /// Look up function `name` in `env` and apply it to evaluated arguments.
    pub fn call_function(
        &mut self,
        name: &str,
        args: &[(Option<Sym>, SexpId)],
        env: SexpId,
    ) -> EvalResult {
        self.scoped(|ctx| {
            ctx.protect(env);
            for &(_, arg) in args {
                ctx.protect(arg);
            }
            let sym = ctx.session.intern(name)?;
            let fun = ctx.find_fun(sym, env)?;
            let call = ctx.make_call(sym.as_sexp(), &Args::from_pairs(args.to_vec()))?;
            ctx.apply(fun, args, call, env)
        })
    }

    fn eval_call(&mut self, call_expr: SexpId, env: SexpId) -> EvalResult {
        let (head, arg_list, _) = self.session.cons_parts(call_expr)?;
        let fun = if self.session.type_of(head)? == SexpType::Sym {
            let sym = self.sym_from_sexp(head)?;
            self.find_fun(sym, env)?
        } else {
            self.eval(head, env)?
        };
        self.invoke_raw(fun, arg_list, call_expr, env)
    }

    pub(crate) fn invoke_raw(
        &mut self,
        fun: SexpId,
        arg_list: SexpId,
        expr: SexpId,
        env: SexpId,
    ) -> EvalResult {
        self.scoped(|ctx| {
            for root in [fun, arg_list, expr, env] {
                ctx.protect(root);
            }
            match ctx.session.type_of(fun)? {
                SexpType::Builtin | SexpType::Special => {
                    let (prim, _) = ctx.session.prim_parts(fun)?;
                    let call = Call {
                        expr,
                        env,
                        prim,
                        user_call: expr,
                    };
                    ctx.run_primitive(
                        call,
                        Args::from_pairs(ctx.session.pairlist_iter(arg_list).collect()),
                        false,
                        true,
                    )
                }
                SexpType::Clo => {
                    let raw = Args::from_pairs(ctx.session.pairlist_iter(arg_list).collect());
                    let actuals = ctx.promise_args(&raw, env, expr)?;
                    ctx.invoke_closure(fun, actuals, expr, env, None)
                }
                _ => Err(ctx.stop(expr, msg::NOT_A_FUNCTION)),
            }
        })
    }

    pub(crate) fn run_primitive(
        &mut self,
        call: Call,
        mut args: Args,
        evaluated: bool,
        dispatch: bool,
    ) -> EvalResult {
        let Some(&def) = self.prims.get(usize::from(call.prim.0)) else {
            return Err(Unwind::Internal(InternalError {
                message: "unknown primitive".into(),
            }));
        };
        self.scoped(|ctx| {
            for value in [call.expr, call.env, call.user_call] {
                ctx.protect(value);
            }
            for &value in args.values() {
                ctx.protect(value);
            }
            let mut promised = None;
            if !evaluated && def.args != ArgEval::Unevaluated {
                let raw = ctx.promise_args(&args, call.env, call.err_call())?;
                let mut values = Vec::with_capacity(raw.len());
                for &arg in raw.values() {
                    ctx.protect(arg);
                }
                for (tag, arg) in raw.iter() {
                    let value = if arg == SEXP_MISSING {
                        if def.args == ArgEval::Eval {
                            return Err(
                                ctx.stop(call.err_call(), "argument is missing, with no default")
                            );
                        }
                        arg
                    } else {
                        ctx.force(arg)?
                    };
                    ctx.protect(value);
                    values.push((tag, value));
                }
                args = Args::from_pairs(values);
                promised = Some(raw);
            }
            let valid = match def.arity {
                Arity::Any => true,
                Arity::Exact(count) => args.len() == count,
                Arity::Range(lo, hi) => (lo..=hi).contains(&args.len()),
            };
            if !valid {
                return Err(ctx.stop(
                    call.err_call(),
                    format!("incorrect number of arguments to '{}'", def.name),
                ));
            }
            ctx.visible = true;
            if dispatch
                && let Some(result) = ctx.dispatch_primitive(def, call, &args, promised.as_ref())?
            {
                return Ok(result);
            }
            let result = (def.fun)(ctx, &call, &mut args)?;
            let result = ctx.preserve_group_attributes(def, &args, result)?;
            ctx.visible = match def.visibility {
                Visibility::On => true,
                Visibility::Off => false,
                Visibility::SetByFn => ctx.visible,
            };
            Ok(result)
        })
    }

    pub(crate) fn check_vector_len(
        &mut self,
        len: usize,
        element_bytes: usize,
        call: SexpId,
    ) -> EvalResult<()> {
        if len > self.host.limits().max_vector_bytes / element_bytes {
            return Err(self.stop(call, "vector allocation exceeds the session limit"));
        }
        Ok(())
    }

    pub(crate) fn sym_from_sexp(&mut self, id: SexpId) -> Result<Sym, Unwind> {
        let name = self.session.sym_name_of(id)?.to_owned();
        Ok(self.session.intern(&name)?)
    }
}

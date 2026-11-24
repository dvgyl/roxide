//! Core tree-walking evaluator behind `Ctx::eval` (ADR-0004 & ADR-0006).

use roxide_diag::msg;
use roxide_gc::SexpId;
use roxide_value::{
    PromiseState, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, Sym, ValueApi, ValueError,
};

use crate::api::{ArgEval, Arity, Call, Dispatch, EvalResult, InternalError, Unwind, Visibility};
use crate::ctx::{Args, Ctx};
use crate::dispatch::DispatchInput;

const OVERFLOW_HEADROOM: usize = 500;
const STACK_RECOVERY_BYTES: usize = 128 * 1024;

pub(crate) enum ArgInput {
    Expressions,
    ReceiverValue {
        target: SexpId,
        replacement: Option<SexpId>,
    },
    Values,
}

fn is_subscript_generic(dispatch: Dispatch) -> bool {
    matches!(dispatch, Dispatch::Internal("[" | "[[" | "[<-" | "[[<-"))
}

impl Ctx {
    /// Evaluate an R expression `expr` in environment `env`.
    pub fn eval(&mut self, expr: SexpId, env: SexpId) -> EvalResult {
        let marker = 0_u8;
        let previous_stack = self.enter_evaluation(&marker)?;
        let mark = self.session.heap.protect_mark();
        self.protect(expr);
        self.protect(env);
        let result = self.eval_protected(expr, env);
        let result = self.condition_result(SEXP_NIL, result);
        self.session.heap.unprotect_to(mark);
        self.stack_origin = previous_stack;
        result
    }

    fn enter_evaluation(&mut self, marker: &u8) -> EvalResult<Option<usize>> {
        // Keep a reserve for condition handlers and stack unwinding.
        let address = std::ptr::from_ref(marker).addr();
        let Some(origin) = self.stack_origin else {
            self.stack_origin = Some(address);
            return Ok(None);
        };
        let used = origin.abs_diff(address);
        let limit = self
            .stack_budget
            .saturating_sub(if self.overflow_headroom == 0 {
                STACK_RECOVERY_BYTES
            } else {
                0
            });
        if used < limit {
            Ok(Some(origin))
        } else if self.overflow_headroom == 0 {
            Err(self.expression_overflow())
        } else {
            Err(Unwind::TopLevel)
        }
    }

    fn eval_protected(&mut self, expr: SexpId, env: SexpId) -> EvalResult {
        self.safepoint();
        self.run_pending_finalizers();
        self.check_user_interrupt()?;
        let ty = self.session.type_of(expr)?;
        let counted = usize::from(matches!(ty, SexpType::Lang | SexpType::Prom));
        let limit = self
            .opts
            .expressions
            .min(self.host.limits().max_expressions)
            .saturating_add(self.overflow_headroom);
        if counted != 0 && self.expr_depth >= limit {
            return Err(if self.overflow_headroom == 0 {
                self.expression_overflow()
            } else {
                Unwind::TopLevel
            });
        }
        self.expr_depth += counted;
        let previous_env = std::mem::replace(&mut self.eval_env, env);
        let result = match ty {
            SexpType::Lang => self.eval_call(expr, env),
            SexpType::Prom => self.force_protected(expr),
            SexpType::Sym => self.eval_symbol(expr, env),
            _ => {
                self.visible = true;
                Ok(expr)
            }
        };
        self.eval_env = previous_env;
        self.expr_depth -= counted;
        result
    }

    fn expression_overflow(&mut self) -> Unwind {
        self.overflow_headroom = OVERFLOW_HEADROOM;
        let error = self.stack_overflow_condition();
        self.overflow_headroom = 0;
        error
    }

    fn eval_symbol(&mut self, expr: SexpId, env: SexpId) -> EvalResult {
        self.visible = true;
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
            return self.eval(value, env);
        }
        let Some((binding_env, binding)) = self.lookup_binding(sym, env)? else {
            let name = self.session.sym_name(sym)?.to_owned();
            let call = self.frame(env).map_or(SEXP_NIL, |frame| frame.call);
            return Err(self.stop(call, format!("object '{name}' not found")));
        };
        if !binding.active && binding.value == SEXP_MISSING {
            let name = self.session.sym_name(sym)?.to_owned();
            let call = self.frame(env).map_or(SEXP_NIL, |frame| frame.call);
            return Err(self.stop(
                call,
                format!("argument \"{name}\" is missing, with no default"),
            ));
        }
        self.read_binding(binding, binding_env)
    }

    /// Force a lazy promise (`PROMSXP`) implementing ADR-0006's 4-state lifecycle.
    pub fn force(&mut self, promise: SexpId) -> EvalResult {
        self.scoped(|ctx| {
            ctx.protect(promise);
            let result = ctx.force_protected(promise);
            ctx.condition_result(SEXP_NIL, result)
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

    fn lookup_binding(
        &self,
        sym: Sym,
        mut env: SexpId,
    ) -> EvalResult<Option<(SexpId, roxide_value::Binding)>> {
        while env != SEXP_NIL {
            if let Some(binding) = self.session.env_get(env, sym)
                && binding.value != SEXP_UNBOUND
            {
                return Ok(Some((env, binding)));
            }
            env = self.session.env_enclos(env)?;
        }
        Ok(None)
    }

    /// Look up symbol `sym` along the lexical environment chain starting at `env`, forcing promises.
    pub fn get_var(&mut self, sym: Sym, env: SexpId) -> EvalResult<Option<SexpId>> {
        self.lookup_binding(sym, env)?
            .map(|(cur, binding)| self.read_binding(binding, cur))
            .transpose()
    }

    /// Read a binding in one environment, forcing a promise or calling an active getter.
    pub fn get_local(&mut self, sym: Sym, env: SexpId) -> EvalResult<Option<SexpId>> {
        self.session
            .env_get(env, sym)
            .map(|binding| self.read_binding(binding, env))
            .transpose()
    }

    pub(crate) fn read_binding(
        &mut self,
        binding: roxide_value::Binding,
        env: SexpId,
    ) -> EvalResult {
        if binding.active {
            self.apply(binding.value, &[], SEXP_NIL, env)
        } else if self.session.type_of(binding.value)? == SexpType::Prom {
            self.eval(binding.value, env)
        } else {
            Ok(binding.value)
        }
    }

    /// Look up a callable binding along the lexical chain.
    pub fn find_fun(&mut self, sym: Sym, env: SexpId) -> EvalResult {
        let result = self.find_fun_at(sym, env, SEXP_NIL);
        self.condition_result(SEXP_NIL, result)
    }

    pub(crate) fn find_fun_at(&mut self, sym: Sym, env: SexpId, call: SexpId) -> EvalResult {
        let mut cur = env;
        while cur != SEXP_NIL {
            if let Some(binding) = self.session.env_get(cur, sym) {
                let val = binding.value;
                if val == SEXP_UNBOUND {
                    cur = self.session.env_enclos(cur)?;
                    continue;
                }
                if !binding.active && val == SEXP_MISSING {
                    let name = self.session.sym_name(sym)?.to_owned();
                    let call = self.frame(env).map_or(SEXP_NIL, |frame| frame.call);
                    return Err(self.stop(
                        call,
                        format!("argument \"{name}\" is missing, with no default"),
                    ));
                }
                let val = self.read_binding(binding, cur)?;
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
        Err(self.stop(call, format!("could not find function \"{name}\"")))
    }

    /// Assign `val` to `sym` in `env`.
    pub fn assign(&mut self, sym: Sym, val: SexpId, env: SexpId) -> EvalResult<()> {
        if let Some(binding) = self.session.env_get(env, sym)
            && binding.active
            && !binding.locked
        {
            return self
                .apply(binding.value, &[(None, val)], SEXP_NIL, env)
                .map(|_| ());
        }
        match self.session.env_define(env, sym, val) {
            Ok(()) => Ok(()),
            Err(error @ (ValueError::LockedBinding(_) | ValueError::LockedEnvironment)) => {
                let call = self.frame(env).map_or(SEXP_NIL, |frame| frame.call);
                let message = if env == roxide_value::EMPTY_ENV {
                    String::from("cannot assign values in the empty environment")
                } else {
                    error.to_string()
                };
                Err(self.stop(call, message))
            }
            Err(error) => self.condition_result(SEXP_NIL, Err(error.into())),
        }
    }

    /// Super-assign (`<<-`) `val` to `sym` in the first enclosing environment binding `sym`,
    /// or `.GlobalEnv` if unbound in all enclosing frames.
    pub fn assign_super(&mut self, sym: Sym, val: SexpId, env: SexpId) -> EvalResult<()> {
        let dest = self.super_env(sym, env);
        let dest = self.condition_result(SEXP_NIL, dest)?;
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
        let marker = 0_u8;
        let previous_stack = self.enter_evaluation(&marker)?;
        let mark = self.session.heap.protect_mark();
        let result = self.apply_protected(fun, args, call_expr, env);
        let result = self.condition_result(call_expr, result);
        self.session.heap.unprotect_to(mark);
        self.stack_origin = previous_stack;
        result
    }

    fn apply_protected(
        &mut self,
        fun: SexpId,
        args: &[(Option<Sym>, SexpId)],
        call_expr: SexpId,
        env: SexpId,
    ) -> EvalResult {
        let ctx = self;
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
                ctx.run_primitive(
                    call,
                    Args::from_pairs(args.to_vec()),
                    ArgInput::Values,
                    true,
                )
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
            let call = ctx.make_call(sym.as_sexp(), &Args::from_pairs(args.to_vec()))?;
            ctx.protect(call);
            let fun = ctx.find_fun_at(sym, env, call)?;
            ctx.apply(fun, args, call, env)
        })
    }

    fn eval_call(&mut self, call_expr: SexpId, env: SexpId) -> EvalResult {
        let (head, arg_list, _) = self.session.cons_parts(call_expr)?;
        let fun = if self.session.type_of(head)? == SexpType::Sym {
            let sym = self.sym_from_sexp(head)?;
            self.find_fun_at(sym, env, call_expr)?
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
                        ArgInput::Expressions,
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
        args: Args,
        input: ArgInput,
        dispatch: bool,
    ) -> EvalResult {
        let Some(&def) = self.prims.get(usize::from(call.prim.0)) else {
            return Err(Unwind::Internal(InternalError {
                message: "unknown primitive".into(),
            }));
        };
        let mark = self.session.heap.protect_mark();
        let trace = matches!(
            def.name,
            "print"
                | "dput"
                | "cat"
                | "sprintf"
                | "tryCatch"
                | "withCallingHandlers"
                | "withRestarts"
        );
        if trace {
            self.calls.push((call.expr, call.env));
        }
        let result = self.run_primitive_body(def, call, args, input, dispatch);
        let result = self.condition_result(call.err_call(), result);
        if trace {
            self.calls.pop();
        }
        self.session.heap.unprotect_to(mark);
        result
    }

    fn run_primitive_body(
        &mut self,
        def: &crate::PrimDef,
        call: Call,
        mut args: Args,
        input: ArgInput,
        dispatch: bool,
    ) -> EvalResult {
        let ctx = self;
        for value in [call.expr, call.env, call.user_call] {
            ctx.protect(value);
        }
        for &value in args.values() {
            ctx.protect(value);
        }
        let (mut receiver, replacement) = match input {
            ArgInput::ReceiverValue {
                target,
                replacement,
            } => {
                ctx.protect(target);
                if let Some(value) = replacement {
                    ctx.protect(value);
                }
                (Some(target), replacement)
            }
            _ => (None, None),
        };
        let mut dispatch = dispatch;
        let mut raw;
        let source = match input {
            ArgInput::Expressions | ArgInput::ReceiverValue { .. }
                if def.args != ArgEval::Unevaluated =>
            {
                let subscript = dispatch && is_subscript_generic(def.dispatch);
                if subscript
                    && receiver.is_none()
                    && !args.is_empty()
                    && ctx.session.sym_name_of(args.get(0)).ok() != Some("...")
                {
                    receiver = Some(ctx.eval_subscript_receiver(args.get(0), call.env)?);
                }
                raw = if def.args == ArgEval::EvalDollar {
                    args
                } else if receiver.is_some() {
                    let end = args.len() - usize::from(replacement.is_some());
                    let middle = Args::from_pairs(args.iter().skip(1).take(end - 1).collect());
                    let middle = ctx.expand_dots(&middle, call.env, call.err_call())?;
                    Args::from_pairs(
                        args.iter()
                            .take(1)
                            .chain(middle.iter())
                            .chain(args.iter().skip(end))
                            .collect(),
                    )
                } else {
                    ctx.expand_dots(&args, call.env, call.err_call())?
                };
                if subscript && !raw.is_empty() {
                    for &arg in raw.values() {
                        ctx.protect(arg);
                    }
                    let target = match receiver {
                        Some(value) => value,
                        None => ctx.eval_subscript_receiver(raw.get(0), call.env)?,
                    };
                    if target != SEXP_MISSING
                        && (ctx.session.is_object(target)? || ctx.session.is_s4(target)?)
                    {
                        raw = ctx.subscript_promises(&raw, call, target, replacement)?;
                        ctx.visible = true;
                        if let Some(result) = ctx.dispatch_primitive(
                            def,
                            call,
                            DispatchInput::Subscript {
                                receiver: target,
                                actuals: &raw,
                            },
                        )? {
                            return Ok(result);
                        }
                    }
                    receiver = Some(target);
                    dispatch = false;
                }
                args = ctx.eval_primitive_args(&raw, call, def.args, receiver, replacement)?;
                Some(&raw)
            }
            _ => None,
        };
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
        let retained: Vec<_> = if def.args == ArgEval::Unevaluated {
            Vec::new()
        } else {
            args.values()
                .iter()
                .copied()
                .skip(usize::from(ctx.can_overwrite(&call)))
                .collect()
        };
        for &value in &retained {
            ctx.session.heap.inc_ref(value);
        }
        let result = ctx.call_primitive(def, call, &mut args, source, dispatch);
        for value in retained {
            ctx.session.heap.dec_ref(value);
        }
        result
    }

    fn call_primitive(
        &mut self,
        def: &crate::PrimDef,
        call: Call,
        args: &mut Args,
        raw: Option<&Args>,
        dispatch: bool,
    ) -> EvalResult {
        self.visible = true;
        if dispatch
            && let Some(result) =
                self.dispatch_primitive(def, call, DispatchInput::Values { args, source: raw })?
        {
            return Ok(result);
        }
        if def.name == "names" && self.session.type_of(args.get(0))? == SexpType::Env {
            return self.environment_names(args.get(0));
        }
        if self.session.foreign_alt_seen() && !self.is_altrep_aware(def.name) {
            for &value in args.values() {
                self.resolve_altrep(value)?;
            }
        }
        let result = if self.has_stream_sink() {
            self.invoke_streaming_primitive(def.fun, &call, args)
        } else {
            (def.fun)(self, &call, args)
        };
        let result = match result {
            Err(Unwind::Value(err @ ValueError::ForeignAltrep { .. })) => {
                return Err(self.stop(call.err_call(), err.to_string()));
            }
            result => result?,
        };
        let result = self.preserve_group_attributes(def, args, result)?;
        self.visible = match def.visibility {
            Visibility::On => true,
            Visibility::Off => false,
            Visibility::SetByFn => self.visible,
        };
        Ok(result)
    }

    fn eval_subscript_receiver(&mut self, expr: SexpId, env: SexpId) -> EvalResult {
        let target = if expr == SEXP_MISSING {
            SEXP_MISSING
        } else {
            self.eval(expr, env)?
        };
        self.protect(target);
        Ok(target)
    }

    fn subscript_promises(
        &mut self,
        raw: &Args,
        call: Call,
        receiver: SexpId,
        replacement: Option<SexpId>,
    ) -> EvalResult<Args> {
        let receiver_expr = self.promise_expr(raw.get(0))?;
        let receiver = self.value_promise(receiver_expr, receiver)?;
        self.protect(receiver);
        let mut pairs: Vec<_> = [(raw.name(0), receiver)]
            .into_iter()
            .chain(raw.iter().skip(1))
            .collect();
        if let Some(value) = replacement
            && let Some((_, expr)) = pairs.last_mut()
        {
            *expr = self.value_promise(*expr, value)?;
            self.protect(*expr);
        }
        let actuals = self.promise_args(&Args::from_pairs(pairs), call.env, call.err_call())?;
        for &promise in actuals.values() {
            self.protect(promise);
        }
        Ok(actuals)
    }

    fn eval_primitive_args(
        &mut self,
        raw: &Args,
        call: Call,
        policy: ArgEval,
        receiver: Option<SexpId>,
        replacement: Option<SexpId>,
    ) -> EvalResult<Args> {
        for &arg in raw.values() {
            self.protect(arg);
        }
        let mut values = Vec::with_capacity(raw.len());
        let result = (|| {
            for (i, (tag, arg)) in raw.iter().enumerate() {
                let value = if i == 0
                    && let Some(value) = receiver
                {
                    value
                } else if i + 1 == raw.len()
                    && let Some(value) = replacement
                {
                    value
                } else if policy == ArgEval::EvalDollar && i == 1 {
                    arg
                } else if arg == SEXP_MISSING {
                    if policy != ArgEval::EvalKeepMissing {
                        return Err(
                            self.stop(call.err_call(), "argument is missing, with no default")
                        );
                    }
                    arg
                } else {
                    self.eval(arg, call.env)?
                };
                self.protect(value);
                self.session.heap.inc_ref(value);
                values.push((tag, value));
            }
            Ok(())
        })();
        for &(_, value) in &values {
            self.session.heap.dec_ref(value);
        }
        result?;
        Ok(Args::from_pairs(values))
    }

    pub(crate) fn with_call<T>(
        &mut self,
        call: SexpId,
        env: SexpId,
        run: impl FnOnce(&mut Self) -> EvalResult<T>,
    ) -> EvalResult<T> {
        self.calls.push((call, env));
        let result = run(self);
        self.calls.pop();
        result
    }

    pub(crate) fn condition_result<T>(
        &mut self,
        call: SexpId,
        result: EvalResult<T>,
    ) -> EvalResult<T> {
        result.map_err(|err| match err {
            Unwind::Value(error) => self.stop(call, error.to_string()),
            other => other,
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

//! Core tree-walking evaluator behind `Ctx::eval` (ADR-0004 & ADR-0006).

use roxide_diag::msg;
use roxide_gc::SexpId;
use roxide_value::{PromiseState, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, Sym, ValueApi};

use crate::api::{ArgEval, Call, EvalResult, Unwind, Visibility};
use crate::ctx::{Args, Ctx};

impl Ctx {
    /// Evaluate an R expression `expr` in environment `env`.
    pub fn eval(&mut self, expr: SexpId, env: SexpId) -> EvalResult {
        self.safepoint();
        if self.host.poll_interrupt() {
            return Err(self.stop(SEXP_NIL, "interrupted"));
        }
        if self.expr_depth >= self.opts.expressions {
            return Err(self.stop(
                SEXP_NIL,
                "evaluation nested too deeply: infinite recursion / options(expressions=)?",
            ));
        }
        self.expr_depth += 1;
        let res = self.eval_inner(expr, env);
        self.expr_depth -= 1;
        res
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
                let Some(val) = self.get_var(sym, env)? else {
                    let name = self.session.sym_name(sym)?.to_owned();
                    return Err(self.stop(SEXP_NIL, format!("object '{name}' not found")));
                };
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
        self.safepoint();
        let (expr, penv, val, state) = self.session.promise_parts(promise)?;
        match state {
            PromiseState::Forced => Ok(val),
            PromiseState::Evaluating => Err(self.stop(SEXP_NIL, msg::RECURSIVE_PROMISE)),
            PromiseState::Unforced | PromiseState::Interrupted => {
                if state == PromiseState::Interrupted {
                    self.warn(SEXP_NIL, msg::RESTART_PROMISE);
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
        self.session.env_define(env, sym, val)?;
        Ok(())
    }

    fn eval_call(&mut self, call_expr: SexpId, env: SexpId) -> EvalResult {
        let (head, arg_list, _) = self.session.cons_parts(call_expr)?;
        let fun = if self.session.type_of(head)? == SexpType::Sym {
            let sym = self.sym_from_sexp(head)?;
            self.find_fun(sym, env)?
        } else {
            self.eval(head, env)?
        };

        let fun_ty = self.session.type_of(fun)?;
        match fun_ty {
            SexpType::Builtin | SexpType::Special => {
                let (prim_id, _) = self.session.prim_parts(fun)?;
                let def = self.prims[prim_id.0 as usize];
                let raw_pairs: Vec<(Option<Sym>, SexpId)> =
                    self.session.pairlist_iter(arg_list).collect();

                let evaluated = match def.args {
                    ArgEval::Unevaluated => raw_pairs,
                    ArgEval::Eval | ArgEval::EvalKeepMissing => self.scoped(|ctx| {
                        let mut out = Vec::with_capacity(raw_pairs.len());
                        for (tag, arg_expr) in raw_pairs {
                            if arg_expr == SEXP_MISSING {
                                if matches!(def.args, ArgEval::EvalKeepMissing) {
                                    out.push((tag, SEXP_MISSING));
                                    continue;
                                }
                                return Err(
                                    ctx.stop(call_expr, "argument is missing, with no default")
                                );
                            }
                            let v = ctx.eval(arg_expr, env)?;
                            ctx.protect(v);
                            out.push((tag, v));
                        }
                        Ok(out)
                    })?,
                };

                let mut args = Args::from_pairs(evaluated);
                let call = Call {
                    expr: call_expr,
                    env,
                    prim: prim_id,
                    user_call: call_expr,
                };
                let res = self.scoped(|ctx| {
                    for &v in args.values() {
                        ctx.protect(v);
                    }
                    (def.fun)(ctx, &call, &mut args)
                })?;
                match def.visibility {
                    Visibility::On => self.visible = true,
                    Visibility::Off => self.visible = false,
                    Visibility::SetByFn => {}
                }
                Ok(res)
            }
            SexpType::Clo => Err(self.stop(
                call_expr,
                "closure evaluation will be enabled in Milestone 1",
            )),
            _ => Err(self.stop(call_expr, msg::NOT_A_FUNCTION)),
        }
    }

    pub(crate) fn sym_from_sexp(&mut self, id: SexpId) -> Result<Sym, Unwind> {
        let name = self.session.sym_name_of(id)?.to_owned();
        Ok(self.session.intern(&name)?)
    }
}

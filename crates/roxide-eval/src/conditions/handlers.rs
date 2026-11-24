use roxide_diag::format as diag_format;
use roxide_gc::SexpId;
use roxide_value::{DeparseOpts, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use crate::{Args, Call, Ctx, EvalResult, Transfer, Unwind};

use super::Handler;

impl Ctx {
    /// Execute `body` with an exiting `"error"` handler (`R_tryCatchError`, ADR-0018).
    ///
    /// If `body` signals an R `"error"` condition, the handler stack is restored and
    /// `handler` is invoked with the GC-protected condition `SEXP`.
    pub fn with_native_error_handler(
        &mut self,
        body: impl FnOnce(&mut Self) -> EvalResult,
        handler: impl FnOnce(&mut Self, SexpId) -> EvalResult,
    ) -> EvalResult {
        let target = self.next_frame();
        let base = self.conditions.handlers.len();
        self.conditions.handlers.push(Handler {
            target: Some(target),
            class: String::from("error"),
            fun: SEXP_NIL,
        });
        let result = body(self);
        self.conditions.handlers.truncate(base);
        match result {
            Err(Unwind::Transfer {
                target: dest,
                kind: Transfer::ExitingHandler { cond, .. },
            }) if dest == target => self.scoped(|ctx| {
                ctx.protect(cond);
                handler(ctx, cond)
            }),
            other => other,
        }
    }

    /// Execute `body` with isolated condition handlers and restarts (`R_ToplevelExec`, ADR-0018).
    ///
    /// Outer handlers and restarts are saved inside `Ctx::conditions` (remaining GC-rooted)
    /// and unconditionally restored when `body` returns or unwinds.
    pub fn with_native_toplevel<R>(
        &mut self,
        body: impl FnOnce(&mut Self) -> EvalResult<R>,
    ) -> EvalResult<R> {
        let saved_handlers = std::mem::take(&mut self.conditions.handlers);
        let saved_restarts = std::mem::take(&mut self.conditions.restarts);
        self.conditions
            .saved_stacks
            .push((saved_handlers, saved_restarts));
        let result = body(self);
        if let Some((handlers, restarts)) = self.conditions.saved_stacks.pop() {
            self.conditions.handlers = handlers;
            self.conditions.restarts = restarts;
        }
        result
    }

    /// Run calling handlers or transfer control to an exiting handler.
    pub(crate) fn signal_condition(&mut self, cond: SexpId) -> EvalResult<()> {
        self.scoped(|ctx| {
            ctx.protect(cond);
            ctx.safepoint();
            let classes = ctx.classes(cond, false)?;
            let mut end = ctx.conditions.handlers.len();
            while let Some(index) = ctx.conditions.handlers[..end]
                .iter()
                .rposition(|h| classes.contains(&h.class))
            {
                let handler = &ctx.conditions.handlers[index];
                if let Some(target) = handler.target {
                    return Err(Unwind::Transfer {
                        target,
                        kind: Transfer::ExitingHandler {
                            handler: index,
                            cond,
                        },
                    });
                }
                let fun = handler.fun;
                let handlers = ctx.conditions.handlers.split_off(index);
                let visible = ctx.visible;
                let result = ctx.scoped(|ctx| {
                    for handler in &handlers {
                        ctx.protect(handler.fun);
                    }
                    ctx.apply(fun, &[(None, cond)], SEXP_NIL, ctx.eval_env)
                });
                ctx.conditions.handlers.extend(handlers);
                result?;
                ctx.visible = visible;
                end = index;
            }
            Ok(())
        })
    }

    fn eval_try_expr(
        &mut self,
        args: &Args,
        matched: &crate::closure::Matches,
        call: &Call,
    ) -> EvalResult {
        let list_call = self.named_call(
            "tryCatchList",
            &["expr", "classes", "parentenv", "handlers"],
        )?;
        self.with_call(list_call, call.env, |ctx| {
            if matched.dots.is_empty() {
                return ctx.eval_arg(args, matched, 0, call, SEXP_MISSING);
            }
            let one_call =
                ctx.named_call("tryCatchOne", &["expr", "names", "parentenv", "handlers"])?;
            let ret = ctx.named_call("return", &["expr"])?;
            let invoke = ctx.session.intern("doTryCatch")?.as_sexp();
            let names = ["name", "parentenv", "handler"].map(|name| ctx.session.intern(name));
            let [name, env, handler] = names;
            let invoke = ctx.make_call(
                invoke,
                &Args::from_pairs(vec![
                    (None, ret),
                    (None, name?.as_sexp()),
                    (None, env?.as_sexp()),
                    (None, handler?.as_sexp()),
                ]),
            )?;
            ctx.with_call(one_call, call.env, |ctx| {
                ctx.with_call(invoke, call.env, |ctx| {
                    ctx.eval_arg(args, matched, 0, call, SEXP_MISSING)
                })
            })
        })
    }

    fn named_call(&mut self, name: &str, args: &[&str]) -> EvalResult {
        let head = self.session.intern(name)?.as_sexp();
        let args = args
            .iter()
            .map(|name| self.session.intern(name).map(|sym| (None, sym.as_sexp())))
            .collect::<Result<Vec<_>, _>>()?;
        self.make_call(head, &Args::from_pairs(args))
    }
}

pub(super) fn with_handlers(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let exiting = ctx.prims[usize::from(call.prim.0)].name == "tryCatch";
    let names: &[&str] = if exiting {
        &["expr", "...", "finally"]
    } else {
        &["expr", "..."]
    };
    let args = &ctx.expand_dots(args, call.env, call.err_call())?;
    for &value in args.values() {
        ctx.protect(value);
    }
    let matched = ctx.match_names(args, names, call.err_call())?;
    let finally = if exiting {
        ctx.matched_arg(args, &matched, 2)
    } else {
        None
    };
    let id = ctx.next_frame();
    let base = ctx.conditions.handlers.len();
    let mut result = (|| {
        let mut handlers = Vec::new();
        for i in matched.dots.iter().copied() {
            let Some(tag) = args.name(i) else {
                return Err(ctx.stop(
                    call.err_call(),
                    "condition handlers must be specified with a condition class",
                ));
            };
            let class = ctx.session.sym_name(tag)?.to_owned();
            let fun = ctx.eval(args.get(i), call.env)?;
            ctx.protect(fun);
            if !matches!(
                ctx.session.type_of(fun)?,
                SexpType::Clo | SexpType::Builtin | SexpType::Special
            ) {
                return Err(ctx.stop(call.err_call(), "condition handlers must be functions"));
            }
            handlers.push(Handler {
                target: exiting.then_some(id),
                class,
                fun,
            });
        }
        ctx.conditions.handlers.extend(handlers.into_iter().rev());
        if exiting {
            ctx.eval_try_expr(args, &matched, call)
        } else {
            ctx.eval_arg(args, &matched, 0, call, SEXP_MISSING)
        }
    })();
    if let Err(Unwind::Transfer {
        target,
        kind: Transfer::ExitingHandler { handler, cond },
    }) = result
        && target == id
        && let Some(entry) = ctx.conditions.handlers.get(handler)
    {
        let fun = entry.fun;
        ctx.conditions.handlers.truncate(base);
        result = ctx.apply(fun, &[(None, cond)], SEXP_NIL, call.env);
    }
    ctx.conditions.handlers.truncate(base);
    if let Some(finally) = finally {
        ctx.protect_result(&result);
        let visible = ctx.visible;
        ctx.eval(finally, call.env)?;
        ctx.visible = visible;
    }
    result
}

pub(super) fn try_eval(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["expr", "silent", "outFile"], call.err_call())?;
    let id = ctx.next_frame();
    let base = ctx.conditions.handlers.len();
    ctx.conditions.handlers.push(Handler {
        target: Some(id),
        class: "error".to_owned(),
        fun: SEXP_NIL,
    });
    let mut traced = matched;
    traced.dots.push(0);
    let result = ctx.eval_try_expr(args, &traced, call);
    ctx.conditions.handlers.truncate(base);
    let Err(Unwind::Transfer {
        target,
        kind: Transfer::ExitingHandler { cond, .. },
    }) = result
    else {
        return result;
    };
    if target != id {
        return result;
    }
    ctx.protect(cond);
    let silent = ctx.eval_flag(args, &traced, 1, call, false)?;
    let fields = ctx.list_args(cond)?;
    let error_call = fields.named(&ctx.session, "call").unwrap_or(SEXP_NIL);
    let message = fields.named(&ctx.session, "message").unwrap_or(SEXP_NIL);
    let message = ctx.concat_message(&Args::from_pairs(vec![(None, message)]), &[])?;
    let text = if error_call == SEXP_NIL {
        format!("Error : {message}\n")
    } else {
        let display_call = if ctx
            .session
            .cons_parts(error_call)
            .is_ok_and(|(head, _, _)| ctx.session.sym_name_of(head).ok() == Some("doTryCatch"))
        {
            call.expr
        } else {
            error_call
        };
        let lines = ctx.deparse_lines(display_call, DeparseOpts::default())?;
        diag_format::error(Some(&lines), &message)
    };
    ctx.conditions.last_error.clone_from(&text);
    let value = ctx.session.scalar_str(&text)?;
    ctx.protect(value);
    if !silent {
        let out = ctx.eval_arg(args, &traced, 2, call, SEXP_NIL)?;
        if out == SEXP_NIL {
            let _ = ctx.host.write_err(text.as_bytes());
        } else {
            let file = ctx.session.intern("file")?;
            ctx.call_function(
                "cat",
                &[(None, value), (Some(file), out)],
                roxide_value::BASE_ENV,
            )?;
        }
    }
    let class = ctx.session.scalar_str("try-error")?;
    let class_tag = ctx.session.intern("class")?;
    ctx.session.set_attrib(value, class_tag, class)?;
    let condition_tag = ctx.session.intern("condition")?;
    ctx.session.set_attrib(value, condition_tag, cond)?;
    ctx.visible = false;
    Ok(value)
}

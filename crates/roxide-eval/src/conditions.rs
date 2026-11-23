use roxide_diag::format as diag_format;
use roxide_gc::{SexpId, Tracer};
use roxide_value::{
    DeparseOpts, FormatOpts, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi, format::format_real_scalar,
};

use crate::control::logical;
use crate::{Args, Arity, Call, Ctx, Dispatch, EvalResult, FrameId, Module, Transfer, Unwind};

pub(crate) const MODULE: Module = Module {
    prims: &[
        primitive!(
            "simpleError",
            simple_condition,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "simpleWarning",
            simple_condition,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "simpleMessage",
            simple_condition,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "conditionMessage",
            condition_field,
            Eval,
            Arity::Exact(1),
            On,
            Dispatch::Internal("conditionMessage")
        ),
        primitive!(
            "conditionCall",
            condition_field,
            Eval,
            Arity::Exact(1),
            On,
            Dispatch::Internal("conditionCall")
        ),
        primitive!("stop", notify, Eval, Arity::Any, Off),
        primitive!("warning", notify, Eval, Arity::Any, Off),
        primitive!("message", notify, Eval, Arity::Any, Off),
        primitive!("signalCondition", signal, Eval, Arity::Exact(1), Off),
        primitive!("tryCatch", with_handlers, Unevaluated, Arity::Any, SetByFn),
        primitive!(
            "withCallingHandlers",
            with_handlers,
            Unevaluated,
            Arity::Any,
            SetByFn
        ),
        primitive!(
            "withRestarts",
            with_restarts,
            Unevaluated,
            Arity::Any,
            SetByFn
        ),
        primitive!("invokeRestart", invoke_restart, Eval, Arity::Any, SetByFn),
        primitive!("findRestart", find_restart, Eval, Arity::Range(1, 2), On),
        primitive!("options", options, Eval, Arity::Any, SetByFn),
        primitive!("getOption", get_option, Eval, Arity::Range(1, 2), On),
        primitive!("stopifnot", stop_if_not, Unevaluated, Arity::Any, Off),
    ],
    prelude: "",
};

struct Handler {
    target: Option<FrameId>,
    class: String,
    fun: SexpId,
}

struct Restart {
    target: FrameId,
    name: String,
    value: SexpId,
    fun: SexpId,
}

#[derive(Default)]
pub(crate) struct Conditions {
    handlers: Vec<Handler>,
    restarts: Vec<Restart>,
    pub(crate) unhandled: Option<String>,
}

impl Conditions {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for handler in &self.handlers {
            tracer.edge(handler.fun);
        }
        for restart in &self.restarts {
            tracer.edge(restart.fun);
            tracer.edge(restart.value);
        }
    }
}

impl Ctx {
    fn simple_condition(&mut self, kind: &str, message: SexpId, call: SexpId) -> EvalResult {
        let class = match kind {
            "error" => "simpleError",
            "warning" => "simpleWarning",
            _ => "simpleMessage",
        };
        let classes = self.strings([class, kind, "condition"])?;
        let message_tag = self.session.intern("message")?;
        let call_tag = self.session.intern("call")?;
        let cond = self.list(&Args::from_pairs(vec![
            (Some(message_tag), message),
            (Some(call_tag), call),
        ]))?;
        let tag = self.session.intern("class")?;
        self.session.set_attrib(cond, tag, classes)?;
        Ok(cond)
    }

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

    pub(crate) fn error_condition(&mut self, call: SexpId, text: String) -> Unwind {
        let result = self.scoped(|ctx| {
            ctx.protect(call);
            let message = ctx.session.scalar_str(&text)?;
            let cond = ctx.simple_condition("error", message, call)?;
            ctx.raise_condition(cond, call, &text)
        });
        match result {
            Err(err) => err,
            Ok(()) => Unwind::TopLevel,
        }
    }

    fn raise_condition(&mut self, cond: SexpId, call: SexpId, text: &str) -> EvalResult<()> {
        self.signal_condition(cond)?;
        let call_lines = if call == SEXP_NIL {
            None
        } else {
            self.deparse_lines(call, DeparseOpts::default()).ok()
        };
        let formatted = diag_format::error(call_lines.as_deref(), text);
        if self.expr_depth == 0 && self.frames.is_empty() {
            self.flush_warnings();
            let _ = self.host.write_err(formatted.as_bytes());
        } else {
            self.conditions.unhandled = Some(formatted);
        }
        Err(Unwind::TopLevel)
    }

    pub(crate) fn finish_error<T>(&mut self, result: EvalResult<T>) -> EvalResult<T> {
        if self.expr_depth == 0
            && self.frames.is_empty()
            && let Some(message) = self.conditions.unhandled.take()
            && matches!(result, Err(Unwind::TopLevel))
        {
            let _ = self.host.write_err(message.as_bytes());
            self.flush_warnings();
        }
        result
    }

    pub(crate) fn emit_condition(
        &mut self,
        kind: &str,
        call: SexpId,
        text: String,
        immediate: bool,
    ) -> EvalResult<()> {
        self.scoped(|ctx| {
            ctx.protect(call);
            let message = ctx.session.scalar_str(&text)?;
            let cond = ctx.simple_condition(kind, message, call)?;
            ctx.emit_existing(kind, cond, call, text, immediate)
        })
    }

    fn emit_existing(
        &mut self,
        kind: &str,
        cond: SexpId,
        call: SexpId,
        text: String,
        immediate: bool,
    ) -> EvalResult<()> {
        self.scoped(|ctx| {
            ctx.protect(cond);
            ctx.protect(call);
            let id = ctx.next_frame();
            let name = if kind == "warning" {
                "muffleWarning"
            } else {
                "muffleMessage"
            };
            ctx.push_restart(id, name.to_owned(), SEXP_NIL)?;
            let result = ctx.signal_condition(cond);
            ctx.conditions.restarts.pop();
            match result {
                Err(Unwind::Transfer {
                    target,
                    kind: Transfer::Restart { .. },
                }) if target == id => return Ok(()),
                other => other?,
            }
            if kind == "message" {
                let _ = ctx.host.write_err(text.as_bytes());
            } else {
                match ctx.opts.warn {
                    value if value < 0 => {}
                    value if value >= 2 => {
                        return Err(ctx.stop(call, format!("(converted from warning) {text}")));
                    }
                    value if value == 1 || immediate => {
                        let prefix = if call == SEXP_NIL {
                            "Warning: ".to_owned()
                        } else {
                            let call = ctx.deparse_lines(call, DeparseOpts::default())?.join(" ");
                            format!("Warning in {call} :\n  ")
                        };
                        let _ = ctx.host.write_err(format!("{prefix}{text}\n").as_bytes());
                    }
                    _ => ctx.warn(call, text),
                }
            }
            Ok(())
        })
    }

    fn push_restart(&mut self, target: FrameId, name: String, fun: SexpId) -> EvalResult<()> {
        let value = self.session.scalar_str(&name)?;
        let restart = self.list(&Args::from_pairs(vec![(None, value), (None, fun)]))?;
        let class = self.session.scalar_str("restart")?;
        let tag = self.session.intern("class")?;
        self.session.set_attrib(restart, tag, class)?;
        self.conditions.restarts.push(Restart {
            target,
            name,
            value: restart,
            fun,
        });
        Ok(())
    }

    pub(crate) fn option_value(&mut self, name: &str) -> EvalResult<Option<SexpId>> {
        let tag = self.session.intern(name)?;
        if let Some(&(_, value)) = self.options.iter().find(|&&(key, _)| key == tag) {
            return Ok(Some(value));
        }
        Ok(Some(match name {
            "digits" => self.session.scalar_int(self.opts.digits as i32)?,
            "scipen" => self.session.scalar_int(self.opts.scipen)?,
            "warn" => self.session.scalar_int(self.opts.warn)?,
            "expressions" => self.session.scalar_int(self.opts.expressions as i32)?,
            "width" => self.session.scalar_int(self.opts.width as i32)?,
            "warnPartialMatchArgs" => self
                .session
                .scalar_lgl(logical(self.opts.warn_partial_match_args))?,
            "warnPartialMatchDollar" => self
                .session
                .scalar_lgl(logical(self.opts.warn_partial_match_dollar))?,
            "keep.source" => self.session.scalar_lgl(logical(self.interactive))?,
            _ => return Ok(None),
        }))
    }

    fn set_option(&mut self, name: &str, value: SexpId, call: &Call) -> EvalResult {
        let old = self.option_value(name)?.unwrap_or(SEXP_NIL);
        self.protect(old);
        match name {
            "digits" => self.opts.digits = self.option_int(name, value, 1, 22, call)? as usize,
            "scipen" => self.opts.scipen = self.option_int(name, value, -999, 999, call)?,
            "warn" => self.opts.warn = self.integer(value, call.err_call())?,
            "expressions" => {
                self.opts.expressions = self.option_int(name, value, 25, 500_000, call)? as usize
            }
            "width" => self.opts.width = self.option_int(name, value, 10, 10_000, call)? as usize,
            "warnPartialMatchArgs" => {
                self.opts.warn_partial_match_args =
                    value != SEXP_NIL && self.truth(value, call.err_call())?
            }
            "warnPartialMatchDollar" => {
                self.opts.warn_partial_match_dollar =
                    value != SEXP_NIL && self.truth(value, call.err_call())?
            }
            _ => {}
        }
        let tag = self.session.intern(name)?;
        if let Some(i) = self.options.iter().position(|&(key, _)| key == tag) {
            if value == SEXP_NIL {
                self.options.remove(i);
            } else {
                self.options[i].1 = value;
            }
        } else if value != SEXP_NIL {
            self.options.push((tag, value));
        }
        Ok(old)
    }

    fn option_int(
        &mut self,
        name: &str,
        value: SexpId,
        lo: i32,
        hi: i32,
        call: &Call,
    ) -> EvalResult<i32> {
        let value = self.integer(value, call.err_call())?;
        if !(lo..=hi).contains(&value) {
            return Err(self.stop(
                call.err_call(),
                format!("invalid '{name}' parameter, allowed {lo}...{hi}"),
            ));
        }
        Ok(value)
    }

    fn concat_message(&mut self, args: &Args, skip: &[&str]) -> EvalResult<String> {
        let mut text = String::new();
        for (tag, value) in args.iter() {
            if tag.is_some_and(|tag| {
                self.session
                    .sym_name(tag)
                    .is_ok_and(|name| skip.contains(&name))
            }) {
                continue;
            }
            if value == SEXP_NIL {
                continue;
            }
            if let Ok(strings) = self.session.str_slice(value) {
                for &id in strings {
                    text.push_str(&String::from_utf8_lossy(self.session.char_bytes(id)?));
                }
            } else if self.session.type_of(value)? == SexpType::Real {
                let opts = FormatOpts {
                    digits: self.opts.digits,
                    scipen: self.opts.scipen,
                };
                for number in self.session.real(value)?.iter() {
                    text.push_str(&format_real_scalar(number, opts));
                }
            } else {
                let (strings, _) = self.session.coerce(value, SexpType::Str)?;
                for &id in self.session.str_slice(strings)? {
                    text.push_str(&String::from_utf8_lossy(self.session.char_bytes(id)?));
                }
            }
        }
        Ok(text)
    }
}

fn simple_condition(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["message", "call"], call.err_call())?;
    let name = ctx.prims[usize::from(call.prim.0)].name;
    let kind = match name {
        "simpleError" => "error",
        "simpleWarning" => "warning",
        _ => "message",
    };
    let msg = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let msg = if kind == "message" {
        msg
    } else {
        ctx.session.coerce(msg, SexpType::Str)?.0
    };
    ctx.simple_condition(
        kind,
        msg,
        ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL),
    )
}

fn condition_field(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = ctx.prims[usize::from(call.prim.0)].name;
    let field = if name == "conditionMessage" {
        "message"
    } else {
        "call"
    };
    let args = ctx.list_args(args.get(0))?;
    Ok(args.named(&ctx.session, field).unwrap_or(SEXP_NIL))
}

fn notify(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = ctx.prims[usize::from(call.prim.0)].name;
    let kind = if name == "stop" { "error" } else { name };
    let object = args.get(0);
    if args.len() == 1
        && ctx
            .classes(object, false)?
            .iter()
            .any(|class| class == "condition")
    {
        let values = ctx.list_args(object)?;
        let err_call = values.named(&ctx.session, "call").unwrap_or(SEXP_NIL);
        let message = ctx.concat_message(
            &Args::from_pairs(vec![(
                None,
                values.named(&ctx.session, "message").unwrap_or(SEXP_NIL),
            )]),
            &[],
        )?;
        if kind == "error" {
            ctx.raise_condition(object, err_call, &message)?;
        } else {
            ctx.emit_existing(kind, object, err_call, message, false)?;
        }
        return Ok(SEXP_NIL);
    }
    let (skip, err_call) = if kind == "message" {
        (&["appendLF", "domain"][..], call.expr)
    } else {
        let show = match args.named(&ctx.session, "call.") {
            Some(value) => ctx.truth(value, call.err_call())?,
            None => true,
        };
        let err_call = if show {
            ctx.frame(call.env).map_or(SEXP_NIL, |frame| frame.call)
        } else {
            SEXP_NIL
        };
        (
            &["call.", "immediate.", "noBreaks.", "domain"][..],
            err_call,
        )
    };
    let mut text = ctx.concat_message(args, skip)?;
    if kind == "message" {
        let append = match args.named(&ctx.session, "appendLF") {
            Some(value) => ctx.truth(value, call.err_call())?,
            None => true,
        };
        if append {
            text.push('\n');
        }
    }
    if kind == "error" {
        return Err(ctx.stop(err_call, text));
    }
    let immediate = match args.named(&ctx.session, "immediate.") {
        Some(value) => ctx.truth(value, call.err_call())?,
        None => false,
    };
    let value = if kind == "warning" {
        ctx.session.scalar_str(&text)?
    } else {
        SEXP_NIL
    };
    ctx.protect(value);
    ctx.emit_condition(kind, err_call, text, immediate)?;
    Ok(value)
}

fn signal(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    let cond = if ctx.session.type_of(value)? == SexpType::Str {
        let class_tag = ctx.session.intern("class")?;
        let message_tag = ctx.session.intern("message")?;
        let call_tag = ctx.session.intern("call")?;
        let cond = ctx.list(&Args::from_pairs(vec![
            (Some(message_tag), value),
            (Some(call_tag), SEXP_NIL),
        ]))?;
        let class = ctx.strings(["simpleCondition", "condition"])?;
        ctx.session.set_attrib(cond, class_tag, class)?;
        cond
    } else if ctx
        .classes(value, false)?
        .iter()
        .any(|class| class == "condition")
    {
        value
    } else {
        return Err(ctx.stop(call.err_call(), "argument must be a condition object"));
    };
    ctx.signal_condition(cond)?;
    Ok(SEXP_NIL)
}

fn with_handlers(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let exiting = ctx.prims[usize::from(call.prim.0)].name == "tryCatch";
    let names: &[&str] = if exiting {
        &["expr", "...", "finally"]
    } else {
        &["expr", "..."]
    };
    let args = &ctx.promise_args(args, call.env, call.err_call())?;
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
        ctx.eval_arg(args, &matched, 0, call, SEXP_MISSING)
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

fn with_restarts(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let args = &ctx.promise_args(args, call.env, call.err_call())?;
    for &value in args.values() {
        ctx.protect(value);
    }
    let matched = ctx.match_names(args, &["expr", "..."], call.err_call())?;
    let id = ctx.next_frame();
    let base = ctx.conditions.restarts.len();
    let mut restarts = Vec::new();
    for i in matched.dots.iter().copied() {
        let Some(tag) = args.name(i) else {
            return Err(ctx.stop(call.err_call(), "restarts must be named"));
        };
        let name = ctx.session.sym_name(tag)?.to_owned();
        let fun = ctx.eval(args.get(i), call.env)?;
        ctx.protect(fun);
        restarts.push((name, fun));
    }
    let result = (|| {
        for (name, fun) in restarts.into_iter().rev() {
            ctx.push_restart(id, name, fun)?;
        }
        ctx.eval_arg(args, &matched, 0, call, SEXP_MISSING)
    })();
    let fun = match result {
        Err(Unwind::Transfer {
            target,
            kind: Transfer::Restart { restart, .. },
        }) if target == id => ctx.conditions.restarts[base..]
            .iter()
            .find(|entry| entry.value == restart)
            .map(|entry| entry.fun),
        _ => None,
    };
    ctx.conditions.restarts.truncate(base);
    if let (
        Some(fun),
        Err(Unwind::Transfer {
            kind: Transfer::Restart { args, .. },
            ..
        }),
    ) = (fun, &result)
    {
        let pairs: Vec<_> = ctx.session.pairlist_iter(*args).collect();
        return ctx.apply(fun, &pairs, SEXP_NIL, call.env);
    }
    result
}

fn find_restart(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = ctx.string(args.get(0), call.err_call())?;
    Ok(ctx
        .conditions
        .restarts
        .iter()
        .rfind(|entry| entry.name == name)
        .map_or(SEXP_NIL, |entry| entry.value))
}

fn invoke_restart(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.is_empty() {
        return Err(ctx.stop(
            call.err_call(),
            "argument \"r\" is missing, with no default",
        ));
    }
    let r = args.get(0);
    let found = if ctx.session.type_of(r)? == SexpType::Str {
        let name = ctx.string(r, call.err_call())?;
        ctx.conditions
            .restarts
            .iter()
            .rfind(|entry| entry.name == name)
    } else {
        ctx.conditions
            .restarts
            .iter()
            .rfind(|entry| entry.value == r)
    };
    let Some(entry) = found else {
        return Err(ctx.stop(call.err_call(), "no such restart"));
    };
    let (target, restart) = (entry.target, entry.value);
    let args = ctx.pairlist(&Args::from_pairs(args.iter().skip(1).collect()), false)?;
    Err(Unwind::Transfer {
        target,
        kind: Transfer::Restart { restart, args },
    })
}

fn options(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.is_empty() {
        let mut tags: Vec<_> = ctx.options.iter().map(|&(tag, _)| tag).collect();
        for name in [
            "digits",
            "scipen",
            "warn",
            "expressions",
            "width",
            "keep.source",
            "warnPartialMatchArgs",
            "warnPartialMatchDollar",
        ] {
            let tag = ctx.session.intern(name)?;
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        tags.sort_by_key(|tag| ctx.session.sym_name(*tag).unwrap_or_default());
        let mut values = Vec::new();
        for tag in tags {
            let name = ctx.session.sym_name(tag)?.to_owned();
            values.push((Some(tag), ctx.option_value(&name)?.unwrap_or(SEXP_NIL)));
        }
        return ctx.list(&Args::from_pairs(values));
    }
    let mut visible = true;
    let mut out = Vec::new();
    for (tag, value) in args.iter() {
        if let Some(tag) = tag {
            let name = ctx.session.sym_name(tag)?.to_owned();
            out.push((Some(tag), ctx.set_option(&name, value, call)?));
            visible = false;
        } else if ctx.session.type_of(value)? == SexpType::Vec {
            for (tag, value) in ctx.list_args(value)?.iter() {
                let Some(tag) = tag else {
                    return Err(ctx.stop(call.err_call(), "invalid argument"));
                };
                let name = ctx.session.sym_name(tag)?.to_owned();
                out.push((Some(tag), ctx.set_option(&name, value, call)?));
            }
            visible = false;
        } else {
            let name = ctx.string(value, call.err_call())?;
            let tag = ctx.session.intern(&name)?;
            out.push((Some(tag), ctx.option_value(&name)?.unwrap_or(SEXP_NIL)));
        }
        if let Some((_, value)) = out.last() {
            ctx.protect(*value);
        }
    }
    ctx.visible = visible;
    ctx.list(&Args::from_pairs(out))
}

fn get_option(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["x", "default"], call.err_call())?;
    let x = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let name = ctx.string(x, call.err_call())?;
    Ok(ctx
        .option_value(&name)?
        .unwrap_or_else(|| ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL)))
}

fn stop_if_not(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["...", "exprs", "local"], call.err_call())?;
    let mut items = Vec::new();
    let explicit = ctx.matched_arg(args, &matched, 1);
    let mut env = call.env;
    if let Some(expr) = explicit {
        if !matched.dots.is_empty() {
            return Err(ctx.stop(
                call.err_call(),
                "must specify either '...' or 'exprs', but not both",
            ));
        }
        env = ctx.eval_arg(args, &matched, 2, call, call.env)?;
        ctx.protect(env);
        if ctx.session.type_of(expr)? == SexpType::Lang
            && let (head, tail, _) = ctx.session.cons_parts(expr)?
            && ctx.session.sym_name_of(head).ok() == Some("{")
        {
            items.extend(ctx.session.pairlist_iter(tail));
        } else {
            items.push((None, expr));
        }
    } else {
        let raw = Args::from_pairs(
            matched
                .dots
                .iter()
                .map(|&i| (args.name(i), args.get(i)))
                .collect(),
        );
        let promises = ctx.promise_args(&raw, env, call.err_call())?;
        for &value in promises.values() {
            ctx.protect(value);
        }
        items.extend(promises.iter());
    }
    for (tag, expr) in items {
        let value = ctx.eval(expr, env)?;
        if !ctx
            .session
            .lgl(value)
            .is_ok_and(|values| values.iter().all(|v| v.get() == Some(true)))
        {
            let message = if let Some(tag) = tag {
                ctx.session.sym_name(tag)?.to_owned()
            } else {
                let expr = ctx.promise_expr(expr)?;
                let text = ctx.deparse_lines(expr, Default::default())?.join(" ");
                format!("{text} is not TRUE")
            };
            return Err(ctx.stop(SEXP_NIL, message));
        }
    }
    Ok(SEXP_NIL)
}

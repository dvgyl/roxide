use roxide_diag::format as diag_format;
use roxide_gc::SexpId;
use roxide_value::{
    DeparseOpts, FormatOpts, SEXP_NIL, SexpType, ValueApi, format::format_real_scalar,
};

use crate::{Args, Call, Ctx, EvalResult, Transfer, Unwind};

impl Ctx {
    /// Return the latest error text. The default handler formats the text for display.
    pub fn last_error_message(&self) -> &str {
        &self.conditions.last_error
    }

    /// Set whether the default error handler writes to stderr. Return the old setting.
    pub fn set_show_error_messages(&mut self, show: bool) -> bool {
        std::mem::replace(&mut self.show_error_messages, show)
    }

    /// Signal a native interrupt. Return only if a handler invokes the resume restart.
    pub fn signal_native_interrupt(&mut self, resumable: bool) -> EvalResult<()> {
        let condition = self.session.alloc_list(0)?;
        let classes = self.strings(["interrupt", "condition"])?;
        let class = self.session.intern("class")?;
        self.session.set_attrib(condition, class, classes)?;
        if !resumable {
            self.signal_condition(condition)?;
            return Err(Unwind::TopLevel);
        }

        let target = self.next_frame();
        let base = self.conditions.restarts.len();
        self.push_restart(target, String::from("resume"), SEXP_NIL)?;
        let result = self.signal_condition(condition);
        self.conditions.restarts.truncate(base);
        match result {
            Err(Unwind::Transfer {
                target: dest,
                kind: Transfer::Restart { .. },
            }) if dest == target => Ok(()),
            other => other.and(Err(Unwind::TopLevel)),
        }
    }

    /// Signal a simple error and return its unwind.
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

    /// Signal an error with the given classes before the standard error classes.
    pub fn stop_with_class(
        &mut self,
        call: SexpId,
        text: impl Into<String>,
        classes: &[&str],
    ) -> Unwind {
        let text = text.into();
        let result = self.scoped(|ctx| {
            ctx.protect(call);
            let message = ctx.session.scalar_str(&text)?;
            let cond = ctx.make_condition(
                message,
                call,
                classes.iter().copied().chain(["error", "condition"]),
            )?;
            ctx.raise_condition(cond, call, &text)
        });
        match result {
            Err(err) => err,
            Ok(()) => Unwind::TopLevel,
        }
    }

    /// Signal a `packageNotFoundError` condition carrying `package` and `lib.loc` fields.
    pub fn stop_package_not_found(
        &mut self,
        call: SexpId,
        package: &str,
        lib_loc: SexpId,
    ) -> Unwind {
        let text = format!("there is no package called ‘{package}’");
        let result = self.scoped(|ctx| {
            ctx.protect(call);
            ctx.protect(lib_loc);
            let message = ctx.session.scalar_str(&text)?;
            ctx.protect(message);
            let pkg = ctx.session.scalar_str(package)?;
            ctx.protect(pkg);
            let classes = ctx.strings(["packageNotFoundError", "error", "condition"])?;
            ctx.protect(classes);
            let message_tag = ctx.session.intern("message")?;
            let call_tag = ctx.session.intern("call")?;
            let pkg_tag = ctx.session.intern("package")?;
            let lib_tag = ctx.session.intern("lib.loc")?;
            let cond = ctx.list(&Args::from_pairs(vec![
                (Some(message_tag), message),
                (Some(call_tag), call),
                (Some(pkg_tag), pkg),
                (Some(lib_tag), lib_loc),
            ]))?;
            ctx.protect(cond);
            let tag = ctx.session.intern("class")?;
            ctx.session.set_attrib(cond, tag, classes)?;
            ctx.raise_condition(cond, call, &text)
        });
        match result {
            Err(err) => err,
            Ok(()) => Unwind::TopLevel,
        }
    }

    fn raise_condition(&mut self, cond: SexpId, call: SexpId, text: &str) -> EvalResult<()> {
        self.conditions.last_error = text.to_owned();
        self.signal_condition(cond)?;
        let call_lines = if call == SEXP_NIL {
            None
        } else {
            self.deparse_lines(call, DeparseOpts::default()).ok()
        };
        let formatted = diag_format::error(call_lines.as_deref(), text);
        self.conditions.last_error = formatted;
        if self.show_error_messages {
            let _ = self.host.write_err(self.conditions.last_error.as_bytes());
            if !self.interactive && call != SEXP_NIL {
                self.print_call_trace(call);
            }
        }
        if !self.pending_warnings.is_empty() {
            if self.show_error_messages {
                let _ = self.host.write_err(b"In addition: ");
            }
            self.flush_warnings();
        }
        Err(Unwind::TopLevel)
    }

    fn print_call_trace(&mut self, error_call: SexpId) {
        let mut names = self.calls.iter().filter_map(|&(call, _)| {
            let (head, _, _) = self.session.cons_parts(call).ok()?;
            let name = self.session.sym_name_of(head).unwrap_or("<Anonymous>");
            Some((call, name))
        });
        let Some((first, name)) = names.next() else {
            return;
        };
        let mut names = names.peekable();
        if first == error_call && names.peek().is_none() {
            return;
        }
        let mut trace = format!("Calls: {name}");
        for (_, name) in names {
            trace.push_str(" -> ");
            trace.push_str(name);
        }
        trace.push('\n');
        let _ = self.host.write_err(trace.as_bytes());
    }

    /// Signal an expression stack overflow.
    pub(crate) fn stack_overflow_condition(&mut self) -> Unwind {
        const MESSAGE: &str =
            "evaluation nested too deeply: infinite recursion / options(expressions=)?";
        let result = self.scoped(|ctx| {
            let msg = ctx.session.scalar_str(MESSAGE)?;
            let condition = ctx.make_condition(
                msg,
                SEXP_NIL,
                [
                    "expressionStackOverflowError",
                    "stackOverflowError",
                    "error",
                    "condition",
                ],
            )?;
            ctx.raise_condition(condition, SEXP_NIL, MESSAGE)
        });
        result.err().unwrap_or(Unwind::TopLevel)
    }

    /// Signal a simple warning or message. Apply default handling if handlers let it proceed.
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

    pub(super) fn concat_message(&mut self, args: &Args, skip: &[&str]) -> EvalResult<String> {
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

pub(super) fn notify(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = ctx.prims[usize::from(call.prim.0)].name;
    let kind = match name {
        "stop" => "error",
        "packageStartupMessage" => "message",
        name => name,
    };
    let startup = name == "packageStartupMessage"
        || (name == "message" && {
            // The package loader's base prelude routes startup messages through message().
            let symbol = ctx.session.intern("packageStartupMessage")?;
            ctx.session
                .env_get(roxide_value::BASE_ENV, symbol)
                .is_some_and(|binding| {
                    ctx.frame(call.env)
                        .is_some_and(|frame| frame.fun == binding.value)
                })
        });
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
            ctx.calls
                .iter()
                .rfind(|&&(_, env)| env == call.env)
                .map_or(SEXP_NIL, |&(expr, _)| expr)
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
    if startup {
        let msg = ctx.session.scalar_str(&text)?;
        let condition = ctx.make_condition(
            msg,
            err_call,
            [
                "packageStartupMessage",
                "simpleMessage",
                "message",
                "condition",
            ],
        )?;
        ctx.emit_existing("message", condition, err_call, text, false)?;
    } else {
        ctx.emit_condition(kind, err_call, text, immediate)?;
    }
    Ok(value)
}

pub(super) fn signal(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

pub(super) fn stop_if_not(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

pub(super) fn get_error_message(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    Ok(ctx.session.scalar_str(&ctx.conditions.last_error)?)
}

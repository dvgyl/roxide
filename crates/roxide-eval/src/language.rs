use std::collections::HashSet;

use roxide_diag::{Span, SrcFile};
use roxide_gc::SexpId;
use roxide_syntax::{ParseStep, Parser};
use roxide_value::{
    BASE_ENV, CharId, DeparseOpts, Encoding, GLOBAL_ENV, KeepSource, PromiseState, SEXP_MISSING,
    SEXP_NIL, SexpType, ValueApi, lower,
};

use crate::closure::{Matches, dots_index};
use crate::control::logical;
use crate::ctx::Frame;
use crate::eval::ArgInput;
use crate::{Args, Arity, Call, Ctx, EvalResult, Module};

pub(crate) const MODULE: Module = Module {
    prims: &[
        primitive!("function", closure, Unevaluated, Arity::Range(2, 3), On),
        primitive!("quote", quote, Unevaluated, Arity::Exact(1), On),
        primitive!("missing", missing, Unevaluated, Arity::Exact(1), On),
        primitive!(
            "substitute",
            substitute,
            Unevaluated,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "match.call",
            match_call,
            Unevaluated,
            Arity::Range(0, 4),
            On
        ),
        primitive!("...length", dots_length, Eval, Arity::Exact(0), On),
        primitive!("...elt", dots_element, Eval, Arity::Exact(1), On),
        primitive!("eval", evaluate, Unevaluated, Arity::Range(1, 3), SetByFn),
        primitive!("evalq", evaluate, Unevaluated, Arity::Range(1, 3), SetByFn),
        primitive!("local", evaluate, Unevaluated, Arity::Range(1, 2), SetByFn),
        primitive!("force", force, Eval, Arity::Exact(1), On),
        primitive!(
            "withVisible",
            with_visible,
            Unevaluated,
            Arity::Exact(1),
            On
        ),
        primitive!("parse", parse, Eval, Arity::Any, On),
        primitive!("deparse", deparse, Eval, Arity::Any, On),
        primitive!("deparse1", deparse, Eval, Arity::Any, On),
        primitive!("do.call", do_call, Eval, Arity::Range(2, 4), SetByFn),
        primitive!(".Internal", internal, Unevaluated, Arity::Exact(1), SetByFn),
    ],
    prelude: "",
};

impl Ctx {
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

    pub(crate) fn string(&mut self, value: SexpId, call: SexpId) -> EvalResult<String> {
        if let Ok([id]) = self.session.str_slice(value)
            && *id != CharId::NA
        {
            return Ok(String::from_utf8_lossy(self.session.char_bytes(*id)?).into_owned());
        }
        Err(self.stop(call, "invalid character argument"))
    }

    pub(crate) fn name(&mut self, value: SexpId, call: SexpId) -> EvalResult<String> {
        match self.session.sym_name_of(value) {
            Ok(name) => Ok(name.to_owned()),
            Err(_) => self.string(value, call),
        }
    }

    pub(crate) fn integer(&mut self, value: SexpId, call: SexpId) -> EvalResult<i32> {
        let (int, _) = self.session.coerce(value, SexpType::Int)?;
        if self.session.xlength(int)? != 1
            || self.session.int(int)?.get(0) == roxide_value::NA_INTEGER
        {
            return Err(self.stop(call, "invalid integer argument"));
        }
        Ok(self.session.int(int)?.get(0))
    }

    pub(crate) fn strings<'a>(&mut self, values: impl IntoIterator<Item = &'a str>) -> EvalResult {
        let strings = values
            .into_iter()
            .map(|value| self.session.mk_char(value.as_bytes(), Encoding::Native))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.session.alloc_str_from(&strings)?)
    }

    pub(crate) fn list(&mut self, args: &Args) -> EvalResult {
        let list = self.session.alloc_list(args.len())?;
        for (i, &value) in args.values().iter().enumerate() {
            self.session.list_set(list, i, value)?;
        }
        if args.iter().any(|(tag, _)| tag.is_some()) {
            let tags = args
                .iter()
                .map(|(tag, _)| tag.map_or(Ok(""), |tag| self.session.sym_name(tag)))
                .collect::<Result<Vec<_>, _>>()?;
            let tags: Vec<_> = tags.iter().map(|tag| (*tag).to_owned()).collect();
            let names = self.strings(tags.iter().map(String::as_str))?;
            let names_sym = self.session.intern("names")?;
            self.session.set_attrib(list, names_sym, names)?;
        }
        Ok(list)
    }

    pub(crate) fn list_args(&mut self, list: SexpId) -> EvalResult<Args> {
        if self.session.type_of(list)? == SexpType::Vec {
            let names_sym = self.session.intern("names")?;
            let names = self.session.attr(list, names_sym);
            let tags = if names == SEXP_NIL {
                Vec::new()
            } else {
                self.session.str_slice(names)?.to_vec()
            };
            let mut pairs = Vec::new();
            for i in 0..self.session.xlength(list)? {
                let tag = if let Some(&name) = tags.get(i) {
                    let name = self.session.char_bytes(name)?;
                    if name.is_empty() || tags[i] == CharId::NA {
                        None
                    } else {
                        let name = String::from_utf8_lossy(name).into_owned();
                        Some(self.session.intern(&name)?)
                    }
                } else {
                    None
                };
                pairs.push((tag, self.session.list_get(list, i)?));
            }
            Ok(Args::from_pairs(pairs))
        } else {
            Ok(Args::from_pairs(self.session.pairlist_iter(list).collect()))
        }
    }
}

fn closure(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    Ok(ctx
        .session
        .alloc_closure(args.get(0), args.get(1), call.env)?)
}

fn quote(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if let Some(tag) = args.name(0) {
        let name = ctx.session.sym_name(tag)?;
        if name != "expr" {
            return Err(ctx.stop(
                call.err_call(),
                format!("supplied argument name '{name}' does not match 'expr'"),
            ));
        }
    }
    Ok(args.get(0))
}

fn force(_: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    Ok(args.get(0))
}

fn with_visible(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = ctx.eval(args.get(0), call.env)?;
    let visible = ctx.session.scalar_lgl(logical(ctx.visible))?;
    let value_tag = ctx.session.intern("value")?;
    let visible_tag = ctx.session.intern("visible")?;
    ctx.list(&Args::from_pairs(vec![
        (Some(value_tag), value),
        (Some(visible_tag), visible),
    ]))
}

fn dots_length(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
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

fn dots_element(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn missing(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn substitute(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["expr", "env"], call.err_call())?;
    let expr = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_MISSING);
    let env = ctx.eval_arg(args, &matched, 1, call, call.env)?;
    let frame = match ctx.session.type_of(env)? {
        SexpType::Env => env,
        SexpType::Vec | SexpType::List => ctx.data_env(env, roxide_value::EMPTY_ENV)?,
        SexpType::Nil => roxide_value::EMPTY_ENV,
        _ => return Err(ctx.stop(call.err_call(), "invalid environment specified")),
    };
    substitute_expr(ctx, expr, frame, 0, call.err_call())
}

fn substitute_expr(
    ctx: &mut Ctx,
    expr: SexpId,
    frame: SexpId,
    depth: usize,
    call: SexpId,
) -> EvalResult {
    if depth >= 128 {
        return Err(ctx.stop(call, "substitute depth limit exceeded"));
    }
    match ctx.session.type_of(expr)? {
        SexpType::Prom => ctx.promise_expr(expr),
        SexpType::Sym if expr != SEXP_MISSING && frame != GLOBAL_ENV => {
            let sym = ctx.sym_from_sexp(expr)?;
            let Some(binding) = ctx.session.env_get(frame, sym) else {
                return Ok(expr);
            };
            let name = ctx.session.sym_name(sym)?;
            if name == "..." {
                return if binding.value == SEXP_MISSING {
                    Ok(SEXP_NIL)
                } else {
                    let first = ctx
                        .session
                        .pairlist_iter(binding.value)
                        .next()
                        .map_or(SEXP_NIL, |(_, val)| val);
                    ctx.promise_expr(first)
                };
            }
            ctx.promise_expr(binding.value)
        }
        ty @ (SexpType::Lang | SexpType::List) => {
            let (head, tail, _) = ctx.session.cons_parts(expr)?;
            let (head, tail) = if ty == SexpType::Lang {
                (
                    Some(substitute_expr(ctx, head, frame, depth + 1, call)?),
                    tail,
                )
            } else {
                (None, expr)
            };
            let args = Args::from_pairs(ctx.session.pairlist_iter(tail).collect());
            let dots = ctx.session.intern("...")?;
            let mut pairs = Vec::new();
            for (tag, expr) in args.iter() {
                if expr == dots.as_sexp()
                    && frame != GLOBAL_ENV
                    && let Some(binding) = ctx.session.env_get(frame, dots)
                {
                    for (tag, value) in ctx.session.pairlist_iter(binding.value) {
                        pairs.push((tag, ctx.promise_expr(value)?));
                    }
                } else {
                    pairs.push((tag, substitute_expr(ctx, expr, frame, depth + 1, call)?));
                }
            }
            let pairs = Args::from_pairs(pairs);
            if let Some(head) = head {
                ctx.make_call(head, &pairs)
            } else {
                ctx.pairlist(&pairs, false)
            }
        }
        _ => Ok(expr),
    }
}

fn match_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn evaluate(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let kind = ctx.prims[usize::from(call.prim.0)].name;
    let mut names = vec!["expr", "envir"];
    if kind != "local" {
        names.push("enclos");
    }
    let matched = ctx.match_names(args, &names, call.err_call())?;
    let Some(mut expr) = ctx.matched_arg(args, &matched, 0) else {
        return Err(ctx.stop(
            call.err_call(),
            "argument \"expr\" is missing, with no default",
        ));
    };
    if kind == "eval" {
        expr = ctx.eval(expr, call.env)?;
    }
    ctx.protect(expr);
    let env = if kind == "local" && ctx.matched_arg(args, &matched, 1).is_none() {
        ctx.session.alloc_env(call.env)?
    } else {
        ctx.eval_arg(args, &matched, 1, call, call.env)?
    };
    ctx.protect(env);
    let parent = if kind == "local" {
        call.env
    } else {
        ctx.eval_arg(args, &matched, 2, call, call.env)?
    };
    let parent = if parent == SEXP_NIL { BASE_ENV } else { parent };
    let env = match ctx.session.type_of(env)? {
        SexpType::Nil | SexpType::Vec | SexpType::List => ctx.data_env(env, parent)?,
        SexpType::Int | SexpType::Real if kind != "local" => ctx.eval_env_at(env, call)?,
        _ => ctx.require_env(env, call.err_call())?,
    };
    ctx.protect(env);
    let id = ctx.next_frame();
    let frame = Frame {
        id,
        cloenv: env,
        call: call.expr,
        on_exit: Vec::new(),
        fun: SEXP_NIL,
        caller: call.env,
        actuals: args.clone(),
        dispatch: None,
    };
    ctx.run_frame(frame, |ctx| {
        if ctx.session.type_of(expr)? == SexpType::Expr {
            let mut value = SEXP_NIL;
            for i in 0..ctx.session.xlength(expr)? {
                value = ctx.eval(ctx.session.list_get(expr, i)?, env)?;
            }
            Ok(value)
        } else {
            ctx.eval(expr, env)
        }
    })
}

fn parse(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(
        args,
        &[
            "file",
            "n",
            "text",
            "prompt",
            "keep.source",
            "srcfile",
            "encoding",
        ],
        call.err_call(),
    )?;
    let n = ctx
        .matched_arg(args, &matched, 1)
        .map(|value| ctx.integer(value, call.err_call()))
        .transpose()?
        .unwrap_or(-1);
    let keep_source = match ctx.matched_arg(args, &matched, 4) {
        Some(value) => ctx.truth(value, call.err_call())?,
        None => ctx
            .option_value("keep.source")?
            .is_some_and(|value| value == roxide_value::TRUE_SCALAR),
    };
    let mut text = ctx.matched_arg(args, &matched, 2).unwrap_or(SEXP_NIL);
    let mut source_name = String::from("<text>");
    if text == SEXP_NIL {
        let Some(file) = ctx
            .matched_arg(args, &matched, 0)
            .filter(|&file| file != SEXP_NIL)
        else {
            return Ok(ctx.session.alloc_expr_from(&[])?);
        };
        if ctx.session.type_of(file)? == SexpType::Str {
            let slice = ctx.session.str_slice(file)?;
            if slice.len() == 1 && slice[0] != CharId::NA {
                let name = String::from_utf8_lossy(ctx.session.char_bytes(slice[0])?).into_owned();
                if name.is_empty() {
                    return Ok(ctx.session.alloc_expr_from(&[])?);
                }
                source_name = name;
            }
        }
        let read_sym = ctx.session.intern("readLines")?;
        let read_fun = ctx.find_fun(read_sym, BASE_ENV)?;
        let warn_sym = ctx.session.intern("warn")?;
        text = ctx.apply(
            read_fun,
            &[(None, file), (Some(warn_sym), roxide_value::FALSE_SCALAR)],
            call.err_call(),
            call.env,
        )?;
    }
    let lines = ctx
        .session
        .str_slice(text)?
        .iter()
        .map(|&line| {
            if line == CharId::NA {
                return Ok("NA".to_owned());
            }
            Ok(String::from_utf8_lossy(ctx.session.char_bytes(line)?).into_owned())
        })
        .collect::<EvalResult<Vec<_>>>()?;
    let source = lines.join("\n");
    let file_id = ctx.sources.add_file(&source_name, source.as_str());
    let mut parser = Parser::new(&source, file_id);
    let mut expressions = Vec::new();
    let mut spans = Vec::new();
    while n < 0 || expressions.len() < n as usize {
        match parser.parse_next() {
            ParseStep::Eof => break,
            ParseStep::Incomplete => {
                return Err(ctx.stop(call.err_call(), "unexpected end of input"));
            }
            ParseStep::Error(err) => return Err(ctx.stop(call.err_call(), err.message)),
            ParseStep::Expr(node) => {
                spans.push(parser.ast().span(node));
                expressions.push(lower(parser.ast(), node, &mut ctx.session, KeepSource::No)?);
            }
        }
    }
    let expr = ctx.session.alloc_expr_from(&expressions)?;
    if keep_source {
        add_source(ctx, expr, &source, &lines, &spans)?;
    }
    Ok(expr)
}

fn add_source(
    ctx: &mut Ctx,
    expr: SexpId,
    source: &str,
    lines: &[String],
    spans: &[Span],
) -> EvalResult<()> {
    let file = SrcFile::new("<text>", source);
    let srcfile = ctx.session.alloc_env(roxide_value::EMPTY_ENV)?;
    let class_sym = ctx.session.intern("class")?;
    let srcfile_sym = ctx.session.intern("srcfile")?;
    let srcref_sym = ctx.session.intern("srcref")?;
    let whole_sym = ctx.session.intern("wholeSrcref")?;
    let classes = ctx.strings(["srcfilecopy", "srcfile"])?;
    ctx.session.set_attrib(srcfile, class_sym, classes)?;
    let lines_value = ctx.strings(lines.iter().map(String::as_str))?;
    let lines_sym = ctx.session.intern("lines")?;
    let filename_sym = ctx.session.intern("filename")?;
    let filename = ctx.session.scalar_str("<text>")?;
    ctx.assign(lines_sym, lines_value, srcfile)?;
    ctx.assign(filename_sym, filename, srcfile)?;
    let class = ctx.session.scalar_str("srcref")?;
    let mut reference = |lo, hi| -> EvalResult {
        let srcref = ctx.session.alloc_int_from(&file.srcref8(lo, hi))?;
        ctx.session.set_attrib(srcref, class_sym, class)?;
        ctx.session.set_attrib(srcref, srcfile_sym, srcfile)?;
        Ok(srcref)
    };
    let pairs = spans
        .iter()
        .map(|span| Ok((None, reference(span.lo, span.hi)?)))
        .collect::<EvalResult<Vec<_>>>()?;
    let whole = reference(0, source.len() as u32)?;
    let list = ctx.list(&Args::from_pairs(pairs))?;
    ctx.session.set_attrib(expr, srcref_sym, list)?;
    ctx.session.set_attrib(expr, srcfile_sym, srcfile)?;
    ctx.session.set_attrib(expr, whole_sym, whole)?;
    Ok(())
}

fn deparse(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let one_line = ctx.prims[usize::from(call.prim.0)].name == "deparse1";
    let extra = if one_line { "collapse" } else { "nlines" };
    let matched = ctx.match_names(
        args,
        &[
            "expr",
            if one_line { extra } else { "width.cutoff" },
            if one_line { "width.cutoff" } else { "backtick" },
            "control",
            if one_line { "backtick" } else { extra },
        ],
        call.err_call(),
    )?;
    let expr = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let mut opts = DeparseOpts {
        width_cutoff: if one_line { 500 } else { 60 },
        ..DeparseOpts::default()
    };
    if let Some(value) = ctx.matched_arg(args, &matched, if one_line { 2 } else { 1 }) {
        opts.width_cutoff = ctx.integer(value, call.err_call())?.clamp(20, 500) as usize;
    }
    if let Some(value) = ctx.matched_arg(args, &matched, 3) {
        let control = ctx
            .session
            .str_slice(value)?
            .iter()
            .map(|&value| ctx.session.char_bytes(value))
            .collect::<Result<Vec<_>, _>>()?;
        opts.keep_integer =
            control.contains(&b"keepInteger".as_slice()) || control.contains(&b"all".as_slice());
        opts.keep_na =
            control.contains(&b"keepNA".as_slice()) || control.contains(&b"all".as_slice());
        opts.digits17 = control.contains(&b"digits17".as_slice());
    }
    let mut lines = ctx.deparse_lines(expr, opts)?;
    if one_line {
        let separator = ctx
            .matched_arg(args, &matched, 1)
            .map(|value| ctx.string(value, call.err_call()))
            .transpose()?;
        return Ok(ctx
            .session
            .scalar_str(&lines.join(separator.as_deref().unwrap_or(" ")))?);
    }
    if let Some(value) = ctx.matched_arg(args, &matched, 4) {
        let n = ctx.integer(value, call.err_call())?;
        if n >= 0 {
            lines.truncate(n as usize);
        }
    }
    ctx.strings(lines.iter().map(String::as_str))
}

fn do_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["what", "args", "quote", "envir"], call.err_call())?;
    let what = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let list = ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL);
    let env = ctx.matched_arg(args, &matched, 3).unwrap_or(call.env);
    ctx.require_env(env, call.err_call())?;
    let quote = match ctx.matched_arg(args, &matched, 2) {
        Some(value) => ctx.truth(value, call.err_call())?,
        None => false,
    };
    if !matches!(ctx.session.type_of(list)?, SexpType::Vec | SexpType::Nil) {
        return Err(ctx.stop(call.err_call(), "second argument must be a list"));
    }
    let args = ctx.list_args(list)?;
    let head = if ctx.session.type_of(what)? == SexpType::Str {
        let name = ctx.string(what, call.err_call())?;
        ctx.session.intern(&name)?.as_sexp()
    } else {
        what
    };
    let args = if quote {
        let q = ctx.session.intern("quote")?.as_sexp();
        let pairs = args
            .iter()
            .map(|(tag, value)| {
                Ok((
                    tag,
                    ctx.make_call(q, &Args::from_pairs(vec![(None, value)]))?,
                ))
            })
            .collect::<EvalResult<Vec<_>>>()?;
        Args::from_pairs(pairs)
    } else {
        args
    };
    let expr = ctx.make_call(head, &args)?;
    ctx.eval(expr, env)
}

fn internal(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let expr = args.get(0);
    if ctx.session.type_of(expr)? != SexpType::Lang {
        return Err(ctx.stop(call.err_call(), "invalid .Internal() argument"));
    }
    let (head, tail, _) = ctx.session.cons_parts(expr)?;
    let name = ctx.session.sym_name_of(head)?.to_owned();
    let Some(&prim) = ctx.internals.get(name.as_str()) else {
        return Err(ctx.stop(
            call.err_call(),
            format!("there is no .Internal function '{name}'"),
        ));
    };
    let user_call = ctx
        .frame(call.env)
        .map_or(call.err_call(), |frame| frame.call);
    let call = Call {
        expr,
        prim,
        env: call.env,
        user_call,
    };
    ctx.run_primitive(
        call,
        Args::from_pairs(ctx.session.pairlist_iter(tail).collect()),
        ArgInput::Expressions,
        true,
    )
}

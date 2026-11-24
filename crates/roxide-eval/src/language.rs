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
        primitive!("formals", function_parts, Eval, Arity::Exact(1), On),
        primitive!(
            "formals<-",
            set_function_parts,
            Eval,
            Arity::Range(2, 3),
            On
        ),
        primitive!("body", function_parts, Eval, Arity::Exact(1), On),
        primitive!("body<-", set_function_parts, Eval, Arity::Range(2, 3), On),
        primitive!("args", function_parts, Eval, Arity::Exact(1), On),
        primitive!("alist", alist, Unevaluated, Arity::Any, On),
        primitive!("as.pairlist", as_pairlist, Eval, Arity::Exact(1), On),
        primitive!("as.function", as_function, Eval, Arity::Range(1, 2), On),
        primitive!("call", construct_call, EvalKeepMissing, Arity::Any, On),
        primitive!("as.call", as_call, Eval, Arity::Exact(1), On),
        primitive!("bquote", bquote, Unevaluated, Arity::Range(1, 3), On),
        primitive!("match.arg", match_arg, Eval, Arity::Range(1, 3), On),
        primitive!("match.fun", match_fun, Eval, Arity::Range(1, 2), On),
        primitive!("Recall", recall, Unevaluated, Arity::Any, SetByFn),
        primitive!("nargs", nargs, Eval, Arity::Exact(0), On),
        primitive!("str2lang", str_to_language, Eval, Arity::Exact(1), On),
        primitive!("str2expression", str_to_language, Eval, Arity::Exact(1), On),
        primitive!(
            "eval.parent",
            eval_parent,
            Eval,
            Arity::Range(1, 2),
            SetByFn
        ),
        primitive!(
            "forceAndCall",
            force_and_call,
            Unevaluated,
            Arity::Any,
            SetByFn
        ),
        primitive!("switch", switch, Unevaluated, Arity::Any, SetByFn),
        primitive!(
            "is.primitive",
            language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!("is.call", language_predicate, Eval, Arity::Exact(1), On),
        primitive!("is.name", language_predicate, Eval, Arity::Exact(1), On),
        primitive!("is.symbol", language_predicate, Eval, Arity::Exact(1), On),
        primitive!("is.pairlist", language_predicate, Eval, Arity::Exact(1), On),
        primitive!(
            "is.expression",
            language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!("is.language", language_predicate, Eval, Arity::Exact(1), On),
        primitive!(
            "is.recursive",
            language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!("is.object", language_predicate, Eval, Arity::Exact(1), On),
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
    prelude: r#"
identity <- function(x) x
force <- function(x) x
Negate <- function(f) {
    f <- match.fun(f)
    function(...) !f(...)
}
Vectorize <- function(FUN, vectorize.args = arg.names, SIMPLIFY = TRUE, USE.NAMES = TRUE) {
    FUN <- match.fun(FUN)
    arg.names <- names(formals(FUN))
    if (is.null(arg.names)) stop("FUN must be a function with formal arguments")
    if (!is.character(vectorize.args) && !is.null(vectorize.args))
        stop("'vectorize.args' must be a character vector")
    if (length(vectorize.args) == 0L) return(FUN)
    includes <- function(x, choices) vapply(x, function(name) any(name == choices), FALSE)
    if (!all(includes(vectorize.args, arg.names)))
        stop("must specify formal argument names in 'vectorize.args'")
    if (any(includes(vectorize.args, c("FUN", "SIMPLIFY", "USE.NAMES", "MoreArgs"))))
        stop("'FUN', 'SIMPLIFY', 'USE.NAMES' and 'MoreArgs' cannot be vectorized")
    wrapped <- function() {
        supplied <- as.list(match.call(expand.dots = FALSE))[-1L]
        values <- lapply(supplied, eval, envir = parent.frame())
        varying <- includes(names(values), vectorize.args)
        do.call(mapply, c(list(FUN = FUN), values[varying],
                          list(MoreArgs = values[!varying], SIMPLIFY = SIMPLIFY,
                               USE.NAMES = USE.NAMES)), quote = TRUE)
    }
    formals(wrapped) <- formals(FUN)
    wrapped
}
ifelse <- function(test, yes, no) {
    if (is.null(attributes(test)) && length(test) == 1L) {
        if (is.na(test)) return(NA)
        if (test && length(yes) == 1L && is.null(attributes(yes))) return(yes)
        if (!test && length(no) == 1L && is.null(attributes(no))) return(no)
    }
    if (!is.logical(test)) {
        saved <- attributes(test)
        test <- as.logical(test)
        attributes(test) <- saved
    }
    result <- test
    indices <- seq_along(test)
    positive <- indices[!is.na(test) & test]
    negative <- indices[!is.na(test) & !test]
    if (length(positive)) result[positive] <- rep(yes, length.out = length(test))[positive]
    if (length(negative)) result[negative] <- rep(no, length.out = length(test))[negative]
    result
}
"#,
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
        if matches!(self.session.type_of(list)?, SexpType::Vec | SexpType::Expr) {
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
        supplied: Vec::new(),
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
        let namespace = ctx.session.intern("::")?.as_sexp();
        let base = ctx.session.intern("base")?.as_sexp();
        let quote = ctx.session.intern("quote")?.as_sexp();
        let quote = ctx.make_call(
            namespace,
            &Args::from_pairs(vec![(None, base), (None, quote)]),
        )?;
        let pairs = args
            .iter()
            .map(|(tag, value)| {
                Ok((
                    tag,
                    ctx.make_call(quote, &Args::from_pairs(vec![(None, value)]))?,
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

fn function_parts(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let kind = ctx.prims[usize::from(call.prim.0)].name;
    let mut fun = args.get(0);
    if matches!(ctx.session.type_of(fun)?, SexpType::Str | SexpType::Sym) {
        let name = ctx.name(fun, call.err_call())?;
        let tag = ctx.session.intern(&name)?;
        fun = ctx.find_fun_at(tag, call.env, call.err_call())?;
    }
    let (formals, body) = if let Ok((formals, body, _)) = ctx.session.closure_parts(fun) {
        (formals, body)
    } else if kind == "args"
        && let Some(name) = ctx.primitive_name(fun)
    {
        let signature: &[&str] = match name {
            "sum" | "min" | "max" | "prod" | "any" | "all" => &["...", "na.rm"],
            "c" => &["..."],
            "rep" => &["x", "..."],
            "seq.int" => &["from", "to", "by", "length.out", "along.with", "..."],
            "log" => &["x", "base"],
            "round" | "signif" => &["x", "digits", "..."],
            "length" | "sqrt" | "abs" | "sin" | "cos" | "tan" | "exp" | "is.null" | "is.na"
            | "names" | "is.finite" | "is.infinite" => &["x"],
            _ => return Ok(SEXP_NIL),
        };
        let mut pairs = Vec::with_capacity(signature.len());
        for &name in signature {
            let default = match name {
                "na.rm" => roxide_value::FALSE_SCALAR,
                "base" => {
                    let head = ctx.session.intern("exp")?.as_sexp();
                    let value = ctx.session.scalar_real(1.0)?;
                    ctx.make_call(head, &Args::from_pairs(vec![(None, value)]))?
                }
                "digits" if ctx.primitive_name(fun) == Some("round") => {
                    ctx.session.scalar_real(0.0)?
                }
                "digits" => ctx.session.scalar_real(6.0)?,
                _ => SEXP_MISSING,
            };
            pairs.push((Some(ctx.session.intern(name)?), default));
        }
        (ctx.pairlist(&Args::from_pairs(pairs), false)?, SEXP_NIL)
    } else {
        return Ok(SEXP_NIL);
    };
    match kind {
        "args" => Ok(ctx.session.alloc_closure(formals, SEXP_NIL, GLOBAL_ENV)?),
        "formals" => Ok(formals),
        _ => Ok(body),
    }
}

fn set_function_parts(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["fun", "envir", "value"], call.err_call())?;
    let fun = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let (mut formals, mut body, env) = ctx
        .session
        .closure_parts(fun)
        .map_err(|_| ctx.stop(call.err_call(), "argument is not a function"))?;
    let (env, value) = if args.len() == 2 && args.named(&ctx.session, "value").is_none() {
        (env, args.get(1))
    } else {
        (
            ctx.matched_arg(args, &matched, 1).unwrap_or(env),
            ctx.matched_arg(args, &matched, 2).unwrap_or(SEXP_NIL),
        )
    };
    ctx.require_env(env, call.err_call())?;
    if ctx.prims[usize::from(call.prim.0)].name == "formals<-" {
        let pairs = ctx.list_args(value)?;
        if pairs.iter().any(|(tag, _)| tag.is_none())
            || !matches!(
                ctx.session.type_of(value)?,
                SexpType::Vec | SexpType::List | SexpType::Nil
            )
        {
            return Err(ctx.stop(
                call.err_call(),
                "invalid formal argument list for 'function'",
            ));
        }
        formals = ctx.pairlist(&pairs, false)?;
    } else {
        body = value;
    }
    Ok(ctx.session.alloc_closure(formals, body, env)?)
}

fn alist(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let args = ctx.expand_dots(args, call.env, call.err_call())?;
    let pairs = args
        .iter()
        .map(|(tag, value)| Ok((tag, ctx.promise_expr(value)?)))
        .collect::<EvalResult<Vec<_>>>()?;
    ctx.list(&Args::from_pairs(pairs))
}

fn as_pairlist(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut value = args.get(0);
    match ctx.session.type_of(value)? {
        SexpType::List | SexpType::Nil => return Ok(value),
        SexpType::Vec | SexpType::Expr => {}
        _ => {
            value = ctx
                .session
                .coerce(value, SexpType::Vec)
                .map_err(|_| ctx.stop(call.err_call(), "cannot coerce to pairlist"))?
                .0;
        }
    }
    let pairs = ctx.list_args(value)?;
    ctx.pairlist(&pairs, false)
}

fn as_function(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["x", "envir"], call.err_call())?;
    let value = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    if matches!(
        ctx.session.type_of(value)?,
        SexpType::Clo | SexpType::Builtin | SexpType::Special
    ) {
        return Ok(value);
    }
    let env = ctx.matched_arg(args, &matched, 1).unwrap_or(call.env);
    ctx.require_env(env, call.err_call())?;
    let mut pairs: Vec<_> = ctx.list_args(value)?.iter().collect();
    let Some((_, body)) = pairs.pop() else {
        return Err(ctx.stop(call.err_call(), "list argument expected"));
    };
    if pairs.iter().any(|(tag, _)| tag.is_none()) {
        return Err(ctx.stop(
            call.err_call(),
            "invalid formal argument list for 'function'",
        ));
    }
    let formals = ctx.pairlist(&Args::from_pairs(pairs), false)?;
    Ok(ctx.session.alloc_closure(formals, body, env)?)
}

fn construct_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = ctx.string(args.get(0), call.err_call())?;
    let head = ctx.session.intern(&name)?.as_sexp();
    ctx.make_call(head, &Args::from_pairs(args.iter().skip(1).collect()))
}

fn as_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    if ctx.session.type_of(value)? == SexpType::Lang {
        return Ok(value);
    }
    let pairs = ctx.list_args(value)?;
    if pairs.is_empty() {
        return Err(ctx.stop(call.err_call(), "invalid length 0 argument"));
    }
    ctx.make_call(
        pairs.get(0),
        &Args::from_pairs(pairs.iter().skip(1).collect()),
    )
}

fn bquote(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["expr", "where", "splice"], call.err_call())?;
    let target = ctx.eval_arg(args, &matched, 1, call, call.env)?;
    let env = match ctx.session.type_of(target)? {
        SexpType::Env => target,
        SexpType::Vec | SexpType::List | SexpType::Nil => ctx.data_env(target, call.env)?,
        _ => return Err(ctx.stop(call.err_call(), "invalid 'where' argument")),
    };
    ctx.protect(env);
    let splice = ctx.eval_flag(args, &matched, 2, call, false)?;
    let expr = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_MISSING);
    let expr = ctx.promise_expr(expr)?;
    bquote_expr(ctx, expr, env, splice, 0)
}

fn bquote_expr(ctx: &mut Ctx, expr: SexpId, env: SexpId, splice: bool, depth: usize) -> EvalResult {
    if depth >= ctx.opts.expressions.min(256) {
        return Err(ctx.stop(SEXP_NIL, "bquote expression is too deeply nested"));
    }
    let ty = ctx.session.type_of(expr)?;
    if !matches!(ty, SexpType::Lang | SexpType::List) {
        return Ok(expr);
    }
    if ty == SexpType::Lang {
        let (head, tail, _) = ctx.session.cons_parts(expr)?;
        match ctx.session.sym_name_of(head).ok() {
            Some(".") => return ctx.eval(ctx.session.cons_parts(tail)?.0, env),
            Some("..") if splice => {
                return Err(ctx.stop(SEXP_NIL, "can only splice inside a call"));
            }
            _ => {}
        }
    }
    let items: Vec<_> = ctx.session.pairlist_iter(expr).collect();
    let mut pairs = Vec::new();
    for (i, (tag, item)) in items.into_iter().enumerate() {
        if splice && (i > 0 || ty == SexpType::List) && ctx.session.type_of(item)? == SexpType::Lang
        {
            let (head, tail, _) = ctx.session.cons_parts(item)?;
            if ctx.session.sym_name_of(head).ok() == Some("..") {
                let value = ctx.eval(ctx.session.cons_parts(tail)?.0, env)?;
                ctx.protect(value);
                let value = match ctx.session.type_of(value)? {
                    SexpType::Vec | SexpType::Expr | SexpType::Nil => value,
                    SexpType::Lgl
                    | SexpType::Int
                    | SexpType::Real
                    | SexpType::Cplx
                    | SexpType::Str
                    | SexpType::Raw => ctx.session.coerce(value, SexpType::Vec)?.0,
                    _ => return Err(ctx.stop(SEXP_NIL, "can only splice vectors")),
                };
                ctx.protect(value);
                pairs.extend(ctx.list_args(value)?.iter());
                continue;
            }
        }
        let value = bquote_expr(ctx, item, env, splice, depth + 1)?;
        ctx.protect(value);
        pairs.push((tag, value));
    }
    let args = Args::from_pairs(pairs);
    if ty == SexpType::Lang {
        ctx.make_call(
            args.get(0),
            &Args::from_pairs(args.iter().skip(1).collect()),
        )
    } else {
        ctx.pairlist(&args, false)
    }
}

fn match_fun(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["FUN", "descend"], call.err_call())?;
    let fun = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    if matches!(
        ctx.session.type_of(fun)?,
        SexpType::Clo | SexpType::Builtin | SexpType::Special
    ) {
        return Ok(fun);
    }
    let name = ctx.name(fun, call.err_call())?;
    let tag = ctx.session.intern(&name)?;
    if let Some(descend) = ctx.matched_arg(args, &matched, 1)
        && !ctx.truth(descend, call.err_call())?
    {
        return ctx
            .get_var(tag, call.env)?
            .ok_or_else(|| ctx.stop(call.err_call(), format!("object '{name}' not found")));
    }
    ctx.find_fun_at(tag, call.env, call.err_call())
}

fn match_arg(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["arg", "choices", "several.ok"], call.err_call())?;
    let value = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let choices = if let Some(choices) = ctx.matched_arg(args, &matched, 1) {
        choices
    } else {
        let expr = ctx.session.cons_parts(call.expr)?.1;
        let raw = Args::from_pairs(ctx.session.pairlist_iter(expr).collect());
        let raw = ctx.matched_arg(&raw, &matched, 0).unwrap_or(SEXP_MISSING);
        let Ok(sym) = ctx.sym_from_sexp(raw) else {
            return Err(ctx.stop(
                call.err_call(),
                "argument \"choices\" is missing, with no default",
            ));
        };
        let Some(frame) = ctx.frame(call.env) else {
            return Err(ctx.stop(
                call.err_call(),
                "argument \"choices\" is missing, with no default",
            ));
        };
        let (formals, _, _) = ctx.session.closure_parts(frame.fun)?;
        let default = ctx
            .session
            .pairlist_iter(formals)
            .find_map(|(tag, value)| (tag == Some(sym)).then_some(value))
            .unwrap_or(SEXP_MISSING);
        ctx.eval(default, call.env)?
    };
    ctx.protect(choices);
    let several = match ctx.matched_arg(args, &matched, 2) {
        Some(several) => ctx.truth(several, call.err_call())?,
        None => false,
    };
    if ctx.session.type_of(choices)? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "'choices' must be a character vector"));
    }
    let choices = ctx.session.str_slice(choices)?.to_vec();
    let values = if value == SEXP_NIL {
        Vec::new()
    } else if ctx.session.type_of(value)? == SexpType::Str {
        ctx.session.str_slice(value)?.to_vec()
    } else {
        return Err(ctx.stop(call.err_call(), "'arg' must be NULL or a character vector"));
    };
    if value == SEXP_NIL || (!several && values == choices) {
        return Ok(ctx
            .session
            .alloc_str_from(&choices[..choices.len().min(1)])?);
    }
    if !several && values.len() != 1 {
        return Err(ctx.stop(call.err_call(), "'arg' must be of length 1"));
    }
    if several && values.is_empty() {
        return Err(ctx.stop(call.err_call(), "'arg' must be of length >= 1"));
    }
    let mut selected = Vec::new();
    for value in values {
        if choices.contains(&value) {
            selected.push(value);
        } else if value != CharId::NA {
            let prefix = ctx.session.char_bytes(value)?;
            let mut matches = choices.iter().filter(|&&id| {
                !prefix.is_empty()
                    && ctx
                        .session
                        .char_bytes(id)
                        .is_ok_and(|choice| choice.starts_with(prefix))
            });
            if let Some(&exact) = matches.next()
                && matches.next().is_none()
            {
                selected.push(exact);
            }
        }
    }
    if selected.is_empty() {
        let labels = choices
            .iter()
            .map(|&id| {
                Ok(format!(
                    "“{}”",
                    String::from_utf8_lossy(ctx.session.char_bytes(id)?)
                ))
            })
            .collect::<EvalResult<Vec<_>>>()?;
        return Err(ctx.stop(
            call.err_call(),
            format!("'arg' should be one of {}", labels.join(", ")),
        ));
    }
    Ok(ctx.session.alloc_str_from(&selected)?)
}

fn recall(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let Some(frame) = ctx
        .frame(call.env)
        .cloned()
        .filter(|frame| frame.fun != SEXP_NIL)
    else {
        return Err(ctx.stop(call.err_call(), "'Recall' called from outside a closure"));
    };
    let args = ctx.promise_args(args, call.env, call.err_call())?;
    ctx.invoke_closure(frame.fun, args, frame.call, call.env, frame.dispatch)
}

fn nargs(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    let Some(frame) = ctx.frame(call.env).filter(|frame| frame.fun != SEXP_NIL) else {
        return Ok(SEXP_NIL);
    };
    Ok(ctx.session.scalar_int(frame.actuals.len() as i32)?)
}

fn str_to_language(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let single = ctx.prims[usize::from(call.prim.0)].name == "str2lang";
    if ctx.session.type_of(args.get(0))? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "argument must be character"));
    }
    if single && ctx.session.xlength(args.get(0))? != 1 {
        return Err(ctx.stop(call.err_call(), "argument must be a character string"));
    }
    let text = ctx.session.intern("text")?;
    let source = ctx.session.intern("keep.source")?;
    let expr = parse(
        ctx,
        call,
        &mut Args::from_pairs(vec![
            (Some(text), args.get(0)),
            (Some(source), roxide_value::FALSE_SCALAR),
        ]),
    )?;
    if !single {
        return Ok(expr);
    }
    if ctx.session.xlength(expr)? != 1 {
        return Err(ctx.stop(
            call.err_call(),
            "parsing result not of length one, but 0 or > 1",
        ));
    }
    Ok(ctx.session.list_get(expr, 0)?)
}

fn eval_parent(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["expr", "n"], call.err_call())?;
    let expr = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let n = ctx.matched_arg(args, &matched, 1);
    let env = if n.is_some_and(|value| ctx.integer(value, call.err_call()).ok() == Some(0)) {
        call.env
    } else {
        let args = n.map_or_else(Vec::new, |value| vec![(None, value)]);
        let fun = ctx.session.intern("parent.frame")?;
        let fun = ctx.find_fun(fun, BASE_ENV)?;
        ctx.apply(fun, &args, call.err_call(), call.env)?
    };
    if ctx.session.type_of(expr)? == SexpType::Expr {
        let mut result = SEXP_NIL;
        for i in 0..ctx.session.xlength(expr)? {
            result = ctx.eval(ctx.session.list_get(expr, i)?, env)?;
        }
        Ok(result)
    } else {
        ctx.eval(expr, env)
    }
}

fn force_and_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.len() < 2 {
        return Err(ctx.stop(call.err_call(), "at least two arguments are required"));
    }
    let n = ctx.eval(args.get(0), call.env)?;
    let n = ctx.integer(n, call.err_call())?.max(0) as usize;
    let head = args.get(1);
    let fun = if ctx.session.type_of(head)? == SexpType::Sym {
        let tag = ctx.sym_from_sexp(head)?;
        ctx.find_fun_at(tag, call.env, call.err_call())?
    } else {
        ctx.eval(head, call.env)?
    };
    ctx.protect(fun);
    let args = Args::from_pairs(args.iter().skip(2).collect());
    let expr = ctx.make_call(head, &args)?;
    ctx.protect(expr);
    if ctx.session.type_of(fun)? != SexpType::Clo {
        return ctx.invoke_raw(fun, ctx.session.cons_parts(expr)?.1, expr, call.env);
    }
    let promises = ctx.promise_args(&args, call.env, call.err_call())?;
    for &value in promises.values() {
        ctx.protect(value);
    }
    for &value in promises.values().iter().take(n) {
        if value == SEXP_MISSING {
            return Err(ctx.stop(call.err_call(), "argument is missing, with no default"));
        }
        ctx.force(value)?;
    }
    ctx.invoke_closure(fun, promises, expr, call.env, None)
}

fn language_predicate(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    use SexpType::*;
    let value = args.get(0);
    let ty = ctx.session.type_of(value)?;
    let flag = match ctx.prims[usize::from(call.prim.0)].name {
        "is.primitive" => matches!(ty, Builtin | Special),
        "is.call" => ty == Lang,
        "is.name" | "is.symbol" => ty == Sym,
        "is.pairlist" => matches!(ty, List | Nil),
        "is.expression" => ty == Expr,
        "is.language" => matches!(ty, Sym | Lang | Expr),
        "is.recursive" => matches!(
            ty,
            Vec | List | Expr | Lang | Clo | Env | Prom | Dot | Builtin | Special
        ),
        _ => ctx.session.is_object(value)?,
    };
    Ok(ctx.session.scalar_lgl(logical(flag))?)
}

fn switch(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let args = ctx.expand_dots(args, call.env, call.err_call())?;
    for &value in args.values() {
        ctx.protect(value);
    }
    if args.is_empty() {
        return Err(ctx.stop(call.err_call(), "'EXPR' is missing"));
    }
    let expr = ctx.eval(args.get(0), call.env)?;
    if ctx.session.xlength(expr)? != 1 {
        return Err(ctx.stop(call.err_call(), "EXPR must be a length 1 vector"));
    }
    let mut selected = None;
    if args.len() == 1 {
        ctx.warn_now(call.err_call(), "'switch' with no alternatives")?;
    } else if ctx.session.type_of(expr)? == SexpType::Str {
        let id = ctx.session.str_slice(expr)?[0];
        let mut defaults = args
            .iter()
            .skip(1)
            .enumerate()
            .filter_map(|(i, (name, value))| name.is_none().then_some((i + 1, value)));
        for (i, (tag, _)) in args.iter().enumerate().skip(1) {
            if let Some(tag) = tag
                && id != CharId::NA
                && ctx.session.sym_name(tag)?.as_bytes() == ctx.session.char_bytes(id)?
            {
                selected = args.values()[i..]
                    .iter()
                    .copied()
                    .find(|&value| value != SEXP_MISSING);
                break;
            }
        }
        if let Some((_, first)) = defaults.next() {
            if let Some((_, second)) = defaults.next() {
                let deparse = |value| {
                    ctx.deparse_lines(value, DeparseOpts::default())
                        .map(|v| v.join(" "))
                };
                let message = format!(
                    "duplicate 'switch' defaults: '{}' and '{}'",
                    deparse(first)?,
                    deparse(second)?
                );
                return Err(ctx.stop(call.err_call(), message));
            }
            selected = selected.or(Some(first));
        }
    } else {
        let integer = ctx.session.coerce(expr, SexpType::Int)?.0;
        if let Some(index) = ctx.session.int(integer)?.iter().next()
            && index > 0
            && (index as usize) < args.len()
        {
            selected = Some(args.get(index as usize));
        }
    }
    if let Some(expr) = selected {
        if expr == SEXP_MISSING {
            return Err(ctx.stop(call.err_call(), "empty alternative in numeric switch"));
        }
        return ctx.eval(expr, call.env);
    }
    ctx.visible = false;
    Ok(SEXP_NIL)
}

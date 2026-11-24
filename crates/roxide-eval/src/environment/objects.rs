use roxide_gc::SexpId;
use roxide_value::{BASE_ENV, BASE_NAMESPACE, EMPTY_ENV, GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi};

use crate::{Args, Call, Ctx, EvalResult};

impl Ctx {
    /// Return the value if it is an environment.
    pub(crate) fn require_env(&mut self, value: SexpId, call: SexpId) -> EvalResult {
        if self.session.type_of(value)? == SexpType::Env {
            Ok(value)
        } else {
            Err(self.stop(call, "invalid 'envir' argument"))
        }
    }

    /// Bind named data entries in a new environment with the given parent.
    pub(crate) fn data_env(&mut self, data: SexpId, parent: SexpId) -> EvalResult {
        self.require_env(parent, SEXP_NIL)?;
        let env = self.session.alloc_env(parent)?;
        for (name, value) in self.list_args(data)?.iter() {
            if let Some(name) = name {
                self.assign(name, value, env)?;
            }
        }
        Ok(env)
    }

    pub(super) fn search_env(&mut self, position: SexpId, call: &Call) -> EvalResult {
        if self.session.type_of(position)? == SexpType::Env {
            return Ok(position);
        }
        let path = self.search_path()?;
        if self.session.type_of(position)? == SexpType::Str {
            let name = self.string(position, call.err_call())?;
            for &env in &path {
                if self.search_entry_name(env)? == name {
                    return Ok(env);
                }
            }
            return Err(self.stop(
                call.err_call(),
                format!("no item called \"{name}\" on the search list"),
            ));
        }
        match self.integer(position, call.err_call())? {
            -1 => Ok(call.env),
            n if n >= 1 && (n as usize) <= path.len() => Ok(path[n as usize - 1]),
            _ => Err(self.stop(call.err_call(), "invalid 'pos' argument")),
        }
    }

    /// Return the name that the search path uses for this environment.
    pub(crate) fn search_entry_name(&mut self, env: SexpId) -> EvalResult<String> {
        if env == GLOBAL_ENV {
            return Ok(String::from(".GlobalEnv"));
        }
        if env == BASE_ENV {
            return Ok(String::from("package:base"));
        }
        let name_sym = self.session.intern("name")?;
        let attr = self.session.attr(env, name_sym);
        if attr != SEXP_NIL
            && self.session.type_of(attr)? == SexpType::Str
            && let Ok(slice) = self.session.str_slice(attr)
            && let Some(&first) = slice.first()
            && first != roxide_value::CharId::NA
        {
            return Ok(String::from_utf8_lossy(self.session.char_bytes(first)?).into_owned());
        }
        Ok(String::new())
    }
}

pub(super) fn as_environment(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    match ctx.session.type_of(args.get(0))? {
        SexpType::Vec => populate_environment(ctx, call, args.get(0), SEXP_NIL, EMPTY_ENV),
        SexpType::Int | SexpType::Real if ctx.session.xlength(args.get(0))? > 1 => {
            let values = ctx.session.coerce(args.get(0), SexpType::Int)?.0;
            let values = ctx.session.int(values)?.to_vec();
            let mut result = Vec::with_capacity(values.len());
            for value in values {
                let position = ctx.session.scalar_int(value)?;
                result.push((None, ctx.search_env(position, call)?));
            }
            ctx.list(&Args::from_pairs(result))
        }
        _ => ctx.search_env(args.get(0), call),
    }
}

pub(super) fn list_to_env(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(
        args,
        &["x", "envir", "parent", "hash", "size"],
        call.err_call(),
    )?;
    let list = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let env = ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL);
    let parent = ctx.matched_arg(args, &matched, 2).unwrap_or(call.env);
    populate_environment(ctx, call, list, env, parent)
}

fn populate_environment(
    ctx: &mut Ctx,
    call: &Call,
    list: SexpId,
    env: SexpId,
    parent: SexpId,
) -> EvalResult {
    if ctx.session.type_of(list)? != SexpType::Vec {
        return Err(ctx.stop(call.err_call(), "first argument must be a named list"));
    }
    let pairs = ctx.list_args(list)?;
    let names = ctx.session.intern("names")?;
    if !pairs.is_empty() && ctx.session.attr(list, names) == SEXP_NIL {
        return Err(ctx.stop(
            call.err_call(),
            "names(x) must be a character vector of the same length as x",
        ));
    }
    if pairs.iter().any(|(tag, _)| tag.is_none()) {
        return Err(ctx.stop(call.err_call(), "attempt to use zero-length variable name"));
    }
    let env = if env == SEXP_NIL {
        ctx.require_env(parent, call.err_call())?;
        ctx.session.alloc_env(parent)?
    } else {
        ctx.require_env(env, call.err_call())?
    };
    ctx.protect(env);
    for (tag, value) in pairs.iter() {
        if let Some(tag) = tag {
            ctx.assign(tag, value, env)?;
        }
    }
    Ok(env)
}

pub(super) fn new_env(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["hash", "parent", "size"], call.err_call())?;
    let parent = ctx.matched_arg(args, &matched, 1).unwrap_or(call.env);
    ctx.require_env(parent, call.err_call())?;
    Ok(ctx.session.alloc_env(parent)?)
}

pub(super) fn constant_env(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    Ok(match ctx.prims[usize::from(call.prim.0)].name {
        "baseenv" => BASE_ENV,
        "emptyenv" => EMPTY_ENV,
        _ => GLOBAL_ENV,
    })
}

pub(super) fn environment(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let fun = args.get(0);
    match ctx.session.type_of(fun)? {
        SexpType::Nil => Ok(call.env),
        SexpType::Clo => Ok(ctx.session.closure_parts(fun)?.2),
        _ => {
            let sym = ctx.session.intern(".Environment")?;
            Ok(ctx.session.attr(fun, sym))
        }
    }
}

pub(super) fn set_env(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["fun", "value"], call.err_call())?;
    let fun = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let env = ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL);
    if ctx.session.type_of(fun)? == SexpType::Clo {
        ctx.require_env(env, call.err_call())?;
        let (formals, body, _) = ctx.session.closure_parts(fun)?;
        let copy = ctx.session.alloc_closure(formals, body, env)?;
        for (name, value) in ctx.session.attrs(fun)? {
            ctx.session.set_attrib(copy, name, value)?;
        }
        return Ok(copy);
    }
    if env != SEXP_NIL {
        ctx.require_env(env, call.err_call())?;
    }
    let sym = ctx.session.intern(".Environment")?;
    let copy = ctx.session.shallow_duplicate(fun)?;
    ctx.session.set_attrib(copy, sym, env)?;
    Ok(copy)
}

pub(super) fn parent_env(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = ctx.require_env(args.get(0), call.err_call())?;
    if env == EMPTY_ENV {
        return Err(ctx.stop(call.err_call(), "the empty environment has no parent"));
    }
    Ok(ctx.session.env_enclos(env)?)
}

pub(super) fn set_parent_env(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = ctx.require_env(args.get(0), call.err_call())?;
    let value = ctx.require_env(args.get(1), call.err_call())?;
    if env == EMPTY_ENV {
        return Err(ctx.stop(
            call.err_call(),
            "can not set parent of the empty environment",
        ));
    }
    ctx.session.env_set_enclos(env, value)?;
    Ok(env)
}

pub(super) fn search(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let path = ctx.search_path()?;
    let mut names = Vec::with_capacity(path.len());
    for env in path {
        names.push(ctx.search_entry_name(env)?);
    }
    ctx.strings(names.iter().map(String::as_str))
}

pub(super) fn topenv(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["envir", "matchThisEnv"], call.err_call())?;
    let mut env = match ctx.matched_arg(args, &matched, 0) {
        Some(v) if v != SEXP_NIL => ctx.require_env(v, call.err_call())?,
        _ => call.env,
    };
    let match_this = match ctx.matched_arg(args, &matched, 1) {
        Some(v) => v,
        None => ctx.option_value("topLevelEnvironment")?.unwrap_or(SEXP_NIL),
    };
    let ns_sym = ctx.session.intern(".__NAMESPACE__.")?;
    let pkg_sym = ctx.session.intern(".packageName")?;
    let mut guard = 0usize;
    while env != SEXP_NIL && guard < 1024 {
        if env == GLOBAL_ENV
            || env == BASE_ENV
            || env == BASE_NAMESPACE
            || env == EMPTY_ENV
            || (match_this != SEXP_NIL && env == match_this)
            || ctx.session.env_get(env, ns_sym).is_some()
            || ctx.session.env_get(env, pkg_sym).is_some()
        {
            return Ok(env);
        }
        env = ctx.session.env_enclos(env)?;
        guard += 1;
    }
    Ok(GLOBAL_ENV)
}

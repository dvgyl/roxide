use roxide_gc::SexpId;
use roxide_value::{
    BASE_ENV, GLOBAL_ENV, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, Sym, ValueApi,
};

use crate::control::logical;
use crate::{Args, Call, Ctx, EvalResult};

use super::{is_function, scalar_flag};

enum LookupResult {
    MissingBinding,
    Value(SexpId),
}

impl Ctx {
    fn binding_env(&self, sym: Sym, mut env: SexpId, inherits: bool) -> EvalResult<Option<SexpId>> {
        while env != SEXP_NIL {
            if self
                .session
                .env_get(env, sym)
                .is_some_and(|binding| binding.value != SEXP_UNBOUND)
            {
                return Ok(Some(env));
            }
            if !inherits {
                break;
            }
            env = self.session.env_enclos(env)?;
        }
        Ok(None)
    }

    fn lookup_mode(
        &mut self,
        sym: Sym,
        mut env: SexpId,
        mode: &str,
        inherits: bool,
        exists: bool,
    ) -> EvalResult<Option<LookupResult>> {
        while let Some(found) = self.binding_env(sym, env, inherits)? {
            if exists && mode == "any" {
                return Ok(Some(LookupResult::Value(SEXP_NIL)));
            }
            let Some(binding) = self.session.env_get(found, sym) else {
                return Ok(None);
            };
            if !binding.active && binding.value == SEXP_MISSING {
                return Ok(Some(LookupResult::MissingBinding));
            }
            let value = self.read_binding(binding, found)?;
            let ty = self.session.type_of(value)?;
            let compatible = match mode {
                "any" => true,
                "function" => matches!(ty, SexpType::Builtin | SexpType::Special | SexpType::Clo),
                "numeric" => matches!(ty, SexpType::Int | SexpType::Real),
                mode => ty.type_name(false) == mode,
            };
            if compatible {
                return Ok(Some(LookupResult::Value(value)));
            }
            if !inherits {
                break;
            }
            env = self.session.env_enclos(found)?;
        }
        Ok(None)
    }

    /// Return the names of the bindings in an environment.
    pub(crate) fn environment_names(&mut self, env: SexpId) -> EvalResult {
        let names = self
            .session
            .env_entries(env)?
            .into_iter()
            .map(|(tag, _)| Ok(self.session.sym_name(tag)?.to_owned()))
            .collect::<EvalResult<Vec<_>>>()?;
        self.strings(names.iter().map(String::as_str))
    }
}

pub(super) fn assign(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(
        args,
        &["x", "value", "pos", "envir", "inherits", "immediate"],
        call.err_call(),
    )?;
    let name = ctx.string(
        ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL),
        call.err_call(),
    )?;
    let Some(value) = ctx.matched_arg(args, &matched, 1) else {
        return Err(ctx.stop(
            call.err_call(),
            "argument \"value\" is missing, with no default",
        ));
    };
    let sym = ctx.session.intern(&name)?;
    let mut env = if let Some(env) = ctx.matched_arg(args, &matched, 3) {
        ctx.require_env(env, call.err_call())?
    } else if let Some(pos) = ctx.matched_arg(args, &matched, 2) {
        ctx.search_env(pos, call)?
    } else {
        call.env
    };
    let inherits = scalar_flag(ctx, ctx.matched_arg(args, &matched, 4), false, call)?;
    if inherits {
        env = ctx.binding_env(sym, env, true)?.unwrap_or(GLOBAL_ENV);
    }
    ctx.assign(sym, value, env)?;
    Ok(value)
}

pub(super) fn get(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let kind = ctx.prims[usize::from(call.prim.0)].name;
    let parameters = if kind == "get0" {
        ["x", "envir", "mode", "inherits", "ifnotfound"]
    } else {
        ["x", "pos", "envir", "mode", "inherits"]
    };
    let matched = ctx.match_names(args, &parameters, call.err_call())?;
    let arg = |i| ctx.matched_arg(args, &matched, i);
    let (name, position, envir, mode, inherit, fallback) = if kind == "get0" {
        (arg(0), None, arg(1), arg(2), arg(3), arg(4))
    } else {
        (arg(0), arg(1), arg(2), arg(3), arg(4), None)
    };
    let name = ctx.string(name.unwrap_or(SEXP_NIL), call.err_call())?;
    let sym = ctx.session.intern(&name)?;
    let env = if let Some(env) = envir {
        ctx.require_env(env, call.err_call())?
    } else if let Some(pos) = position {
        ctx.search_env(pos, call)?
    } else {
        call.env
    };
    let mode = mode
        .map(|value| ctx.string(value, call.err_call()))
        .transpose()?;
    let inherits = scalar_flag(ctx, inherit, true, call)?;
    let exists = kind == "exists";
    let found = ctx.lookup_mode(sym, env, mode.as_deref().unwrap_or("any"), inherits, exists)?;
    if exists {
        return Ok(ctx.session.scalar_lgl(logical(found.is_some()))?);
    }
    match found {
        Some(LookupResult::MissingBinding) => Err(ctx.stop(
            call.err_call(),
            format!("argument \"{name}\" is missing, with no default"),
        )),
        Some(LookupResult::Value(value)) => Ok(value),
        None if kind == "get0" => Ok(fallback.unwrap_or(SEXP_NIL)),
        None => {
            let mode = mode
                .filter(|mode| mode != "any")
                .map_or(String::new(), |mode| format!(" of mode '{mode}'"));
            Err(ctx.stop(call.err_call(), format!("object '{name}'{mode} not found")))
        }
    }
}

pub(super) fn mget(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(
        args,
        &["x", "envir", "mode", "ifnotfound", "inherits"],
        call.err_call(),
    )?;
    let names = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let env = ctx.matched_arg(args, &matched, 1).unwrap_or(call.env);
    ctx.require_env(env, call.err_call())?;
    let modes = ctx.matched_arg(args, &matched, 2);
    let fallback = ctx.matched_arg(args, &matched, 3);
    let inherits = scalar_flag(ctx, ctx.matched_arg(args, &matched, 4), false, call)?;
    let keys = ctx.session.str_slice(names)?.to_vec();
    let modes = match modes {
        Some(value) => ctx.session.str_slice(value)?.to_vec(),
        None => Vec::new(),
    };
    let fallback = fallback
        .map(|value| {
            let value = ctx.session.coerce(value, SexpType::Vec)?.0;
            ctx.protect(value);
            ctx.list_args(value)
        })
        .transpose()?;
    if (!modes.is_empty() && modes.len() != 1 && modes.len() != keys.len())
        || fallback
            .as_ref()
            .is_some_and(|v| v.len() != 1 && v.len() != keys.len())
    {
        return Err(ctx.stop(
            call.err_call(),
            "wrong length for 'mode' or 'ifnotfound' argument",
        ));
    }
    let result = ctx.session.alloc_list(keys.len())?;
    ctx.protect(result);
    for (i, key) in keys.into_iter().enumerate() {
        let name = String::from_utf8_lossy(ctx.session.char_bytes(key)?).into_owned();
        let sym = ctx.session.intern(&name)?;
        let mode = if modes.is_empty() {
            "any".to_owned()
        } else {
            String::from_utf8_lossy(ctx.session.char_bytes(modes[i % modes.len()])?).into_owned()
        };
        let value = match ctx.lookup_mode(sym, env, &mode, inherits, false)? {
            Some(LookupResult::Value(value)) => value,
            Some(LookupResult::MissingBinding) => SEXP_MISSING,
            None => {
                let Some(fallback) = &fallback else {
                    return Err(ctx.stop(call.err_call(), format!("value for ‘{name}’ not found")));
                };
                let value = fallback.get(i % fallback.len());
                if is_function(ctx, value)? {
                    let key = ctx.session.alloc_str_from(&[key])?;
                    ctx.apply(value, &[(None, key)], SEXP_NIL, call.env)?
                } else {
                    value
                }
            }
        };
        ctx.session.list_set(result, i, value)?;
    }
    let tag = ctx.session.intern("names")?;
    ctx.session.set_attrib(result, tag, names)?;
    Ok(result)
}

pub(super) fn delayed_assign(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(
        args,
        &["x", "value", "eval.env", "assign.env"],
        call.err_call(),
    )?;
    let name = ctx.eval_arg(args, &matched, 0, call, SEXP_NIL)?;
    let name = ctx.string(name, call.err_call())?;
    let sym = ctx.session.intern(&name)?;
    let eval_env = ctx.eval_arg(args, &matched, 2, call, call.env)?;
    let assign_env = ctx.eval_arg(args, &matched, 3, call, call.env)?;
    ctx.require_env(eval_env, call.err_call())?;
    ctx.require_env(assign_env, call.err_call())?;
    let value = ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_MISSING);
    let promise = ctx.session.alloc_promise(value, eval_env)?;
    ctx.assign(sym, promise, assign_env)?;
    Ok(SEXP_NIL)
}

pub(super) fn remove(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(
        args,
        &["...", "list", "pos", "envir", "inherits"],
        call.err_call(),
    )?;
    let list = ctx.eval_arg(args, &matched, 1, call, SEXP_NIL)?;
    let mut names = if list == SEXP_NIL {
        Vec::new()
    } else {
        ctx.session
            .str_slice(list)?
            .iter()
            .map(|&id| Ok(String::from_utf8_lossy(ctx.session.char_bytes(id)?).into_owned()))
            .collect::<EvalResult<Vec<_>>>()?
    };
    for i in matched.dots.iter().copied() {
        names.push(ctx.name(args.get(i), call.err_call())?);
    }
    let env = if ctx.matched_arg(args, &matched, 3).is_some() {
        let env = ctx.eval_arg(args, &matched, 3, call, call.env)?;
        ctx.require_env(env, call.err_call())?
    } else if ctx.matched_arg(args, &matched, 2).is_some() {
        let pos = ctx.eval_arg(args, &matched, 2, call, SEXP_NIL)?;
        ctx.search_env(pos, call)?
    } else {
        call.env
    };
    ctx.protect(env);
    let inherits = ctx.eval_flag(args, &matched, 4, call, false)?;
    for name in names {
        let sym = ctx.session.intern(&name)?;
        if let Some(found) = ctx.binding_env(sym, env, inherits)? {
            ctx.session.env_remove(found, sym)?;
        } else {
            ctx.warn_now(call.err_call(), format!("object '{name}' not found"))?;
        }
    }
    Ok(SEXP_NIL)
}

pub(super) fn ls(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(
        args,
        &["name", "pos", "envir", "all.names", "pattern", "sorted"],
        call.err_call(),
    )?;
    let env = if let Some(env) = ctx.matched_arg(args, &matched, 2) {
        ctx.require_env(env, call.err_call())?
    } else if let Some(pos) = ctx
        .matched_arg(args, &matched, 0)
        .or_else(|| ctx.matched_arg(args, &matched, 1))
    {
        ctx.search_env(pos, call)?
    } else {
        call.env
    };
    let all = scalar_flag(ctx, ctx.matched_arg(args, &matched, 3), false, call)?;
    let sorted = scalar_flag(ctx, ctx.matched_arg(args, &matched, 5), true, call)?;
    let mut names = Vec::new();
    for (sym, _) in ctx.session.env_entries(env)? {
        let name = ctx.session.sym_name(sym)?;
        if all || !name.starts_with('.') {
            names.push(name.to_owned());
        }
    }
    if sorted {
        names.sort();
    } else {
        names.reverse();
    }
    let names = ctx.strings(names.iter().map(String::as_str))?;
    if let Some(pattern) = ctx.matched_arg(args, &matched, 4) {
        let value = ctx.session.intern("value")?;
        return ctx.call_function(
            "grep",
            &[
                (None, pattern),
                (None, names),
                (Some(value), roxide_value::TRUE_SCALAR),
            ],
            BASE_ENV,
        );
    }
    Ok(names)
}

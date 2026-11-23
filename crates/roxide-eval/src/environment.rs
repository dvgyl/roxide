use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, EMPTY_ENV, GLOBAL_ENV, RLogical, SEXP_MISSING, SEXP_NIL,
    SEXP_UNBOUND, SexpType, Sym, ValueApi,
};

use crate::control::logical;
use crate::{Args, Arity, Call, Ctx, EvalResult, Module};

pub(crate) const MODULE: Module = Module {
    prims: &[
        primitive!("assign", assign, Eval, Arity::Any, Off),
        primitive!("get", get, Eval, Arity::Any, On),
        primitive!("get0", get, Eval, Arity::Any, On),
        primitive!("mget", mget, Eval, Arity::Range(2, 5), On),
        primitive!("list2env", list_to_env, Eval, Arity::Range(1, 5), On),
        primitive!("as.environment", as_environment, Eval, Arity::Exact(1), On),
        primitive!(
            "reg.finalizer",
            register_finalizer,
            Eval,
            Arity::Range(2, 3),
            On
        ),
        primitive!(
            "activeBindingFunction",
            active_binding_function,
            Eval,
            Arity::Exact(2),
            On
        ),
        primitive!(
            "delayedAssign",
            delayed_assign,
            Unevaluated,
            Arity::Range(2, 4),
            Off
        ),
        primitive!("objects", ls, Eval, Arity::Any, On),
        primitive!("sys.calls", sys_stack, Eval, Arity::Exact(0), On),
        primitive!("sys.frames", sys_stack, Eval, Arity::Exact(0), On),
        primitive!("sys.parent", sys_stack, Eval, Arity::Range(0, 1), On),
        primitive!("sys.parents", sys_stack, Eval, Arity::Exact(0), On),
        primitive!("sys.on.exit", sys_stack, Eval, Arity::Exact(0), On),
        primitive!("exists", get, Eval, Arity::Any, On),
        primitive!("rm", remove, Unevaluated, Arity::Any, Off),
        primitive!("remove", remove, Unevaluated, Arity::Any, Off),
        primitive!("ls", ls, Eval, Arity::Any, On),
        primitive!(
            "makeActiveBinding",
            make_active_binding,
            Eval,
            Arity::Exact(3),
            Off
        ),
        primitive!("new.env", new_env, Eval, Arity::Range(0, 3), On),
        primitive!("emptyenv", constant_env, Eval, Arity::Exact(0), On),
        primitive!("baseenv", constant_env, Eval, Arity::Exact(0), On),
        primitive!("globalenv", constant_env, Eval, Arity::Exact(0), On),
        primitive!("environment", environment, Eval, Arity::Range(0, 1), On),
        primitive!("environment<-", set_env, Eval, Arity::Exact(2), On),
        primitive!("parent.env", parent_env, Eval, Arity::Exact(1), On),
        primitive!("parent.env<-", set_parent_env, Eval, Arity::Exact(2), On),
        primitive!("search", search, Eval, Arity::Exact(0), On),
        primitive!(
            "lockEnvironment",
            lock_environment,
            Eval,
            Arity::Range(1, 2),
            Off
        ),
        primitive!(
            "environmentIsLocked",
            env_is_locked,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!("lockBinding", lock_binding, Eval, Arity::Exact(2), Off),
        primitive!("unlockBinding", unlock_binding, Eval, Arity::Exact(2), Off),
        primitive!(
            "bindingIsLocked",
            binding_is_locked,
            Eval,
            Arity::Exact(2),
            On
        ),
        primitive!(
            "bindingIsActive",
            binding_is_active,
            Eval,
            Arity::Exact(2),
            On
        ),
        primitive!("asNamespace", as_namespace, Eval, Arity::Range(1, 2), On),
        primitive!("isNamespace", is_namespace, Eval, Arity::Exact(1), On),
        primitive!(
            "loadedNamespaces",
            loaded_namespaces,
            Eval,
            Arity::Exact(0),
            On
        ),
        primitive!("isTRUE", is_true_false, Eval, Arity::Exact(1), On),
        primitive!("isFALSE", is_true_false, Eval, Arity::Exact(1), On),
        primitive!("as.name", as_name, Eval, Arity::Exact(1), On),
        primitive!("as.symbol", as_name, Eval, Arity::Exact(1), On),
        primitive!("topenv", topenv, Eval, Arity::Range(0, 2), On),
        primitive!("parent.frame", parent_frame, Eval, Arity::Range(0, 1), On),
        primitive!("sys.call", sys_frame, Eval, Arity::Range(0, 1), On),
        primitive!("sys.function", sys_frame, Eval, Arity::Range(0, 1), On),
        primitive!("sys.frame", sys_frame, Eval, Arity::Range(0, 1), On),
        primitive!("sys.nframe", sys_nframe, Eval, Arity::Exact(0), On),
        primitive!("::", namespace, Unevaluated, Arity::Exact(2), On),
        primitive!(":::", namespace, Unevaluated, Arity::Exact(2), On),
    ],
    prelude: "",
};

impl Ctx {
    pub(crate) fn require_env(&mut self, value: SexpId, call: SexpId) -> EvalResult {
        if self.session.type_of(value)? == SexpType::Env {
            Ok(value)
        } else {
            Err(self.stop(call, "invalid 'envir' argument"))
        }
    }

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

    pub(crate) fn eval_env_at(&mut self, position: SexpId, call: &Call) -> EvalResult {
        let current = self
            .frames
            .iter()
            .rposition(|frame| frame.cloenv == call.env)
            .map_or(0, |i| i + 1) as i64;
        let index = i64::from(self.integer(position, call.err_call())?);
        let absolute = if index < 0 { current + index } else { index };
        if absolute == 0 {
            return Ok(GLOBAL_ENV);
        }
        if absolute < 0 || absolute > current {
            return Err(self.stop(call.err_call(), "not that many frames on the stack"));
        }
        Ok(self.frames[absolute as usize - 1].cloenv)
    }

    fn search_env(&mut self, position: SexpId, call: &Call) -> EvalResult {
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
}

fn make_active_binding(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["sym", "fun", "env"], call.err_call())?;
    let sym = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let sym = if ctx.session.type_of(sym)? == SexpType::Sym {
        ctx.sym_from_sexp(sym)?
    } else {
        let name = ctx.string(sym, call.err_call())?;
        ctx.session.intern(&name)?
    };
    let fun = ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL);
    if !matches!(
        ctx.session.type_of(fun)?,
        SexpType::Clo | SexpType::Builtin | SexpType::Special
    ) {
        return Err(ctx.stop(call.err_call(), "invalid function"));
    }
    let env = ctx.require_env(
        ctx.matched_arg(args, &matched, 2).unwrap_or(SEXP_NIL),
        call.err_call(),
    )?;
    if ctx
        .session
        .env_get(env, sym)
        .is_some_and(|binding| !binding.active)
    {
        return Err(ctx.stop(call.err_call(), "symbol already has a regular binding"));
    }
    ctx.session.env_define_binding(
        env,
        sym,
        roxide_value::Binding {
            value: fun,
            active: true,
            locked: false,
            missing: 0,
        },
    )?;
    Ok(SEXP_NIL)
}

fn scalar_flag(
    ctx: &mut Ctx,
    value: Option<SexpId>,
    default: bool,
    call: &Call,
) -> EvalResult<bool> {
    match value {
        Some(value) => ctx.truth(value, call.err_call()),
        None => Ok(default),
    }
}

fn assign(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn get(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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
    if found == Some(SEXP_MISSING) {
        return Err(ctx.stop(
            call.err_call(),
            format!("argument \"{name}\" is missing, with no default"),
        ));
    }
    if let Some(value) = found {
        return Ok(value);
    }
    if kind == "get0" {
        return Ok(fallback.unwrap_or(SEXP_NIL));
    }
    let mode = mode
        .filter(|mode| mode != "any")
        .map_or(String::new(), |mode| format!(" of mode '{mode}'"));
    Err(ctx.stop(call.err_call(), format!("object '{name}'{mode} not found")))
}

impl Ctx {
    fn lookup_mode(
        &mut self,
        sym: Sym,
        mut env: SexpId,
        mode: &str,
        inherits: bool,
        exists: bool,
    ) -> EvalResult<Option<SexpId>> {
        while let Some(found) = self.binding_env(sym, env, inherits)? {
            if exists && mode == "any" {
                return Ok(Some(SEXP_NIL));
            }
            let Some(value) = self.get_local(sym, found)? else {
                return Ok(None);
            };
            let ty = self.session.type_of(value)?;
            let compatible = match mode {
                "any" => true,
                "function" => matches!(ty, SexpType::Builtin | SexpType::Special | SexpType::Clo),
                "numeric" => matches!(ty, SexpType::Int | SexpType::Real),
                mode => ty.type_name(false) == mode,
            };
            if compatible || value == SEXP_MISSING {
                return Ok(Some(value));
            }
            if !inherits {
                break;
            }
            env = self.session.env_enclos(found)?;
        }
        Ok(None)
    }

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

fn mget(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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
            Some(value) => value,
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

fn as_environment(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn list_to_env(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn active_binding_function(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sym = binding_sym(ctx, args.get(0), call.err_call())?;
    let env = ctx.require_env(args.get(1), call.err_call())?;
    let name = ctx.session.sym_name(sym)?.to_owned();
    match ctx.session.env_get(env, sym) {
        Some(binding) if binding.active => Ok(binding.value),
        Some(_) => Err(ctx.stop(call.err_call(), format!("no active binding for \"{name}\""))),
        None => Err(ctx.stop(call.err_call(), format!("no binding for \"{name}\""))),
    }
}

fn delayed_assign(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn is_function(ctx: &Ctx, value: SexpId) -> EvalResult<bool> {
    Ok(matches!(
        ctx.session.type_of(value)?,
        SexpType::Clo | SexpType::Builtin | SexpType::Special
    ))
}

#[derive(Default)]
struct ExitFinalizers(Vec<(SexpId, SexpId)>);

impl Trace for ExitFinalizers {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for &(env, fun) in &self.0 {
            tracer.edge(env);
            tracer.edge(fun);
        }
    }
}

impl crate::CtxExt for ExitFinalizers {}

fn register_finalizer(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["e", "f", "onexit"], call.err_call())?;
    let env = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let fun = ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL);
    if !matches!(ctx.session.type_of(env)?, SexpType::Env | SexpType::ExtPtr) {
        return Err(ctx.stop(
            call.err_call(),
            "first argument must be environment or external pointer",
        ));
    }
    if !is_function(ctx, fun)? {
        return Err(ctx.stop(call.err_call(), "second argument must be a function"));
    }
    if scalar_flag(ctx, ctx.matched_arg(args, &matched, 2), false, call)? {
        ctx.ext::<ExitFinalizers>().0.push((env, fun));
    }
    Ok(SEXP_NIL)
}

impl Ctx {
    /// Run registered exit finalizers once, in reverse registration order.
    pub fn run_exit_finalizers(&mut self) {
        let finalizers = std::mem::take(&mut self.ext::<ExitFinalizers>().0);
        if finalizers.is_empty() {
            return;
        }
        let _ = self.scoped(|ctx| {
            let visible = ctx.visible;
            for &(env, fun) in &finalizers {
                ctx.protect(env);
                ctx.protect(fun);
            }
            for (env, fun) in finalizers.into_iter().rev() {
                let _ = ctx.apply(fun, &[(None, env)], SEXP_NIL, GLOBAL_ENV);
            }
            ctx.visible = visible;
            Ok(())
        });
    }
}

fn sys_stack(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let current = ctx
        .frames
        .iter()
        .rposition(|frame| frame.cloenv == call.env)
        .map_or(0, |index| index + 1);
    let parent = |frames: &[crate::ctx::Frame], child: usize| {
        let Some(frame) = child.checked_sub(1).and_then(|i| frames.get(i)) else {
            return 0;
        };
        frames[..child - 1]
            .iter()
            .rposition(|entry| entry.cloenv == frame.caller)
            .map_or(0, |index| index + 1)
    };
    match ctx.prims[usize::from(call.prim.0)].name {
        "sys.parent" => {
            let n = if args.is_empty() {
                1
            } else {
                ctx.integer(args.get(0), call.err_call())?
            };
            if n < 0 {
                return Err(ctx.stop(call.err_call(), "invalid 'n' value"));
            }
            let mut index = current;
            for _ in 0..n {
                index = parent(&ctx.frames, index);
                if index == 0 {
                    break;
                }
            }
            Ok(ctx.session.scalar_int(index as i32)?)
        }
        "sys.parents" => {
            let parents: Vec<_> = (1..=current)
                .map(|i| parent(&ctx.frames, i) as i32)
                .collect();
            Ok(ctx.session.alloc_int_from(&parents)?)
        }
        "sys.on.exit" => {
            let Some(frame) = ctx.frame(call.env) else {
                return Ok(SEXP_NIL);
            };
            match frame.on_exit.as_slice() {
                [] => Ok(SEXP_NIL),
                [expr] => Ok(*expr),
                exprs => {
                    let pairs = exprs.iter().map(|&expr| (None, expr)).collect();
                    let head = ctx.session.intern("{")?.as_sexp();
                    ctx.make_call(head, &Args::from_pairs(pairs))
                }
            }
        }
        kind => {
            let pairs = ctx.frames[..current]
                .iter()
                .map(|frame| {
                    (
                        None,
                        if kind == "sys.calls" {
                            frame.call
                        } else {
                            frame.cloenv
                        },
                    )
                })
                .collect();
            ctx.pairlist(&Args::from_pairs(pairs), false)
        }
    }
}

fn remove(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn ls(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn new_env(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["hash", "parent", "size"], call.err_call())?;
    let parent = ctx.matched_arg(args, &matched, 1).unwrap_or(call.env);
    ctx.require_env(parent, call.err_call())?;
    Ok(ctx.session.alloc_env(parent)?)
}

fn constant_env(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    Ok(match ctx.prims[usize::from(call.prim.0)].name {
        "baseenv" => BASE_ENV,
        "emptyenv" => EMPTY_ENV,
        _ => GLOBAL_ENV,
    })
}

fn environment(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn set_env(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn parent_frame(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let n = if args.is_empty() {
        1
    } else {
        ctx.integer(args.get(0), call.err_call())?
    };
    if n < 1 {
        return Err(ctx.stop(call.err_call(), "invalid 'n' value"));
    }
    let mut env = call.env;
    let mut end = ctx.frames.len();
    for _ in 0..n {
        let Some(index) = ctx.frames[..end]
            .iter()
            .rposition(|frame| frame.cloenv == env)
        else {
            return Ok(GLOBAL_ENV);
        };
        env = ctx.frames[index].caller;
        if env == GLOBAL_ENV {
            break;
        }
        end = index;
    }
    Ok(env)
}

fn sys_nframe(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    let depth = ctx
        .frames
        .iter()
        .rposition(|frame| frame.cloenv == call.env)
        .map_or(0, |i| i + 1);
    Ok(ctx.session.scalar_int(depth as i32)?)
}

fn sys_frame(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let current = ctx
        .frames
        .iter()
        .rposition(|frame| frame.cloenv == call.env)
        .map_or(0, |i| i + 1) as i64;
    let which = if args.is_empty() {
        0
    } else {
        i64::from(ctx.integer(args.get(0), call.err_call())?)
    };
    let name = ctx.prims[usize::from(call.prim.0)].name;
    let index = if which == 0 && name == "sys.frame" {
        0
    } else if which <= 0 {
        current + which
    } else {
        which
    };
    if index == 0 {
        return Ok(if name == "sys.frame" {
            GLOBAL_ENV
        } else {
            SEXP_NIL
        });
    }
    if index < 0 || index > current {
        return Err(ctx.stop(call.err_call(), "not that many frames on the stack"));
    }
    let frame = &ctx.frames[index as usize - 1];
    Ok(match name {
        "sys.call" => frame.call,
        "sys.frame" => frame.cloenv,
        _ => frame.fun,
    })
}

fn namespace(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let package = ctx.name(args.get(0), call.err_call())?;
    let name = ctx.name(args.get(1), call.err_call())?;
    let sym = ctx.session.intern(&name)?;
    let is_triple = ctx.prims[usize::from(call.prim.0)].name == ":::";
    if package == "base" {
        return ctx.get_var(sym, BASE_ENV)?.ok_or_else(|| {
            ctx.stop(
                call.err_call(),
                format!("'{name}' is not an exported object from 'namespace:base'"),
            )
        });
    }
    let ns = ctx.load_namespace(&package, call.err_call())?;
    if is_triple {
        if let Some(binding) = ctx.session.env_get(ns, sym)
            && binding.value != SEXP_UNBOUND
        {
            return ctx.read_binding(binding, ns);
        }
        return Err(ctx.stop(call.err_call(), format!("object '{name}' not found")));
    }
    let exported = if let Some(exports_env) = ctx.ns_exports_env(ns)? {
        if let Some(exp_binding) = ctx.session.env_get(exports_env, sym) {
            let target_sym = if ctx.session.type_of(exp_binding.value)? == SexpType::Str {
                let mapped = ctx.string(exp_binding.value, call.err_call())?;
                ctx.session.intern(&mapped)?
            } else {
                sym
            };
            // Resolve strictly within `ns` and its `imports:<pkg>` frame (never `.GlobalEnv`/`base`).
            if let Some(binding) = ctx.session.env_get(ns, target_sym)
                && binding.value != SEXP_UNBOUND
            {
                Some(ctx.read_binding(binding, ns)?)
            } else {
                let imports = ctx.session.env_enclos(ns)?;
                if imports != SEXP_NIL
                    && imports != BASE_NAMESPACE
                    && imports != BASE_ENV
                    && imports != GLOBAL_ENV
                    && imports != EMPTY_ENV
                    && let Some(binding) = ctx.session.env_get(imports, target_sym)
                    && binding.value != SEXP_UNBOUND
                {
                    Some(ctx.read_binding(binding, imports)?)
                } else {
                    None
                }
            }
        } else {
            None
        }
    } else if !name.starts_with('.') {
        ctx.session
            .env_get(ns, sym)
            .filter(|b| b.value != SEXP_UNBOUND)
            .map(|b| ctx.read_binding(b, ns))
            .transpose()?
    } else {
        None
    };
    exported.ok_or_else(|| {
        ctx.stop(
            call.err_call(),
            format!("'{name}' is not an exported object from 'namespace:{package}'"),
        )
    })
}

fn parent_env(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = ctx.require_env(args.get(0), call.err_call())?;
    if env == EMPTY_ENV {
        return Err(ctx.stop(call.err_call(), "the empty environment has no parent"));
    }
    Ok(ctx.session.env_enclos(env)?)
}

fn set_parent_env(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

fn search(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let path = ctx.search_path()?;
    let mut names = Vec::with_capacity(path.len());
    for env in path {
        names.push(ctx.search_entry_name(env)?);
    }
    ctx.strings(names.iter().map(String::as_str))
}

fn lock_environment(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = ctx.require_env(args.get(0), call.err_call())?;
    let bindings = if args.len() >= 2 {
        ctx.truth(args.get(1), call.err_call())?
    } else {
        false
    };
    ctx.session.env_set_locked(env, true)?;
    if bindings {
        ctx.session.env_lock_all_bindings(env)?;
    }
    Ok(SEXP_NIL)
}

fn env_is_locked(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = ctx.require_env(args.get(0), call.err_call())?;
    let locked = ctx.session.env_is_locked(env)?;
    Ok(ctx
        .session
        .scalar_lgl(RLogical::from_option(Some(locked)))?)
}

fn binding_sym(ctx: &mut Ctx, val: SexpId, call: SexpId) -> EvalResult<Sym> {
    if ctx.session.type_of(val)? == SexpType::Sym && val != SEXP_MISSING {
        ctx.sym_from_sexp(val)
    } else {
        let name = ctx.string(val, call)?;
        Ok(ctx.session.intern(&name)?)
    }
}

fn lock_binding(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sym = binding_sym(ctx, args.get(0), call.err_call())?;
    let env = ctx.require_env(args.get(1), call.err_call())?;
    if !ctx.session.env_lock_binding(env, sym, true)? {
        let name = ctx.session.sym_name(sym)?.to_owned();
        return Err(ctx.stop(call.err_call(), format!("no binding for \"{name}\"")));
    }
    Ok(SEXP_NIL)
}

fn unlock_binding(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sym = binding_sym(ctx, args.get(0), call.err_call())?;
    let env = ctx.require_env(args.get(1), call.err_call())?;
    if !ctx.session.env_lock_binding(env, sym, false)? {
        let name = ctx.session.sym_name(sym)?.to_owned();
        return Err(ctx.stop(call.err_call(), format!("no binding for \"{name}\"")));
    }
    Ok(SEXP_NIL)
}

fn binding_is_locked(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sym = binding_sym(ctx, args.get(0), call.err_call())?;
    let env = ctx.require_env(args.get(1), call.err_call())?;
    let Some(locked) = ctx.session.env_binding_is_locked(env, sym)? else {
        let name = ctx.session.sym_name(sym)?.to_owned();
        return Err(ctx.stop(call.err_call(), format!("no binding for \"{name}\"")));
    };
    Ok(ctx
        .session
        .scalar_lgl(RLogical::from_option(Some(locked)))?)
}

fn binding_is_active(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sym = binding_sym(ctx, args.get(0), call.err_call())?;
    let env = ctx.require_env(args.get(1), call.err_call())?;
    let Some(active) = ctx.session.env_binding_is_active(env, sym)? else {
        let name = ctx.session.sym_name(sym)?.to_owned();
        return Err(ctx.stop(call.err_call(), format!("no binding for \"{name}\"")));
    };
    Ok(ctx
        .session
        .scalar_lgl(RLogical::from_option(Some(active)))?)
}

fn as_namespace(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let arg = args.get(0);
    if ctx.session.type_of(arg)? == SexpType::Env && ctx.is_namespace_env(arg) {
        return Ok(arg);
    }
    let name = ctx.string(arg, call.err_call())?;
    let base_ok = if args.len() >= 2 {
        ctx.truth(args.get(1), call.err_call())?
    } else {
        true
    };
    if name == "base" {
        if !base_ok {
            return Err(ctx.stop(call.err_call(), "namespace 'base' is not allowed here"));
        }
        return Ok(BASE_NAMESPACE);
    }
    ctx.load_namespace(&name, call.err_call())
}

fn is_namespace(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let arg = args.get(0);
    let yes = ctx.session.type_of(arg)? == SexpType::Env && ctx.is_namespace_env(arg);
    Ok(ctx.session.scalar_lgl(RLogical::from_option(Some(yes)))?)
}

fn loaded_namespaces(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let names = ctx.ns_loaded_names();
    ctx.strings(names.iter().map(String::as_str))
}

fn is_true_false(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let want = ctx.prims[usize::from(call.prim.0)].name == "isTRUE";
    let x = args.get(0);
    let ok = ctx.session.type_of(x)? == SexpType::Lgl
        && ctx.session.xlength(x)? == 1
        && ctx.session.lgl(x)?.get(0).get() == Some(want);
    Ok(ctx.session.scalar_lgl(RLogical::from_option(Some(ok)))?)
}

fn as_name(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    if ctx.session.type_of(x)? == SexpType::Sym {
        return Ok(x);
    }
    let (coerced, _) = ctx.session.coerce(x, SexpType::Str)?;
    let name = ctx.string(coerced, call.err_call())?;
    if name.is_empty() {
        return Err(ctx.stop(call.err_call(), "attempt to use zero-length variable name"));
    }
    Ok(ctx.session.intern(&name)?.as_sexp())
}

fn topenv(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

impl Drop for Ctx {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.run_exit_finalizers();
        }
    }
}

use roxide_gc::SexpId;
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
    let exists = ctx.prims[usize::from(call.prim.0)].name == "exists";
    let matched = ctx.match_names(
        args,
        &["x", "pos", "envir", "mode", "inherits"],
        call.err_call(),
    )?;
    let name = ctx.string(
        ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL),
        call.err_call(),
    )?;
    let sym = ctx.session.intern(&name)?;
    let mut env = if let Some(env) = ctx.matched_arg(args, &matched, 2) {
        ctx.require_env(env, call.err_call())?
    } else if let Some(pos) = ctx.matched_arg(args, &matched, 1) {
        ctx.search_env(pos, call)?
    } else {
        call.env
    };
    let mode = ctx
        .matched_arg(args, &matched, 3)
        .map(|value| ctx.string(value, call.err_call()))
        .transpose()?;
    let inherits = scalar_flag(ctx, ctx.matched_arg(args, &matched, 4), true, call)?;
    while let Some(found) = ctx.binding_env(sym, env, inherits)? {
        if exists && mode.as_deref().is_none_or(|mode| mode == "any") {
            return Ok(ctx.session.scalar_lgl(logical(true))?);
        }
        let value = ctx.get_var(sym, found)?.unwrap_or(SEXP_NIL);
        if value == SEXP_MISSING && !exists {
            return Err(ctx.stop(
                call.err_call(),
                format!("argument \"{name}\" is missing, with no default"),
            ));
        }
        let ty = ctx.session.type_of(value)?;
        let compatible = match mode.as_deref() {
            None | Some("any") => true,
            Some("function") => matches!(ty, SexpType::Builtin | SexpType::Special | SexpType::Clo),
            Some("numeric") => matches!(ty, SexpType::Int | SexpType::Real),
            Some(mode) => ty.type_name(false) == mode,
        };
        if compatible {
            return if exists {
                Ok(ctx.session.scalar_lgl(logical(true))?)
            } else {
                Ok(value)
            };
        }
        if !inherits {
            break;
        }
        env = ctx.session.env_enclos(found)?;
    }
    if exists {
        return Ok(ctx.session.scalar_lgl(logical(false))?);
    }
    let mode = mode
        .filter(|mode| mode != "any")
        .map_or(String::new(), |mode| format!(" of mode '{mode}'"));
    Err(ctx.stop(call.err_call(), format!("object '{name}'{mode} not found")))
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

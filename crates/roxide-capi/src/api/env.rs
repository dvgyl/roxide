//! Environment lookup, assignment, locks, and active bindings.

#![allow(non_snake_case)]

use std::ffi::{CStr, c_char, c_int};

use roxide_eval::{Ctx, EvalResult, Unwind};
use roxide_gc::SexpId;
use roxide_value::{
    BASE_NAMESPACE, Binding, CharId, Encoding, GLOBAL_ENV, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND,
    SYM_NAME, SYM_NAMESPACE_ENV, SYM_SPEC, SexpType, Sym, ValueApi, ValueError,
};

use super::{api, environment, root, symbol};
use crate::abi::{Rboolean, Sexp};

fn binding_key(ctx: &mut Ctx, sym: Sexp, rho: Sexp) -> EvalResult<(Sym, SexpId)> {
    Ok((symbol(ctx, sym)?, environment(ctx, rho)?))
}

fn missing_binding(ctx: &mut Ctx, sym: Sym) -> Unwind {
    let name = ctx.session().sym_name(sym).unwrap_or("").to_owned();
    ctx.stop(SEXP_NIL, format!("no binding for \"{name}\""))
}

fn binding_frame(ctx: &Ctx, sym: Sym, mut env: SexpId) -> EvalResult<Option<SexpId>> {
    while env != SEXP_NIL {
        if ctx
            .session()
            .env_get(env, sym)
            .is_some_and(|b| b.value != SEXP_UNBOUND)
        {
            return Ok(Some(env));
        }
        env = ctx.session().env_enclos(env)?;
    }
    Ok(None)
}

/// Assign a value in one environment frame.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_defineVar(sym: Sexp, value: Sexp, rho: Sexp) {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api((), |ctx| {
            let (sym, env) = binding_key(ctx, sym, rho)?;
            let value = root(ctx, value)?;
            ctx.assign(sym, value, env)
        })
    }
}

/// Update the first matching binding. Create it in the global environment if absent.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_setVar(sym: Sexp, value: Sexp, rho: Sexp) {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api((), |ctx| {
            let (sym, env) = binding_key(ctx, sym, rho)?;
            let value = root(ctx, value)?;
            let dest = binding_frame(ctx, sym, env)?.unwrap_or(GLOBAL_ENV);
            ctx.assign(sym, value, dest)
        })
    }
}

fn legacy_lookup(ctx: &mut Ctx, sym: Sym, env: SexpId) -> EvalResult {
    let Some(binding) = ctx.session().env_get(env, sym) else {
        return Ok(SEXP_UNBOUND);
    };
    if binding.active {
        return ctx.apply(binding.value, &[], SEXP_NIL, env);
    }
    Ok(binding.value)
}

/// Look up a binding in the environment chain. Return promises without forcing them.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_findVar(sym: Sexp, rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let (sym, env) = binding_key(ctx, sym, rho)?;
            let value = match binding_frame(ctx, sym, env)? {
                Some(frame) => legacy_lookup(ctx, sym, frame)?,
                None => SEXP_UNBOUND,
            };
            Ok(Sexp::from_id(value))
        })
    }
}

/// Look up a binding in one environment frame.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_findVarInFrame(rho: Sexp, sym: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe { Rf_findVarInFrame3(rho, sym, 1) }
}

/// Look up a binding in one frame. Call active bindings and leave promises unforced.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_findVarInFrame3(rho: Sexp, sym: Sexp, _do_get: Rboolean) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let (sym, env) = binding_key(ctx, sym, rho)?;
            legacy_lookup(ctx, sym, env).map(Sexp::from_id)
        })
    }
}

fn get_var_key(ctx: &mut Ctx, sym: Sexp, rho: Sexp) -> EvalResult<(Sym, SexpId)> {
    let id = root(ctx, sym)?;
    let sym = ctx
        .session()
        .sexp_as_sym(id)
        .map_err(|_| ctx.stop(SEXP_NIL, "first argument to 'R_getVarEx' must be a symbol"))?;
    let env = root(ctx, rho)?;
    if ctx.session().type_of(env)? != SexpType::Env {
        return Err(ctx.stop(
            SEXP_NIL,
            "second argument to 'R_getVarEx' must be an environment",
        ));
    }
    Ok((sym, env))
}

fn get_var(ctx: &mut Ctx, sym: Sym, env: SexpId, inherits: Rboolean) -> EvalResult<Option<SexpId>> {
    let value = if inherits != 0 {
        ctx.get_var(sym, env)?
    } else {
        ctx.get_local(sym, env)?
    };
    if value == Some(SEXP_MISSING) {
        let name = ctx.session().sym_name(sym)?.to_owned();
        return Err(ctx.stop(
            SEXP_NIL,
            format!("argument \"{name}\" is missing, with no default"),
        ));
    }
    Ok(value.filter(|&value| value != SEXP_UNBOUND))
}

/// Read a binding and force its promise if necessary. Signal an error if absent.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_getVar(sym: Sexp, rho: Sexp, inherits: Rboolean) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let (sym, env) = get_var_key(ctx, sym, rho)?;
            let value = get_var(ctx, sym, env, inherits)?.ok_or_else(|| {
                let name = ctx.session().sym_name(sym).unwrap_or("").to_owned();
                ctx.stop(SEXP_NIL, format!("object '{name}' not found"))
            })?;
            Ok(Sexp::from_id(value))
        })
    }
}

/// Read a binding and force its promise if necessary. Return the fallback if absent.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_getVarEx(
    sym: Sexp,
    rho: Sexp,
    inherits: Rboolean,
    fallback: Sexp,
) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let (sym, env) = get_var_key(ctx, sym, rho)?;
            let fallback = root(ctx, fallback)?;
            Ok(Sexp::from_id(
                get_var(ctx, sym, env, inherits)?.unwrap_or(fallback),
            ))
        })
    }
}

/// Test for a binding without evaluating it.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_existsVarInFrame(rho: Sexp, sym: Sexp) -> Rboolean {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(0, |ctx| {
            let (sym, env) = binding_key(ctx, sym, rho)?;
            Ok(i32::from(
                ctx.session()
                    .env_get(env, sym)
                    .is_some_and(|b| b.value != SEXP_UNBOUND),
            ))
        })
    }
}

/// Remove a binding from one frame.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_removeVarFromFrame(sym: Sexp, rho: Sexp) {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api((), |ctx| {
            let (sym, env) = binding_key(ctx, sym, rho)?;
            if ctx.session().env_is_locked(env)? {
                return Err(ctx.stop(SEXP_NIL, "cannot remove bindings from a locked environment"));
            }
            ctx.session_mut().env_remove(env, sym)?;
            Ok(())
        })
    }
}

/// Allocate an environment with the given parent.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_NewEnv(enclos: Sexp, _hash: c_int, _size: c_int) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let parent = environment(ctx, enclos)?;
            ctx.safepoint();
            Ok(Sexp::from_id(ctx.session_mut().alloc_env(parent)?))
        })
    }
}

/// Return the parent of an environment.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ParentEnv(rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let env = environment(ctx, rho)?;
            Ok(Sexp::from_id(ctx.session().env_enclos(env)?))
        })
    }
}

fn package_name(ctx: &Ctx, env: SexpId) -> EvalResult<Option<SexpId>> {
    let name = ctx.session().attr(env, SYM_NAME);
    let Some(id) = ctx
        .session()
        .str_slice(name)
        .ok()
        .and_then(|names| names.first())
    else {
        return Ok(None);
    };
    Ok(ctx
        .session()
        .char_bytes(*id)?
        .starts_with(b"package:")
        .then_some(name))
}

/// Test whether an environment has a package name.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_IsPackageEnv(rho: Sexp) -> Rboolean {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(0, |ctx| {
            let id = root(ctx, rho)?;
            Ok(i32::from(
                ctx.session().type_of(id)? == SexpType::Env && package_name(ctx, id)?.is_some(),
            ))
        })
    }
}

/// Return the package name of an environment or R NULL.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_PackageEnvName(rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let id = root(ctx, rho)?;
            let value = if ctx.session().type_of(id)? == SexpType::Env {
                package_name(ctx, id)?.unwrap_or(SEXP_NIL)
            } else {
                SEXP_NIL
            };
            Ok(Sexp::from_id(value))
        })
    }
}

/// Test whether an object is a namespace environment.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_IsNamespaceEnv(rho: Sexp) -> Rboolean {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(0, |ctx| {
            let id = root(ctx, rho)?;
            Ok(i32::from(ctx.is_namespace_env(id)))
        })
    }
}

/// Return the namespace name and version specification or R NULL.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_NamespaceEnvSpec(rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let id = root(ctx, rho)?;
            let spec = if id == BASE_NAMESPACE {
                ctx.session_mut().scalar_str("base")?
            } else {
                ctx.session()
                    .env_get(id, SYM_NAMESPACE_ENV)
                    .and_then(|info| ctx.session().env_get(info.value, SYM_SPEC))
                    .map_or(SEXP_NIL, |spec| spec.value)
            };
            Ok(Sexp::from_id(spec))
        })
    }
}

/// Find or load a namespace by name.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_FindNamespace(name: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let id = root(ctx, name)?;
            let name = if ctx.session().type_of(id)? == SexpType::Sym {
                ctx.session().sym_name_of(id)?.to_owned()
            } else {
                let chars = ctx.session().str_slice(id)?;
                let [name] = chars else {
                    return Err(ctx.stop(SEXP_NIL, "invalid namespace name"));
                };
                if *name == CharId::NA {
                    return Err(ctx.stop(SEXP_NIL, "invalid namespace name"));
                }
                String::from_utf8_lossy(ctx.session().char_bytes(*name)?).into_owned()
            };
            ctx.load_namespace(&name, SEXP_NIL).map(Sexp::from_id)
        })
    }
}

/// Return an already loaded namespace or R NULL.
///
/// # Safety
/// Call inside an active native trampoline. `name` must point to a C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_getRegisteredNamespace(name: *const c_char) -> Sexp {
    // SAFETY: The caller provides a trampoline and a C string.
    unsafe {
        api(Sexp::NULL, |ctx| {
            if name.is_null() {
                return Err(ctx.stop(SEXP_NIL, "invalid NULL namespace name"));
            }
            let name = CStr::from_ptr(name).to_string_lossy();
            Ok(Sexp::from_id(ctx.ns_lookup(&name).unwrap_or(SEXP_NIL)))
        })
    }
}

/// Lock an environment. Optionally lock all current bindings.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_LockEnvironment(rho: Sexp, bindings: Rboolean) {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api((), |ctx| {
            let env = environment(ctx, rho)?;
            if bindings != 0 {
                ctx.session_mut().env_lock_all_bindings(env)?;
            }
            Ok(ctx.session_mut().env_set_locked(env, true)?)
        })
    }
}

/// Test whether an environment is locked.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_EnvironmentIsLocked(rho: Sexp) -> Rboolean {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(0, |ctx| {
            let env = environment(ctx, rho)?;
            Ok(i32::from(ctx.session().env_is_locked(env)?))
        })
    }
}

unsafe fn set_binding_lock(sym: Sexp, rho: Sexp, locked: bool) {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api((), |ctx| {
            let (sym, env) = binding_key(ctx, sym, rho)?;
            if !ctx.session_mut().env_lock_binding(env, sym, locked)? {
                return Err(missing_binding(ctx, sym));
            }
            Ok(())
        })
    }
}

/// Lock an existing binding.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_LockBinding(sym: Sexp, rho: Sexp) {
    // SAFETY: The caller provides an active native trampoline.
    unsafe { set_binding_lock(sym, rho, true) }
}

/// Unlock an existing binding.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_unLockBinding(sym: Sexp, rho: Sexp) {
    // SAFETY: The caller provides an active native trampoline.
    unsafe { set_binding_lock(sym, rho, false) }
}

fn required_binding(ctx: &mut Ctx, sym: Sexp, rho: Sexp) -> EvalResult<Binding> {
    let (sym, env) = binding_key(ctx, sym, rho)?;
    ctx.session()
        .env_get(env, sym)
        .ok_or_else(|| missing_binding(ctx, sym))
}

/// Test whether a binding is locked.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_BindingIsLocked(sym: Sexp, rho: Sexp) -> Rboolean {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(0, |ctx| {
            Ok(i32::from(required_binding(ctx, sym, rho)?.locked))
        })
    }
}

/// Test whether a binding is active.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_BindingIsActive(sym: Sexp, rho: Sexp) -> Rboolean {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(0, |ctx| {
            Ok(i32::from(required_binding(ctx, sym, rho)?.active))
        })
    }
}

/// Return the function of an active binding.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ActiveBindingFunction(sym: Sexp, rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let (sym, env) = binding_key(ctx, sym, rho)?;
            let binding = ctx
                .session()
                .env_get(env, sym)
                .ok_or_else(|| missing_binding(ctx, sym))?;
            if !binding.active {
                let name = ctx.session().sym_name(sym)?.to_owned();
                return Err(ctx.stop(SEXP_NIL, format!("no active binding for \"{name}\"")));
            }
            Ok(Sexp::from_id(binding.value))
        })
    }
}

/// Create an active binding or change its function.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_MakeActiveBinding(sym: Sexp, fun: Sexp, rho: Sexp) {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api((), |ctx| {
            let (sym, env) = binding_key(ctx, sym, rho)?;
            let fun = root(ctx, fun)?;
            if !matches!(
                ctx.session().type_of(fun)?,
                SexpType::Clo | SexpType::Builtin | SexpType::Special
            ) {
                return Err(ctx.stop(SEXP_NIL, "not a function"));
            }
            if ctx
                .session()
                .env_get(env, sym)
                .is_some_and(|binding| !binding.active)
            {
                return Err(ctx.stop(SEXP_NIL, "symbol already has a regular binding"));
            }
            let result = ctx.session_mut().env_define_binding(
                env,
                sym,
                Binding {
                    value: fun,
                    active: true,
                    locked: false,
                    missing: 0,
                },
            );
            match result {
                Err(ValueError::LockedBinding(_)) => Err(ctx.stop(
                    SEXP_NIL,
                    "cannot change active binding if binding is locked",
                )),
                other => Ok(other?),
            }
        })
    }
}

/// List the names in one frame. Optionally include hidden names.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_lsInternal(rho: Sexp, all: Rboolean) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe { R_lsInternal3(rho, all, 1) }
}

/// List names in one frame with optional filtering and sorting.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_lsInternal3(rho: Sexp, all: Rboolean, sorted: Rboolean) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let env = environment(ctx, rho)?;
            ctx.safepoint();
            let entries = ctx.session().env_entries(env)?;
            let mut names = Vec::with_capacity(entries.len());
            for (sym, binding) in entries {
                let name = ctx.session().sym_name(sym)?;
                if binding.value != SEXP_UNBOUND && (all != 0 || !name.starts_with('.')) {
                    names.push(name.to_owned());
                }
            }
            if sorted != 0 {
                names.sort_unstable();
            }
            let session = ctx.session_mut();
            let chars = names
                .iter()
                .map(|name| session.mk_char(name.as_bytes(), Encoding::Utf8))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Sexp::from_id(session.alloc_str_from(&chars)?))
        })
    }
}

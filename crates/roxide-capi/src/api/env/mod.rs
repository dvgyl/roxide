//! Environment lookup, bindings, and variadic arguments.

#![allow(non_snake_case)]

mod bindings;
mod eval;
mod parse;

pub use bindings::*;
pub use eval::*;
pub use parse::*;

use std::ffi::{CStr, c_char, c_int};

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, CharId, EMPTY_ENV, Encoding, GLOBAL_ENV, SEXP_NIL, SEXP_UNBOUND,
    SYM_DOT_PACKAGE_NAME, SYM_NAME, SYM_NAMESPACE_ENV, SYM_SPEC, SexpType, ValueApi,
};

use super::{api, environment, root, vector::alloc_vector};
use crate::abi::{Rboolean, Sexp};
use bindings::legacy_lookup;

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

/// Find the first top-level environment or the requested target in the parent chain.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_topenv(target: Sexp, rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let target = root(ctx, target)?;
            let mut env = root(ctx, rho)?;
            if env == SEXP_NIL {
                return if target == env {
                    Ok(Sexp::from_id(SEXP_NIL))
                } else {
                    Err(ctx.stop(SEXP_NIL, "use of NULL environment is defunct"))
                };
            }
            for _ in 0..ctx.session().heap.live_objects() {
                if env == EMPTY_ENV || env == SEXP_NIL {
                    return Ok(Sexp::from_id(GLOBAL_ENV));
                }
                if env == target
                    || matches!(env, GLOBAL_ENV | BASE_ENV | BASE_NAMESPACE)
                    || package_name(ctx, env)?.is_some()
                    || has_namespace_spec(ctx, env)?
                    || ctx
                        .session()
                        .env_get(env, SYM_DOT_PACKAGE_NAME)
                        .is_some_and(|binding| binding.value != SEXP_UNBOUND)
                {
                    return Ok(Sexp::from_id(env));
                }
                env = ctx.session().env_enclos(env)?;
            }
            Err(ctx.stop(SEXP_NIL, "cyclic environment chain"))
        })
    }
}

/// Return a list vector of the symbols bound in one frame.
///
/// # Safety
/// Call inside an active native trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_envSymbols(rho: Sexp) -> Sexp {
    // SAFETY: The caller provides an active native trampoline.
    unsafe {
        api(Sexp::NULL, |ctx| {
            let env = root(ctx, rho)?;
            if ctx.session().type_of(env)? != SexpType::Env {
                return Err(ctx.stop(SEXP_NIL, "invalid 'envir' argument"));
            }
            ctx.safepoint();
            let mut entries = ctx.session().env_entries(env)?;
            entries.retain(|(_, binding)| binding.value != SEXP_UNBOUND);
            let symbols = alloc_vector(ctx, SexpType::Vec, entries.len())?;
            let session = ctx.session_mut();
            for (i, (sym, _)) in entries.into_iter().rev().enumerate() {
                session.list_set(symbols, i, sym.as_sexp())?;
            }
            Ok(Sexp::from_id(symbols))
        })
    }
}

fn has_namespace_spec(ctx: &mut Ctx, env: SexpId) -> EvalResult<bool> {
    let info = legacy_lookup(ctx, SYM_NAMESPACE_ENV, env)?;
    if ctx.session().type_of(info)? != SexpType::Env {
        return Ok(false);
    }
    let spec = legacy_lookup(ctx, SYM_SPEC, info)?;
    Ok(ctx
        .session()
        .str_slice(spec)
        .is_ok_and(|parts| !parts.is_empty()))
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

/// Binding types in the order of `R_BindingType_t` in `Rinternals.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub enum RBindingType {
    /// The frame has no binding for the symbol.
    Unbound,
    /// The frame binds the symbol to a value.
    Value,
    /// The frame binds the symbol to a missing argument.
    Missing,
    /// The frame binds the symbol to a promise that has no cached value.
    Delayed,
    /// The frame binds the symbol to a promise with a cached value.
    Forced,
    /// The frame binds the symbol to an active binding function.
    Active,
}

/// Dots argument types in the order of `R_DotType_t` in `Rinternals.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub enum RDotType {
    /// The argument holds a value.
    Value,
    /// The argument is missing.
    Missing,
    /// The argument holds a promise without a cached value.
    Delayed,
    /// The argument holds a promise with a cached value.
    Forced,
}

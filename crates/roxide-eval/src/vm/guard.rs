//! Side-effect-free guards for base calls and variable reads.

use roxide_gc::SexpId;
use roxide_value::{
    BASE_ENV, Binding, PromiseState, RSession, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, Sym,
    ValueApi, ValueError,
};

use super::cache::{CallCache, EnvPath, VarCache};

enum CallBinding {
    Absent,
    NotCallable,
    Callable(SexpId),
    Dynamic,
}

fn callable(session: &RSession, env: SexpId, sym: Sym) -> CallBinding {
    let Some(binding) = session.env_get(env, sym) else {
        return CallBinding::Absent;
    };
    if binding.active {
        return CallBinding::Dynamic;
    }
    if binding.value == SEXP_MISSING {
        return CallBinding::Dynamic;
    }
    match session.type_of(binding.value) {
        Ok(SexpType::Clo | SexpType::Builtin | SexpType::Special) => {
            CallBinding::Callable(binding.value)
        }
        Ok(SexpType::Prom) => CallBinding::Dynamic,
        Ok(_) => CallBinding::NotCallable,
        Err(_) => CallBinding::Dynamic,
    }
}

pub(super) fn base_call(
    session: &RSession,
    sym: Sym,
    env: SexpId,
    cached: &mut Option<CallCache>,
) -> Option<SexpId> {
    let CallBinding::Callable(expected) = callable(session, BASE_ENV, sym) else {
        return None;
    };
    if !matches!(
        session.type_of(expected),
        Ok(SexpType::Builtin | SexpType::Special)
    ) {
        return None;
    }
    match callable(session, env, sym) {
        CallBinding::Callable(fun) => return (fun == expected).then_some(fun),
        CallBinding::Dynamic => return None,
        CallBinding::Absent | CallBinding::NotCallable => {}
    }
    let mut env = session.env_enclos(env).ok()?;
    if let Some(entry) = cached
        && entry.sym == sym
        && entry.fun == expected
        && entry.path.matches(session, env)
    {
        return Some(expected);
    }

    let mut path = EnvPath::default();
    let mut cacheable = true;
    while env != SEXP_NIL {
        path.push(session, env)?;
        let mut binding = callable(session, env, sym);
        if matches!(binding, CallBinding::Dynamic)
            && let Some(Binding {
                value,
                active: false,
                ..
            }) = session.env_get(env, sym)
            && let Ok((_, _, value, PromiseState::Forced)) = session.promise_parts(value)
        {
            cacheable = false;
            binding = match session.type_of(value).ok()? {
                SexpType::Clo | SexpType::Builtin | SexpType::Special => {
                    CallBinding::Callable(value)
                }
                _ if value == SEXP_MISSING => CallBinding::Dynamic,
                _ => CallBinding::NotCallable,
            };
        }
        match binding {
            CallBinding::Callable(fun) if fun == expected => {
                if cacheable {
                    *cached = Some(CallCache { sym, path, fun });
                }
                return Some(fun);
            }
            CallBinding::Callable(_) | CallBinding::Dynamic => return None,
            CallBinding::NotCallable => {
                cacheable = false;
                env = session.env_enclos(env).ok()?;
            }
            CallBinding::Absent => env = session.env_enclos(env).ok()?,
        }
    }
    None
}

pub(super) fn variable(
    session: &RSession,
    sym: Sym,
    env: SexpId,
    cached: &mut Option<VarCache>,
) -> Result<Option<(SexpId, Binding)>, ValueError> {
    if let Some(binding) = session.env_get(env, sym)
        && binding.value != SEXP_UNBOUND
    {
        return Ok(Some((env, binding)));
    }

    let mut env = session.env_enclos(env)?;
    if let Some(entry) = cached
        && entry.sym == sym
        && entry.path.matches(session, env)
        && let Some(binding) = session.env_get(entry.binding_env, sym)
        && binding.value != SEXP_UNBOUND
    {
        return Ok(Some((entry.binding_env, binding)));
    }

    let mut path = EnvPath::default();
    let mut cacheable = true;
    while env != SEXP_NIL {
        cacheable &= path.push(session, env).is_some();
        if let Some(binding) = session.env_get(env, sym) {
            if binding.value != SEXP_UNBOUND {
                if cacheable {
                    *cached = Some(VarCache {
                        sym,
                        path,
                        binding_env: env,
                    });
                }
                return Ok(Some((env, binding)));
            }
            cacheable = false;
        }
        env = session.env_enclos(env)?;
    }
    Ok(None)
}

use roxide_gc::SexpId;
use roxide_value::{RSession, SEXP_NIL};

use super::{WireResult, invalid};

pub(super) enum CodecSession<'a, 'h> {
    Plain(&'a mut RSession),
    Hook {
        ctx: &'a mut roxide_eval::Ctx,
        hook: &'h mut dyn super::PersistHook,
        error: Option<roxide_eval::Unwind>,
    },
}

impl std::ops::Deref for CodecSession<'_, '_> {
    type Target = RSession;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Plain(session) => session,
            Self::Hook { ctx, .. } => ctx.session(),
        }
    }
}

impl std::ops::DerefMut for CodecSession<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Plain(session) => session,
            Self::Hook { ctx, .. } => ctx.session_mut(),
        }
    }
}

impl CodecSession<'_, '_> {
    pub(super) fn root(&mut self, value: SexpId) {
        if let Self::Hook { ctx, .. } = self {
            ctx.protect(value);
        }
    }

    pub(super) fn restore(&mut self, key: SexpId) -> WireResult<SexpId> {
        let Self::Hook { ctx, hook, error } = self else {
            return Err(invalid("persistent reference requires a hook"));
        };
        ctx.scoped(|ctx| {
            ctx.protect(key);
            hook.read(ctx, key)
        })
        .map_err(|unwind| {
            *error = Some(unwind);
            invalid("persistent reference hook failed")
        })
    }

    pub(super) fn persist(&mut self, value: SexpId) -> WireResult<Option<SexpId>> {
        let Self::Hook { ctx, hook, error } = self else {
            return Ok(None);
        };
        hook.write(ctx, value).map_err(|unwind| {
            *error = Some(unwind);
            invalid("persistent reference hook failed")
        })
    }

    pub(super) fn finish<T>(&mut self, result: WireResult<T>) -> roxide_eval::EvalResult<T> {
        let Self::Hook { ctx, error, .. } = self else {
            return result.map_err(|error| {
                roxide_eval::Unwind::Internal(roxide_eval::InternalError { message: error.0 })
            });
        };
        if let Some(error) = error.take() {
            return Err(error);
        }
        result.map_err(|error| ctx.stop(SEXP_NIL, error.0))
    }
}

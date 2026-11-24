use roxide_eval::{Ctx, CtxExt, EvalResult, Unwind};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::RSession;

use super::{PersistHook, WireResult, invalid};

#[derive(Default)]
struct CodecRoots(Vec<Vec<SexpId>>);

impl Trace for CodecRoots {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for frame in &self.0 {
            tracer.edges(frame);
        }
    }
}

impl CtxExt for CodecRoots {}

pub(super) enum CodecSession<'a, 'h> {
    Stream(&'a mut RSession),
    Eval {
        ctx: &'a mut Ctx,
        call: SexpId,
        hook: Option<&'h mut dyn PersistHook>,
        error: &'a mut Option<Unwind>,
    },
}

impl std::ops::Deref for CodecSession<'_, '_> {
    type Target = RSession;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Stream(session) => session,
            Self::Eval { ctx, .. } => ctx.session(),
        }
    }
}

impl std::ops::DerefMut for CodecSession<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Stream(session) => session,
            Self::Eval { ctx, .. } => ctx.session_mut(),
        }
    }
}

impl Drop for CodecSession<'_, '_> {
    fn drop(&mut self) {
        if let Self::Eval { ctx, .. } = self {
            ctx.ext::<CodecRoots>().0.pop();
        }
    }
}

impl CodecSession<'_, '_> {
    pub(super) fn run<T>(
        ctx: &mut Ctx,
        call: SexpId,
        hook: Option<&mut dyn PersistHook>,
        codec: impl FnOnce(&mut CodecSession<'_, '_>) -> WireResult<T>,
    ) -> EvalResult<T> {
        ctx.scoped(|ctx| {
            ctx.ext::<CodecRoots>().0.push(vec![call]);
            let mut error = None;
            let result = codec(&mut CodecSession::Eval {
                ctx,
                call,
                hook,
                error: &mut error,
            });
            if let Some(error) = error {
                return Err(error);
            }
            result.map_err(|error| ctx.stop(call, error.0))
        })
    }

    pub(super) fn root(&mut self, value: SexpId) -> WireResult<()> {
        if let Self::Eval { ctx, .. } = self
            && let Some(frame) = ctx.ext::<CodecRoots>().0.last_mut()
        {
            frame.try_reserve(1).map_err(invalid)?;
            frame.push(value);
        }
        Ok(())
    }

    pub(super) fn eval<T>(
        &mut self,
        operation: impl FnOnce(&mut Ctx, SexpId) -> EvalResult<T>,
    ) -> WireResult<T> {
        let Self::Eval {
            ctx, call, error, ..
        } = self
        else {
            return Err(invalid("serialization context is unavailable"));
        };
        operation(ctx, *call).map_err(|unwind| {
            **error = Some(unwind);
            invalid("serialization callback failed")
        })
    }

    pub(super) fn restore(&mut self, key: SexpId) -> WireResult<SexpId> {
        let Self::Eval {
            ctx,
            hook: Some(hook),
            error,
            ..
        } = self
        else {
            return Err(invalid("persistent reference requires a hook"));
        };
        ctx.scoped(|ctx| {
            ctx.protect(key);
            hook.read(ctx, key)
        })
        .map_err(|unwind| {
            **error = Some(unwind);
            invalid("persistent reference hook failed")
        })
    }

    pub(super) fn persist(&mut self, value: SexpId) -> WireResult<Option<SexpId>> {
        let Self::Eval {
            ctx,
            hook: Some(hook),
            error,
            ..
        } = self
        else {
            return Ok(None);
        };
        hook.write(ctx, value).map_err(|unwind| {
            **error = Some(unwind);
            invalid("persistent reference hook failed")
        })
    }
}

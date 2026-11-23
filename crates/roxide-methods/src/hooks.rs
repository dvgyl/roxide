//! Stateless `S4Hooks` bridge delegating into disjoint `roxide-methods` submodules.

use roxide_eval::{Args, Call, Ctx, EvalResult, FrameId, S4Hooks};
use roxide_gc::SexpId;
use roxide_value::Sym;

pub(crate) struct MethodsHooks;

pub(crate) static HOOKS: MethodsHooks = MethodsHooks;

impl S4Hooks for MethodsHooks {
    fn dispatch(
        &self,
        ctx: &mut Ctx,
        generic: Sym,
        call: &Call,
        args: &Args,
    ) -> EvalResult<Option<SexpId>> {
        crate::dispatch::dispatch_primitive(ctx, generic, call, args)
    }

    fn standard_generic(&self, ctx: &mut Ctx, name: Sym, frame: FrameId) -> EvalResult<SexpId> {
        crate::dispatch::standard_generic(ctx, name, frame)
    }

    fn show(&self, ctx: &mut Ctx, value: SexpId, env: SexpId) -> EvalResult<()> {
        crate::show::show_object(ctx, value, env)
    }

    fn class_chain(&self, ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<SexpId>> {
        crate::class::class_chain(ctx, value)
    }

    fn check_slot(
        &self,
        ctx: &mut Ctx,
        obj: SexpId,
        name: Sym,
        value: SexpId,
    ) -> EvalResult<SexpId> {
        crate::class::check_slot(ctx, obj, name, value)
    }
}

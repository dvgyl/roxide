//! Connect the evaluator to the C API.

use std::path::Path;

use roxide_eval::{
    AltOp, AltReply, Args, Call, Ctx, DotCOpts, EvalResult, NativeCallKind, NativeHooks,
};
use roxide_gc::SexpId;

use crate::{api::altrep, dll, dotcall, jump};

struct CapiNativeHooks;

impl NativeHooks for CapiNativeHooks {
    fn altrep(&self, ctx: &mut Ctx, op: AltOp) -> EvalResult<AltReply> {
        altrep::dispatch(ctx, op)
    }

    fn call_native(
        &self,
        ctx: &mut Ctx,
        kind: NativeCallKind,
        symbol: SexpId,
        args: &Args,
        package: Option<&str>,
        dotc: Option<&DotCOpts>,
        op: SexpId,
        env: SexpId,
        call: &Call,
    ) -> EvalResult {
        dotcall::dispatch(
            ctx,
            dotcall::Invocation {
                kind,
                symbol,
                args,
                package,
                dotc,
                op,
                env,
                call,
            },
        )
    }

    fn dyn_load(
        &self,
        ctx: &mut Ctx,
        path: &Path,
        local: bool,
        now: bool,
        call: SexpId,
    ) -> EvalResult {
        dll::load(ctx, path, local, now, call)
    }

    fn dyn_unload(&self, ctx: &mut Ctx, path: &Path, call: SexpId) -> EvalResult<()> {
        dll::unload(ctx, path, call)
    }

    fn is_loaded(
        &self,
        ctx: &mut Ctx,
        symbol: &str,
        package: Option<&str>,
        kind: Option<NativeCallKind>,
    ) -> EvalResult<bool> {
        Ok(ctx
            .ext::<dll::Registry>()
            .lookup(symbol, package, kind.map(Into::into), true)
            .is_some())
    }

    fn get_native_symbol_info(
        &self,
        ctx: &mut Ctx,
        name: &str,
        package: Option<&str>,
        with_registration: bool,
        call: SexpId,
    ) -> EvalResult {
        dll::native_symbol_info(ctx, name, package, with_registration, call)
    }

    fn get_loaded_dlls(&self, ctx: &mut Ctx) -> EvalResult {
        dll::loaded_dlls(ctx)
    }

    fn get_dll_registered_routines(
        &self,
        ctx: &mut Ctx,
        info: SexpId,
        add_names: bool,
        call: SexpId,
    ) -> EvalResult {
        dll::registered_routines(ctx, info, add_names, call)
    }

    fn run_c_finalizer(&self, ctx: &mut Ctx, fun_addr: usize, target: SexpId) -> EvalResult<()> {
        dll::run_finalizer(ctx, fun_addr, target)
    }
}

/// Install the C API native hooks on this context.
pub fn install(ctx: &mut Ctx) {
    ctx.set_native_hooks(&CapiNativeHooks);
    jump::set_altrep_resolver(ctx, Some(altrep::resolve_for_boundary));
}

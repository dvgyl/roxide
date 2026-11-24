use roxide_value::{GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi};

use crate::control::logical;
use crate::{Args, Call, Ctx, EvalResult};

use super::{is_function, scalar_flag};

pub(super) fn register_finalizer(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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
    let onexit = scalar_flag(ctx, ctx.matched_arg(args, &matched, 2), false, call)?;
    ctx.session
        .heap
        .register_finalizer(env, roxide_gc::FinalizerKind::RClosure(fun), onexit);
    Ok(SEXP_NIL)
}

pub(super) fn gc_internal(ctx: &mut Ctx, _call: &Call, _args: &mut Args) -> EvalResult {
    let stats = ctx.collect_gc();
    ctx.run_pending_finalizers();
    let ncells = stats.live_objects as f64;
    let vcells = (stats.live_bytes as f64 / 8.0).ceil();
    let mb_n = (ncells * 56.0) / (1024.0 * 1024.0);
    let mb_v = stats.live_bytes as f64 / (1024.0 * 1024.0);
    let data = [
        ncells,
        vcells,
        (mb_n * 10.0).round() / 10.0,
        (mb_v * 10.0).round() / 10.0,
        ncells.max(1.0),
        vcells.max(1.0),
        (mb_n * 10.0).round() / 10.0,
        (mb_v * 10.0).round() / 10.0,
        roxide_value::NA_REAL,
        roxide_value::NA_REAL,
        ncells,
        vcells,
        (mb_n * 10.0).round() / 10.0,
        (mb_v * 10.0).round() / 10.0,
    ];
    ctx.scoped(|ctx| {
        let mat = ctx.session.alloc_real_from(&data)?;
        ctx.protect(mat);
        let dim = ctx.session.alloc_int_from(&[2, 7])?;
        ctx.protect(dim);
        let dim_sym = ctx.session.intern("dim")?;
        ctx.session.set_attrib(mat, dim_sym, dim)?;
        let rows = ctx.strings(["Ncells", "Vcells"])?;
        ctx.protect(rows);
        let cols = ctx.strings([
            "used",
            "(Mb)",
            "gc trigger",
            "(Mb)",
            "limit (Mb)",
            "max used",
            "(Mb)",
        ])?;
        ctx.protect(cols);
        let dimnames = ctx.session.alloc_list(2)?;
        ctx.protect(dimnames);
        ctx.session.list_set(dimnames, 0, rows)?;
        ctx.session.list_set(dimnames, 1, cols)?;
        let dn_sym = ctx.session.intern("dimnames")?;
        ctx.session.set_attrib(mat, dn_sym, dimnames)?;
        Ok(mat)
    })
}

pub(super) fn gctorture_internal(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let on = scalar_flag(ctx, Some(args.get(0)), true, call)?;
    let prev = ctx.session.heap.wants_collection()
        && matches!(ctx.session.heap.policy(), roxide_gc::GcPolicy::Torture);
    ctx.session.heap.set_policy(if on {
        roxide_gc::GcPolicy::Torture
    } else {
        roxide_gc::GcPolicy::default()
    });
    Ok(ctx.session.scalar_lgl(logical(prev))?)
}

pub(super) fn gctorture2_internal(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let step = ctx.integer(args.get(0), call.err_call())?;
    let prev = if matches!(ctx.session.heap.policy(), roxide_gc::GcPolicy::Torture) {
        1
    } else {
        0
    };
    ctx.session.heap.set_policy(if step > 0 {
        roxide_gc::GcPolicy::Torture
    } else {
        roxide_gc::GcPolicy::default()
    });
    Ok(ctx.session.scalar_int(prev)?)
}

impl Ctx {
    /// Run pending GC finalizers and any remaining `onexit = TRUE` finalizers once,
    /// in reverse registration order, draining any finalizers newly queued during shutdown.
    pub fn run_exit_finalizers(&mut self) {
        const MAX_EXIT_ROUNDS: usize = 16;
        for _ in 0..MAX_EXIT_ROUNDS {
            self.run_pending_finalizers();
            let exit_only = self.session.heap.take_exit_finalizers();
            if exit_only.is_empty() {
                break;
            }
            let _ = self.scoped(|ctx| {
                let visible = ctx.visible;
                for fin in &exit_only {
                    ctx.protect(fin.target);
                    if let roxide_gc::FinalizerKind::RClosure(fun) = fin.kind {
                        ctx.protect(fun);
                    }
                }
                for fin in exit_only.into_iter().rev() {
                    match fin.kind {
                        roxide_gc::FinalizerKind::RClosure(fun) => {
                            let _ = ctx.apply(fun, &[(None, fin.target)], SEXP_NIL, GLOBAL_ENV);
                        }
                        roxide_gc::FinalizerKind::Native { fun, .. } => {
                            if let Some(hooks) = ctx.native_hooks {
                                let _ = hooks.run_c_finalizer(ctx, fun, fin.target);
                            }
                        }
                    }
                }
                ctx.visible = visible;
                Ok(())
            });
            self.run_pending_finalizers();
        }
    }
}

impl Drop for Ctx {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.run_exit_finalizers();
        }
    }
}

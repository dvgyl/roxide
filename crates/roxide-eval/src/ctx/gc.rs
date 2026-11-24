use std::any::{Any, TypeId};
use std::collections::HashMap;

use roxide_gc::{RootSet, SexpId, Trace, Tracer};
use roxide_value::{GLOBAL_ENV, SEXP_NIL, Sym};

use crate::api::{CtxExt, EvalResult};
use crate::namespace::{DeferredS3, NsRecord};

use super::{Ctx, Frame, FrameId};

/// Extension state and its garbage collection trace function.
pub(crate) struct ExtEntry {
    boxed: Box<dyn Any>,
    trace_fn: fn(&dyn Any, &mut Tracer<'_>),
}

struct CtxRoots<'a> {
    frames: &'a [Frame],
    calls: &'a [(SexpId, SexpId)],
    loops: &'a [(FrameId, SexpId)],
    eval_env: SexpId,
    conditions: &'a crate::conditions::Conditions,
    options: &'a [(Sym, SexpId)],
    autoloads: SexpId,
    namespaces: &'a [NsRecord],
    pending_s3: &'a [DeferredS3],
    vm_stack: &'a crate::vm::VmStack,
    extensions: &'a HashMap<TypeId, ExtEntry>,
}

impl RootSet for CtxRoots<'_> {
    fn trace_roots(&self, tracer: &mut Tracer<'_>) {
        for &(call, env) in self.calls {
            tracer.edge(call);
            tracer.edge(env);
        }
        for frame in self.frames {
            tracer.edge(frame.call);
            tracer.edge(frame.cloenv);
            tracer.edge(frame.fun);
            tracer.edge(frame.caller);
            tracer.edges(frame.actuals.values());
            if let Some(dispatch) = &frame.dispatch {
                dispatch.trace(tracer);
            }
            tracer.edges(&frame.on_exit);
        }
        for &(_, env) in self.loops {
            tracer.edge(env);
        }
        tracer.edge(self.eval_env);
        self.conditions.trace(tracer);
        tracer.edge(self.autoloads);
        for rec in self.namespaces {
            rec.trace(tracer);
        }
        for entry in self.pending_s3 {
            entry.trace(tracer);
        }
        for &(_, value) in self.options {
            tracer.edge(value);
        }
        self.vm_stack.trace(tracer);
        for entry in self.extensions.values() {
            (entry.trace_fn)(entry.boxed.as_ref(), tracer);
        }
    }
}

impl Ctx {
    /// Protect a [`SexpId`] on the heap's protect stack.
    pub fn protect(&mut self, id: SexpId) {
        self.session.heap.protect(id);
    }

    /// Protect any R values in an evaluation result during subsequent evaluation.
    ///
    /// This also protects values carried by a non-local transfer, such as an
    /// error condition. Use it before running cleanup that can collect garbage.
    pub fn protect_eval_result(&mut self, result: &EvalResult) {
        self.protect_result(result);
    }

    /// Execute `f` inside a protect-mark scope that restores the protect stack on every exit path.
    pub fn scoped<R>(&mut self, f: impl FnOnce(&mut Self) -> EvalResult<R>) -> EvalResult<R> {
        let mark = self.session.heap.protect_mark();
        let res = f(self);
        self.session.heap.unprotect_to(mark);
        res
    }

    /// Access or lazily initialize per-module GC-traced extension state (ADR-0007).
    pub fn ext<T: CtxExt + Default>(&mut self) -> &mut T {
        let tid = TypeId::of::<T>();
        let entry = self.extensions.entry(tid).or_insert_with(|| ExtEntry {
            boxed: Box::<T>::default(),
            trace_fn: |any, tracer| {
                if let Some(val) = any.downcast_ref::<T>() {
                    val.trace(tracer);
                }
            },
        });
        entry
            .boxed
            .downcast_mut::<T>()
            .unwrap_or_else(|| unreachable!("TypeId key guarantees exact type match"))
    }

    /// Return the extension state if this context has initialized it.
    pub fn ext_ref<T: CtxExt>(&self) -> Option<&T> {
        self.extensions
            .get(&TypeId::of::<T>())?
            .boxed
            .downcast_ref()
    }

    /// Explicit GC safepoint (ADR-0002): runs collection only if policy requests it.
    ///
    /// Does not run queued R/C finalizers directly so C-API allocating paths can call
    /// `safepoint()` safely; evaluator safepoints drain finalizers via [`Ctx::run_pending_finalizers`].
    pub fn safepoint(&mut self) {
        if self.session.heap.wants_collection() {
            let _ = self.collect_gc();
        }
    }

    /// Force an immediate full mark-sweep garbage collection, queuing dead ephemeron finalizers.
    pub fn collect_gc(&mut self) -> roxide_gc::GcStats {
        let roots = CtxRoots {
            frames: &self.frames,
            calls: &self.calls,
            loops: &self.loops,
            eval_env: self.eval_env,
            conditions: &self.conditions,
            options: &self.options,
            autoloads: self.autoloads,
            namespaces: &self.namespaces,
            pending_s3: &self.pending_s3,
            vm_stack: &self.vm_stack,
            extensions: &self.extensions,
        };
        self.session.collect(&roots)
    }

    /// Execute any GC finalizers queued by completed collections (re-entrancy safe, ADR-0019).
    pub fn run_pending_finalizers(&mut self) {
        if self.running_finalizers || !self.session.heap.has_pending_finalizers() {
            return;
        }
        self.running_finalizers = true;
        let visible = self.visible;
        loop {
            let batch = self.session.heap.drain_pending_finalizers();
            if batch.is_empty() {
                break;
            }
            let _ = self.scoped(|ctx| {
                for fin in &batch {
                    ctx.protect(fin.target);
                    if let roxide_gc::FinalizerKind::RClosure(fun) = fin.kind {
                        ctx.protect(fun);
                    }
                }
                for fin in batch.into_iter().rev() {
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
                Ok(())
            });
        }
        self.visible = visible;
        self.running_finalizers = false;
    }
}

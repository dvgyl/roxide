//! Weak references and their finalizers.

use std::collections::HashSet;

use roxide_eval::{Ctx, CtxExt, EvalResult};
use roxide_gc::{FinalizerKind, SexpId, Trace, Tracer};
use roxide_value::{GLOBAL_ENV, SEXP_NIL};

use super::extptr::{CFinalizer, closure_finalizer, native_finalizer};
use super::vector::{allocating, api, id};
use crate::abi::{Rboolean, Sexp};

#[derive(Default)]
struct FinalizedWeakRefs(HashSet<SexpId>);

impl CtxExt for FinalizedWeakRefs {}
impl Trace for FinalizedWeakRefs {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

fn clear_reference(ctx: &mut Ctx, x: SexpId) {
    let mut cleared = std::mem::take(&mut ctx.ext::<FinalizedWeakRefs>().0);
    cleared.retain(|&reference| ctx.session().heap.get(reference).is_ok());
    cleared.insert(x);
    ctx.ext::<FinalizedWeakRefs>().0 = cleared;
}

fn make(
    ctx: &mut Ctx,
    key: Sexp,
    value: Sexp,
    finalizer: Option<FinalizerKind>,
    onexit: Rboolean,
) -> EvalResult<Sexp> {
    let key = id(ctx, key, "R_MakeWeakRef")?;
    let value = id(ctx, value, "R_MakeWeakRef")?;
    let closure = match finalizer {
        Some(FinalizerKind::RClosure(fun)) => fun,
        _ => SEXP_NIL,
    };
    allocating(ctx, &[key, value, closure], |ctx| {
        let value = if key == SEXP_NIL { SEXP_NIL } else { value };
        Ok(Sexp::from_id(ctx.session_mut().alloc_weakref(
            key,
            value,
            finalizer,
            onexit != 0,
        )?))
    })
}

api! {
    /// Make a weak reference with an optional R finalizer.
    fn R_MakeWeakRef(key: Sexp, value: Sexp, finalizer: Sexp, onexit: Rboolean) -> Sexp = Sexp::NULL; |ctx| {
        let fun = id(ctx, finalizer, "R_MakeWeakRef")?;
        let finalizer = closure_finalizer(ctx, fun)?;
        make(ctx, key, value, finalizer, onexit)
    }

    /// Make a weak reference with an optional C finalizer.
    fn R_MakeWeakRefC(key: Sexp, value: Sexp, finalizer: CFinalizer, onexit: Rboolean) -> Sexp = Sexp::NULL; |ctx| {
        make(ctx, key, value, native_finalizer(finalizer), onexit)
    }

    /// Return the live weak key or R NULL.
    fn R_WeakRefKey(reference: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, reference, "R_WeakRefKey")?;
        let key = ctx.session().weakref_key(x)?;
        let cleared = ctx.ext::<FinalizedWeakRefs>().0.contains(&x);
        Ok(Sexp::from_id(if cleared { SEXP_NIL } else { key }))
    }

    /// Return the value associated with a live weak key.
    fn R_WeakRefValue(reference: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, reference, "R_WeakRefValue")?;
        let value = ctx.session().weakref_value(x)?;
        if ctx.ext::<FinalizedWeakRefs>().0.contains(&x) {
            return Ok(Sexp::from_id(SEXP_NIL));
        }
        ctx.session_mut().heap.mark_immutable(value);
        Ok(Sexp::from_id(value))
    }

    /// Clear the weak reference and run its finalizer once.
    fn R_RunWeakRefFinalizer(reference: Sexp) -> () = (); |ctx| {
        let x = id(ctx, reference, "R_RunWeakRefFinalizer")?;
        ctx.session().weakref_key(x)?;
        let Some(entry) = ctx.session_mut().heap.remove_weakref(x) else {
            return Ok(());
        };
        clear_reference(ctx, x);
        let visible = ctx.is_visible();
        let result = ctx.scoped(|ctx| {
            ctx.protect(entry.key);
            match entry.finalizer {
                Some(FinalizerKind::RClosure(fun)) => {
                    ctx.protect(fun);
                    ctx.apply(fun, &[(None, entry.key)], SEXP_NIL, GLOBAL_ENV)?;
                }
                Some(FinalizerKind::Native { fun, .. }) => {
                    if let Some(hooks) = ctx.native_hooks() {
                        hooks.run_c_finalizer(ctx, fun, entry.key)?;
                    }
                }
                None => {}
            }
            Ok(())
        });
        ctx.set_visible(visible);
        result
    }
}

//! External pointers and finalizer registration.

use std::ffi::c_void;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::{FinalizerKind, SexpId};
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use super::vector::{allocating, api, id};
use crate::abi::{Rboolean, Sexp};

/// Nullable C function address stored in an external pointer.
pub type DlFunc = Option<unsafe extern "C" fn() -> *mut c_void>;

/// Nullable C finalizer callback.
pub type CFinalizer = Option<unsafe extern "C" fn(Sexp)>;

fn make(ctx: &mut Ctx, address: usize, tag: Sexp, protect: Sexp) -> EvalResult<Sexp> {
    let tag = id(ctx, tag, "R_MakeExternalPtr")?;
    let protect = id(ctx, protect, "R_MakeExternalPtr")?;
    allocating(ctx, &[tag, protect], |ctx| {
        Ok(Sexp::from_id(
            ctx.session_mut().alloc_extptr(address, tag, protect)?,
        ))
    })
}

pub(super) fn native_finalizer(ctx: &mut Ctx, fun: CFinalizer) -> Option<FinalizerKind> {
    fun.map(|fun| FinalizerKind::Native {
        fun: fun as usize,
        dll: crate::dll::code_owner(ctx, fun as usize, 0),
    })
}

pub(super) fn closure_finalizer(ctx: &mut Ctx, fun: SexpId) -> EvalResult<Option<FinalizerKind>> {
    match ctx.session().type_of(fun)? {
        SexpType::Nil => Ok(None),
        SexpType::Clo => Ok(Some(FinalizerKind::RClosure(fun))),
        _ => Err(ctx.stop(SEXP_NIL, "finalizer must be a function or NULL")),
    }
}

fn register(
    ctx: &mut Ctx,
    x: Sexp,
    kind: Option<FinalizerKind>,
    onexit: Rboolean,
) -> EvalResult<()> {
    let target = id(ctx, x, "R_RegisterFinalizerEx")?;
    if !matches!(
        ctx.session().type_of(target)?,
        SexpType::Env | SexpType::ExtPtr
    ) {
        return Err(ctx.stop(
            SEXP_NIL,
            "finalizer target must be an environment or external pointer",
        ));
    }
    if let Some(kind) = kind {
        crate::dll::remember_finalizer(ctx, target, kind)?;
        if onexit != 0 && matches!(kind, FinalizerKind::Native { dll, .. } if dll != 0) {
            // Keep a weak-reference handle so DLL unload can take this finalizer alone.
            let reference = ctx
                .session_mut()
                .alloc_weakref(target, SEXP_NIL, Some(kind), true)?;
            crate::dll::track_exit_finalizer(ctx, reference, kind);
        } else {
            ctx.session_mut()
                .heap
                .register_finalizer(target, kind, onexit != 0);
        }
    }
    Ok(())
}

api! {
    /// Make an external pointer with a data address.
    fn R_MakeExternalPtr(address: *mut c_void, tag: Sexp, protect: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        make(ctx, address.expose_provenance(), tag, protect)
    }

    /// Make an external pointer with a function address.
    fn R_MakeExternalPtrFn(address: DlFunc, tag: Sexp, protect: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        make(ctx, address.map_or(0, |fun| fun as usize), tag, protect)
    }

    /// Return the data address.
    fn R_ExternalPtrAddr(x: Sexp) -> *mut c_void = std::ptr::null_mut(); |ctx| {
        let x = id(ctx, x, "R_ExternalPtrAddr")?;
        Ok(std::ptr::with_exposed_provenance_mut(ctx.session().extptr_addr(x)?))
    }

    /// Return the function address.
    fn R_ExternalPtrAddrFn(x: Sexp) -> DlFunc = None; |ctx| {
        let x = id(ctx, x, "R_ExternalPtrAddrFn")?;
        let address = ctx.session().extptr_addr(x)?;
        if address == 0 {
            return Ok(None);
        }
        // SAFETY: The caller must use a pointer that stores a C function address.
        Ok(Some(std::mem::transmute::<usize, unsafe extern "C" fn() -> *mut c_void>(address)))
    }

    /// Return the tag object.
    fn R_ExternalPtrTag(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "R_ExternalPtrTag")?;
        Ok(Sexp::from_id(ctx.session().extptr_tag(x)?))
    }

    /// Return the protected object.
    fn R_ExternalPtrProtected(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "R_ExternalPtrProtected")?;
        Ok(Sexp::from_id(ctx.session().extptr_prot(x)?))
    }

    /// Set the address to NULL.
    fn R_ClearExternalPtr(x: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "R_ClearExternalPtr")?;
        ctx.session_mut().set_extptr_addr(x, 0)?;
        Ok(())
    }

    /// Replace the stored address.
    fn R_SetExternalPtrAddr(x: Sexp, address: *mut c_void) -> () = (); |ctx| {
        let x = id(ctx, x, "R_SetExternalPtrAddr")?;
        ctx.session_mut().set_extptr_addr(x, address.expose_provenance())?;
        Ok(())
    }

    /// Replace the tag object.
    fn R_SetExternalPtrTag(x: Sexp, tag: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "R_SetExternalPtrTag")?;
        let tag = id(ctx, tag, "R_SetExternalPtrTag")?;
        ctx.session_mut().set_extptr_tag(x, tag)?;
        Ok(())
    }

    /// Replace the protected object.
    fn R_SetExternalPtrProtected(x: Sexp, protect: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "R_SetExternalPtrProtected")?;
        let protect = id(ctx, protect, "R_SetExternalPtrProtected")?;
        ctx.session_mut().set_extptr_prot(x, protect)?;
        Ok(())
    }

    /// Register a C finalizer for garbage collection.
    fn R_RegisterCFinalizer(x: Sexp, finalizer: CFinalizer) -> () = (); |ctx| {
        let kind = native_finalizer(ctx, finalizer);
        register(ctx, x, kind, 0)
    }

    /// Register a C finalizer for garbage collection and optionally for session exit.
    fn R_RegisterCFinalizerEx(x: Sexp, finalizer: CFinalizer, onexit: Rboolean) -> () = (); |ctx| {
        let kind = native_finalizer(ctx, finalizer);
        register(ctx, x, kind, onexit)
    }

    /// Register an R closure as a garbage collection finalizer.
    fn R_RegisterFinalizer(x: Sexp, finalizer: Sexp) -> () = (); |ctx| {
        let fun = id(ctx, finalizer, "R_RegisterFinalizer")?;
        let kind = closure_finalizer(ctx, fun)?;
        register(ctx, x, kind, 0)
    }

    /// Register an R closure as a finalizer that can also run at session exit.
    fn R_RegisterFinalizerEx(x: Sexp, finalizer: Sexp, onexit: Rboolean) -> () = (); |ctx| {
        let fun = id(ctx, finalizer, "R_RegisterFinalizerEx")?;
        let kind = closure_finalizer(ctx, fun)?;
        register(ctx, x, kind, onexit)
    }

    /// Run the finalizers queued by garbage collection.
    fn R_RunPendingFinalizers() -> () = (); |ctx| {
        ctx.run_pending_finalizers();
        Ok(())
    }

    /// Run the finalizers registered for session exit.
    fn R_RunExitFinalizers() -> () = (); |ctx| {
        ctx.run_exit_finalizers();
        Ok(())
    }
}

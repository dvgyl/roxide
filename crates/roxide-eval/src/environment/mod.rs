mod bindings;
mod frames;
mod memory;
mod namespaces;
mod objects;
mod variables;

use roxide_gc::SexpId;
use roxide_value::{RLogical, SexpType, ValueApi};

use crate::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility,
};

/// Register environment and call stack primitives.
pub(crate) const MODULE: Module = Module {
    prims: &[
        primitive!("assign", variables::assign, Eval, Arity::Any, Off),
        primitive!("get", variables::get, Eval, Arity::Any, On),
        primitive!("get0", variables::get, Eval, Arity::Any, On),
        primitive!("mget", variables::mget, Eval, Arity::Range(2, 5), On),
        primitive!(
            "list2env",
            objects::list_to_env,
            Eval,
            Arity::Range(1, 5),
            On
        ),
        primitive!(
            "as.environment",
            objects::as_environment,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "reg.finalizer",
            memory::register_finalizer,
            Eval,
            Arity::Range(2, 3),
            On
        ),
        primitive!(
            "activeBindingFunction",
            bindings::active_binding_function,
            Eval,
            Arity::Exact(2),
            On
        ),
        primitive!(
            "delayedAssign",
            variables::delayed_assign,
            Unevaluated,
            Arity::Range(2, 4),
            Off
        ),
        primitive!("objects", variables::ls, Eval, Arity::Any, On),
        primitive!("sys.calls", frames::sys_stack, Eval, Arity::Exact(0), On),
        primitive!("sys.frames", frames::sys_stack, Eval, Arity::Exact(0), On),
        primitive!(
            "sys.parent",
            frames::sys_stack,
            Eval,
            Arity::Range(0, 1),
            On
        ),
        primitive!("sys.parents", frames::sys_stack, Eval, Arity::Exact(0), On),
        primitive!("sys.on.exit", frames::sys_stack, Eval, Arity::Exact(0), On),
        primitive!("exists", variables::get, Eval, Arity::Any, On),
        primitive!("rm", variables::remove, Unevaluated, Arity::Any, Off),
        primitive!("remove", variables::remove, Unevaluated, Arity::Any, Off),
        primitive!("ls", variables::ls, Eval, Arity::Any, On),
        primitive!(
            "makeActiveBinding",
            bindings::make_active_binding,
            Eval,
            Arity::Exact(3),
            Off
        ),
        primitive!("new.env", objects::new_env, Eval, Arity::Range(0, 3), On),
        primitive!("emptyenv", objects::constant_env, Eval, Arity::Exact(0), On),
        primitive!("baseenv", objects::constant_env, Eval, Arity::Exact(0), On),
        primitive!(
            "globalenv",
            objects::constant_env,
            Eval,
            Arity::Exact(0),
            On
        ),
        primitive!(
            "environment",
            objects::environment,
            Eval,
            Arity::Range(0, 1),
            On
        ),
        primitive!("environment<-", objects::set_env, Eval, Arity::Exact(2), On),
        primitive!("parent.env", objects::parent_env, Eval, Arity::Exact(1), On),
        primitive!(
            "parent.env<-",
            objects::set_parent_env,
            Eval,
            Arity::Exact(2),
            On
        ),
        primitive!("search", objects::search, Eval, Arity::Exact(0), On),
        primitive!(
            "lockEnvironment",
            bindings::lock_environment,
            Eval,
            Arity::Range(1, 2),
            Off
        ),
        primitive!(
            "environmentIsLocked",
            bindings::env_is_locked,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "lockBinding",
            bindings::lock_binding,
            Eval,
            Arity::Exact(2),
            Off
        ),
        primitive!(
            "unlockBinding",
            bindings::unlock_binding,
            Eval,
            Arity::Exact(2),
            Off
        ),
        primitive!(
            "bindingIsLocked",
            bindings::binding_is_locked,
            Eval,
            Arity::Exact(2),
            On
        ),
        primitive!(
            "bindingIsActive",
            bindings::binding_is_active,
            Eval,
            Arity::Exact(2),
            On
        ),
        primitive!(
            "asNamespace",
            namespaces::as_namespace,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "isNamespace",
            namespaces::is_namespace,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "loadedNamespaces",
            namespaces::loaded_namespaces,
            Eval,
            Arity::Exact(0),
            On
        ),
        primitive!("isTRUE", is_true_false, Eval, Arity::Exact(1), On),
        primitive!("isFALSE", is_true_false, Eval, Arity::Exact(1), On),
        primitive!("as.name", as_name, Eval, Arity::Exact(1), On),
        primitive!("as.symbol", as_name, Eval, Arity::Exact(1), On),
        primitive!("topenv", objects::topenv, Eval, Arity::Range(0, 2), On),
        primitive!(
            "parent.frame",
            frames::parent_frame,
            Eval,
            Arity::Range(0, 1),
            On
        ),
        primitive!("sys.call", frames::sys_frame, Eval, Arity::Range(0, 1), On),
        primitive!(
            "sys.function",
            frames::sys_frame,
            Eval,
            Arity::Range(0, 1),
            On
        ),
        primitive!("sys.frame", frames::sys_frame, Eval, Arity::Range(0, 1), On),
        primitive!("sys.nframe", frames::sys_nframe, Eval, Arity::Exact(0), On),
        primitive!(
            "::",
            namespaces::namespace,
            Unevaluated,
            Arity::Exact(2),
            On
        ),
        primitive!(
            ":::",
            namespaces::namespace,
            Unevaluated,
            Arity::Exact(2),
            On
        ),
        PrimDef {
            name: "rox_gc",
            fun: memory::gc_internal,
            kind: PrimKind::Internal,
            args: ArgEval::Eval,
            arity: Arity::Exact(3),
            dispatch: Dispatch::None,
            visibility: Visibility::On,
        },
        PrimDef {
            name: "rox_gctorture",
            fun: memory::gctorture_internal,
            kind: PrimKind::Internal,
            args: ArgEval::Eval,
            arity: Arity::Exact(1),
            dispatch: Dispatch::None,
            visibility: Visibility::Off,
        },
        PrimDef {
            name: "rox_gctorture2",
            fun: memory::gctorture2_internal,
            kind: PrimKind::Internal,
            args: ArgEval::Eval,
            arity: Arity::Exact(3),
            dispatch: Dispatch::None,
            visibility: Visibility::On,
        },
    ],
    prelude: concat!(
        "gc <- function(verbose = getOption(\"verbose\"), reset = FALSE, full = TRUE) .Internal(rox_gc(verbose, reset, full))\n",
        "gctorture <- function(on = TRUE) .Internal(rox_gctorture(on))\n",
        "gctorture2 <- function(step, wait = step, inhibit_release = FALSE) .Internal(rox_gctorture2(step, wait, inhibit_release))\n",
    ),
};

fn scalar_flag(
    ctx: &mut Ctx,
    value: Option<SexpId>,
    default: bool,
    call: &Call,
) -> EvalResult<bool> {
    match value {
        Some(value) => ctx.truth(value, call.err_call()),
        None => Ok(default),
    }
}

fn is_function(ctx: &Ctx, value: SexpId) -> EvalResult<bool> {
    Ok(matches!(
        ctx.session.type_of(value)?,
        SexpType::Clo | SexpType::Builtin | SexpType::Special
    ))
}

fn is_true_false(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let want = ctx.prims[usize::from(call.prim.0)].name == "isTRUE";
    let x = args.get(0);
    let ok = ctx.session.type_of(x)? == SexpType::Lgl
        && ctx.session.xlength(x)? == 1
        && ctx.session.lgl(x)?.get(0).get() == Some(want);
    Ok(ctx.session.scalar_lgl(RLogical::from_option(Some(ok)))?)
}

fn as_name(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    if ctx.session.type_of(x)? == SexpType::Sym {
        return Ok(x);
    }
    let (coerced, _) = ctx.session.coerce(x, SexpType::Str)?;
    let name = ctx.string(coerced, call.err_call())?;
    if name.is_empty() {
        return Err(ctx.stop(call.err_call(), "attempt to use zero-length variable name"));
    }
    Ok(ctx.session.intern(&name)?.as_sexp())
}

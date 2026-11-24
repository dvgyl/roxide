use std::collections::HashMap;

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, CtxExt, Dispatch, EvalResult, InternalError, Module, PrimDef,
    PrimKind, Unwind, Visibility,
};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{BASE_ENV, ValueApi};

use crate::ops::primitive;

const MODULE: Module = Module {
    prims: &[
        primitive!("dput", dput, Eval, Arity::Range(1, 3), Dispatch::None, Off),
        primitive!("deparse", deparse, Eval, Arity::Any, Dispatch::None, On),
        primitive!(
            "deparse1",
            deparse_one,
            Eval,
            Arity::Any,
            Dispatch::None,
            On
        ),
    ],
    prelude: "",
};

#[derive(Default)]
struct OriginalFormatters(HashMap<&'static str, SexpId>);

impl Trace for OriginalFormatters {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for &value in self.0.values() {
            tracer.edge(value);
        }
    }
}

impl CtxExt for OriginalFormatters {}

/// Keep the evaluator formatters and resolve nested ALTREP inputs before calling them.
pub(crate) fn register(ctx: &mut Ctx) -> Result<(), InternalError> {
    for def in MODULE.prims {
        let name = ctx.session_mut().intern(def.name)?;
        let original = ctx
            .session()
            .env_get(BASE_ENV, name)
            .ok_or_else(|| InternalError {
                message: format!("missing formatter '{}'", def.name),
            })?;
        ctx.ext::<OriginalFormatters>()
            .0
            .insert(def.name, original.value);
    }
    ctx.register_module(&MODULE)
}

fn dput(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    format(ctx, call, args, "dput")
}

fn deparse(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    format(ctx, call, args, "deparse")
}

fn deparse_one(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    format(ctx, call, args, "deparse1")
}

fn format(ctx: &mut Ctx, call: &Call, args: &Args, name: &str) -> EvalResult {
    let original = ctx
        .ext::<OriginalFormatters>()
        .0
        .get(name)
        .copied()
        .ok_or_else(|| {
            Unwind::Internal(InternalError {
                message: "missing evaluator formatter".into(),
            })
        })?;
    for &arg in args.values() {
        ctx.resolve_altrep_deep(arg)?;
    }
    let values: Vec<_> = args.iter().collect();
    ctx.apply(original, &values, call.err_call(), call.env)
}

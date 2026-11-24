use roxide_eval::{Args, Call, Ctx, DotCOpts, EvalResult, NativeCallKind};
use roxide_value::{BASE_ENV, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use super::{character_scalar, hooks, logical, package};
use crate::ops::common::tag;

pub(super) fn dot_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    invoke(ctx, call, args, NativeCallKind::Call)
}

pub(super) fn external(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    invoke(ctx, call, args, NativeCallKind::External)
}

pub(super) fn external2(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    invoke(ctx, call, args, NativeCallKind::External2)
}

pub(super) fn dot_c(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    invoke(ctx, call, args, NativeCallKind::C)
}

pub(super) fn fortran(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    invoke(ctx, call, args, NativeCallKind::Fortran)
}

fn invoke(ctx: &mut Ctx, call: &Call, args: &Args, kind: NativeCallKind) -> EvalResult {
    let symbol_at = args
        .iter()
        .position(|(name, _)| tag(ctx.session(), name) == Some(".NAME"))
        .or_else(|| args.iter().position(|(name, _)| name.is_none()));
    let Some(symbol_at) = symbol_at.filter(|&i| args.get(i) != SEXP_MISSING) else {
        return Err(ctx.stop(call.err_call(), "argument '.NAME' is missing"));
    };
    let mut package_name = None;
    let mut dotc =
        matches!(kind, NativeCallKind::C | NativeCallKind::Fortran).then(DotCOpts::default);
    let mut native_args = Vec::with_capacity(args.len() - 1);
    for (i, (name, value)) in args.iter().enumerate() {
        if i == symbol_at {
            continue;
        }
        match (tag(ctx.session(), name), &mut dotc) {
            (Some("PACKAGE"), _) => package_name = package(ctx, call, value)?,
            (Some("NAOK"), Some(opts)) => opts.naok = logical(ctx, call, value, "NAOK")?,
            (Some("DUP"), Some(opts)) => opts.dup = logical(ctx, call, value, "DUP")?,
            (Some("ENCODING"), Some(opts)) => {
                let (value, _) = ctx.session_mut().coerce(value, SexpType::Str)?;
                opts.encoding = Some(character_scalar(ctx, call, value, "ENCODING")?);
            }
            _ => native_args.push((name, value)),
        }
    }
    let op = if kind == NativeCallKind::External2 {
        let sym = ctx.session_mut().intern(".External2")?;
        ctx.get_var(sym, BASE_ENV)?
            .ok_or_else(|| ctx.stop(call.err_call(), "base .External2 is unavailable"))?
    } else {
        SEXP_NIL
    };
    hooks(ctx).call_native(
        ctx,
        kind,
        args.get(symbol_at),
        &Args::from_pairs(native_args),
        package_name.as_deref(),
        dotc.as_ref(),
        op,
        call.env,
        call,
    )
}

use roxide_eval::{Args, Call, Ctx, DotCOpts, EvalResult, NativeCallKind};
use roxide_gc::SexpId;
use roxide_value::{BASE_ENV, RLogical, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use super::hooks;
use crate::ops::common::{chars, tag, text};

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
    if args.is_empty() {
        return Err(ctx.stop(call.err_call(), "'.NAME' is missing"));
    }
    if let Some(index) = args
        .values()
        .iter()
        .position(|&value| value == SEXP_MISSING)
    {
        return Err(ctx.stop(call.err_call(), format!("argument {} is empty", index + 1)));
    }
    if args.name(0).is_some() {
        return Err(ctx.stop(call.err_call(), "the first argument should not be named"));
    }

    let symbol = args.get(0);
    let uses_dotc = matches!(kind, NativeCallKind::C | NativeCallKind::Fortran);
    let mut package = None;
    let mut naok = None;
    let (mut package_count, mut naok_count, mut dup_count) = (0, 0, 0);
    let mut native_args = Vec::with_capacity(args.len() - 1);
    for (name, value) in args.iter().skip(1) {
        match tag(ctx.session(), name) {
            Some("PACKAGE") => {
                warn_on_second_control(ctx, call, "PACKAGE", &mut package_count)?;
                let &[id] = chars(ctx.session(), value) else {
                    return Err(ctx.stop(
                        call.err_call(),
                        "PACKAGE argument must be a single character string",
                    ));
                };
                package = Some(id);
            }
            Some("NAOK") if uses_dotc => {
                warn_on_second_control(ctx, call, "NAOK", &mut naok_count)?;
                naok = Some(value);
            }
            Some("DUP") if uses_dotc => {
                warn_on_second_control(ctx, call, "DUP", &mut dup_count)?;
            }
            Some("ENCODING") if uses_dotc => {
                ctx.warning_at(call.err_call(), "ENCODING is defunct and will be ignored")?;
            }
            _ => native_args.push((name, value)),
        }
    }
    let mut dotc = uses_dotc.then(DotCOpts::default);
    if let (Some(opts), Some(value)) = (&mut dotc, naok) {
        opts.naok = naok_flag(ctx, call, value)?;
    }
    let package_name = match (ctx.session().type_of(symbol)?, package) {
        (SexpType::Str, Some(id)) => {
            let name = text(ctx.session(), id);
            if name.is_empty() {
                return Err(ctx.stop(call.err_call(), "PACKAGE = \"\" is invalid"));
            }
            Some(name)
        }
        _ => None,
    };
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
        symbol,
        &Args::from_pairs(native_args),
        package_name.as_deref(),
        dotc.as_ref(),
        op,
        call.env,
        call,
    )
}

fn warn_on_second_control(
    ctx: &mut Ctx,
    call: &Call,
    name: &str,
    count: &mut u8,
) -> EvalResult<()> {
    *count = count.saturating_add(1);
    if *count == 2 {
        ctx.warn_now(call.err_call(), format!("'{name}' used more than once"))?;
    }
    Ok(())
}

fn naok_flag(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<bool> {
    let invalid = "invalid 'naok' value";
    if !matches!(
        ctx.session().type_of(value)?,
        SexpType::Lgl
            | SexpType::Int
            | SexpType::Real
            | SexpType::Cplx
            | SexpType::Str
            | SexpType::Raw
    ) {
        return Err(ctx.stop(call.err_call(), invalid));
    }
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
    let values = ctx.session().lgl(value)?;
    let flag = values.iter().next().and_then(RLogical::get);
    flag.ok_or_else(|| ctx.stop(call.err_call(), invalid))
}

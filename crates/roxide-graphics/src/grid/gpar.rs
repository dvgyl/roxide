//! Device defaults and graphical parameters for text metrics.

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use crate::engine::{Device, GContext};
use crate::grdevices::parse_rgba;

pub(super) fn pointsize(device: &dyn Device) -> f64 {
    let reference = device
        .metric_info(
            'M',
            &GContext {
                ps: 1.0,
                ..GContext::default()
            },
        )
        .2;
    let width = device
        .metric_info(
            'M',
            &GContext {
                ps: 0.0,
                ..GContext::default()
            },
        )
        .2;
    if reference > 0.0 && width > 0.0 {
        (width / reference * 1e10).round() / 1e10
    } else {
        GContext::default().ps
    }
}

pub(super) fn text_context(
    ctx: &mut Ctx,
    call: &Call,
    params: SexpId,
    family: SexpId,
) -> EvalResult<GContext> {
    let params = super::numbers(ctx, call, params)?;
    let [size, expansion, font, lineheight] = params.as_slice() else {
        return Err(ctx.stop(call.err_call(), "invalid text parameters"));
    };
    let (family, _) = ctx.session_mut().coerce(family, SexpType::Str)?;
    let family = ctx.session().str_slice(family)?.first().copied();
    let fontfamily = family
        .map(|id| {
            ctx.session()
                .char_bytes(id)
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        })
        .transpose()?
        .unwrap_or_default();
    // A zero effective size must give zero metrics even on devices with a default point size.
    let (ps, cex) = if *size == 0.0 {
        (1.0, 0.0)
    } else {
        (*size, *expansion)
    };
    Ok(GContext {
        ps,
        cex,
        fontface: *font as u8,
        fontfamily,
        lineheight: *lineheight,
        ..GContext::default()
    })
}

pub(super) fn validate_colors(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    parse_rgba(ctx, args.get(0))?;
    parse_rgba(ctx, args.get(1))?;
    Ok(SEXP_NIL)
}

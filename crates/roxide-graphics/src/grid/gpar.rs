//! Device defaults and graphical parameters for text metrics.

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, ValueApi};

use crate::engine::{Device, GContext};

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

pub(super) fn inherit(ctx: &mut Ctx, parent: SexpId, local: SexpId) -> EvalResult {
    use super::fields::*;

    let mut entries = Vec::new();
    let names = attr(ctx, parent, "names")?;
    for i in 0..ctx.session().xlength(names)? {
        let Some(name) = text(ctx, names, i)? else {
            continue;
        };
        let old = ctx.session().list_get(parent, i)?;
        let new = get(ctx, local, &name)?;
        let value = if new == SEXP_NIL {
            old
        } else if matches!(name.as_str(), "alpha" | "cex" | "lex") {
            let n = ctx.session().xlength(old)?.max(ctx.session().xlength(new)?);
            let mut values = Vec::with_capacity(n);
            for index in 0..n {
                values.push(real(ctx, old, index, 1.0)? * real(ctx, new, index, 1.0)?);
            }
            ctx.session_mut().alloc_real_from(&values)?
        } else {
            new
        };
        entries.push((name, value));
    }
    let fields: Vec<_> = entries
        .iter()
        .map(|(name, value)| (name.as_str(), *value))
        .collect();
    let result = named(ctx, &fields)?;
    let class = attr(ctx, parent, "class")?;
    attrib(ctx, result, "class", class)?;
    Ok(result)
}

pub(super) fn merge(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    inherit(ctx, args.get(0), args.get(1))
}

pub(super) fn defaults(ctx: &mut Ctx, size: f64) -> EvalResult {
    use super::fields::*;
    let mut entries = Vec::new();
    for (name, value) in [
        ("fill", "transparent"),
        ("col", "black"),
        ("lty", "solid"),
        ("fontfamily", ""),
        ("lineend", "round"),
        ("linejoin", "round"),
    ] {
        entries.push((name, ctx.session_mut().scalar_str(value)?));
    }
    for (name, value) in [
        ("lwd", 1.0),
        ("cex", 1.0),
        ("fontsize", size),
        ("lineheight", 1.2),
        ("alpha", 1.0),
        ("linemitre", 10.0),
        ("lex", 1.0),
    ] {
        entries.push((name, ctx.session_mut().scalar_real(value)?));
    }
    entries.push(("font", ctx.session_mut().scalar_int(1)?));
    let order = [
        "fill",
        "col",
        "lty",
        "lwd",
        "cex",
        "fontsize",
        "lineheight",
        "font",
        "fontfamily",
        "alpha",
        "lineend",
        "linejoin",
        "linemitre",
        "lex",
    ];
    let mut gp = Vec::new();
    for name in order {
        if let Some(entry) = entries.iter().find(|(field, _)| name == *field) {
            gp.push(*entry);
        }
    }
    let gp = named(ctx, &gp)?;
    let class = strings(ctx, &["gpar"])?;
    attrib(ctx, gp, "class", class)?;
    Ok(gp)
}

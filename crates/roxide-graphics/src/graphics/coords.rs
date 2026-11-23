use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{NA_REAL, SEXP_NIL, ValueApi};

use crate::engine::{GraphicsExt, Rect};

use super::{
    active, flag, nums,
    par::{GPar, Geometry, size, state},
    record, string, strings,
};

pub(super) fn plot_new(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    let slot = active(ctx, call, true)?;
    let initialized = slot.frame_open;
    let par = state(slot)?;
    for name in ["xaxs", "yaxs"] {
        let style = par.text(name);
        if !matches!(style, "r" | "i") {
            let message = format!("axis style \"{style}\" unimplemented");
            return Err(ctx.stop(call.err_call(), message));
        }
    }
    let start_page = !initialized || par.page();
    let geom = par.next_geometry(initialized);
    let bg = par.color("bg");
    let figure = geom.figure;
    let device = geom.device;
    if figure[0] < -1e-10
        || figure[1] > device[1] + 1e-10
        || figure[2] < -1e-10
        || figure[3] > device[3] + 1e-10
    {
        return Err(ctx.stop(call.err_call(), "figure region too large"));
    }
    if size(figure).iter().any(|x| !x.is_finite() || *x <= 0.0) {
        return Err(ctx.stop(
            call.err_call(),
            "outer margins too large (figure region too small)",
        ));
    }
    if size(geom.plot).iter().any(|x| !x.is_finite() || *x <= 0.0) {
        return Err(ctx.stop(call.err_call(), "figure margins too large"));
    }
    if start_page {
        let recording = active(ctx, call, false)?.recording;
        let initial = if recording {
            super::par::snapshot(ctx, call)?
        } else {
            SEXP_NIL
        };
        let id = ctx.ext::<GraphicsExt>().current;
        // The backend uses only Host. No garbage collection can occur while the slot is out.
        let mut slot = ctx.ext::<GraphicsExt>().slots[id]
            .take()
            .ok_or_else(|| ctx.stop(call.err_call(), "no active graphics device"))?;
        let result = slot.device.new_page(ctx.host(), bg);
        if result.is_ok() {
            slot.page_count += 1;
            slot.display_list.clear();
            slot.initial_state = initial;
        }
        ctx.ext::<GraphicsExt>().slots[id] = Some(slot);
        result.map_err(|error| ctx.stop(call.err_call(), error.to_string()))?;
    }
    let slot = active(ctx, call, false)?;
    let par = state(slot)?;
    par.advance(initialized);
    par.log = [false; 2];
    for (axis, name) in ["xaxs", "yaxs"].into_iter().enumerate() {
        let range = expand([0.0, 1.0], par.text(name) == "r");
        par.set_usr(axis, range);
        par.axp[axis] = linear_ticks(range, par.axis_count(axis));
    }
    slot.frame_open = true;
    slot.device.clip(rect(geom.plot));
    record(ctx, call, "C_rx_plot_new", &[])?;
    Ok(SEXP_NIL)
}

pub(super) fn check_frame(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    require_frame(ctx, call)?;
    Ok(SEXP_NIL)
}

pub(super) fn require_frame(ctx: &mut Ctx, call: &Call) -> EvalResult<()> {
    if !active(ctx, call, false)?.frame_open {
        return Err(ctx.stop(call.err_call(), "plot.new has not been called yet"));
    }
    Ok(())
}

pub(super) fn rect([l, r, b, t]: [f64; 4]) -> Rect {
    Rect::normalized(l * 72.0, b * 72.0, r * 72.0, t * 72.0)
}

fn expand([mut lo, mut hi]: [f64; 2], regular: bool) -> [f64; 2] {
    let max = lo.abs().max(hi.abs());
    let d = hi - lo;
    if d == 0.0 {
        let delta = if max == 0.0 { 1.0 } else { 0.4 * max };
        lo -= delta;
        hi += delta;
    } else if d.abs() < 16.0 * f64::EPSILON * max {
        let delta = (0.01 * max).copysign(d);
        lo -= delta;
        hi += delta;
    }
    if regular {
        let margin = 0.04 * hi - 0.04 * lo;
        lo = (lo - margin).max(-f64::MAX);
        hi = (hi + margin).min(f64::MAX);
    }
    [lo, hi]
}

pub(super) fn linear_ticks(mut range: [f64; 2], count: f64) -> [f64; 3] {
    let reverse = range[0] > range[1];
    if reverse {
        range.swap(0, 1);
    }
    let [lo, hi] = range;
    let cell = (hi / count.max(1.0) - lo / count.max(1.0)).max(f64::MIN_POSITIVE);
    let base = 10.0_f64.powf(cell.log10().floor());
    let mut step = base;
    for (candidate, bias) in [(2.0 * base, 0.8), (5.0 * base, 1.7), (10.0 * base, 0.8)] {
        if candidate - cell >= bias * (cell - step) {
            break;
        }
        step = candidate;
    }
    let start = (lo / step - 1e-10).ceil();
    let end = (hi / step + 1e-10).floor();
    let (mut a, mut b, n) = if end > start && step.is_finite() {
        (start * step, end * step, end - start)
    } else {
        (lo, hi, 1.0)
    };
    if reverse {
        std::mem::swap(&mut a, &mut b);
    }
    [a, b, n]
}

fn log_ticks(range: [f64; 2], count: f64) -> [f64; 3] {
    let (lo, hi) = (range[0].min(range[1]), range[0].max(range[1]));
    if hi - lo < 1.0 {
        let [a, b, n] = linear_ticks(range.map(|x| 10.0_f64.powf(x)), count);
        return [a, b, -n];
    }
    let mut lo_pow = (lo - 1e-10).ceil();
    let mut hi_pow = (hi + 1e-10).floor();
    if hi_pow <= lo_pow {
        lo_pow = lo.round();
        hi_pow = hi.round();
    }
    lo_pow = lo_pow.max(f64::MIN_POSITIVE.log10().ceil());
    hi_pow = hi_pow.min(f64::MAX.log10().floor());
    let n = match (hi_pow - lo_pow) as i32 {
        ..=2 => 3.0,
        3 => 2.0,
        _ => 1.0,
    };
    let (a, b) = (10.0_f64.powf(lo_pow), 10.0_f64.powf(hi_pow));
    if range[0] <= range[1] {
        [a, b, n]
    } else {
        [b, a, n]
    }
}

fn limits(ctx: &mut Ctx, call: &Call, id: roxide_gc::SexpId, name: &str) -> EvalResult<[f64; 2]> {
    let v = nums(ctx, id)?;
    let Ok(v) = <[f64; 2]>::try_from(v) else {
        return Err(ctx.stop(call.err_call(), format!("invalid '{name}' value")));
    };
    if v.iter().any(|x| !x.is_finite()) {
        return Err(ctx.stop(call.err_call(), format!("need finite '{name}' values")));
    }
    Ok(v)
}

pub(super) fn plot_window(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let log = string(ctx, args.get(2))?;
    let mut log = match log.as_str() {
        "" => [false, false],
        "x" => [true, false],
        "y" => [false, true],
        "xy" | "yx" => [true, true],
        _ => return Err(ctx.stop(call.err_call(), r#"invalid "log=" specification"#)),
    };
    let asp = nums(ctx, args.get(3))?.first().copied().unwrap_or(NA_REAL);
    let mut xy = [
        limits(ctx, call, args.get(0), "xlim")?,
        limits(ctx, call, args.get(1), "ylim")?,
    ];
    require_frame(ctx, call)?;
    let old_log = state(active(ctx, call, false)?)?.log;
    for (axis, range) in xy.iter_mut().enumerate() {
        log[axis] |= old_log[axis];
        if log[axis] {
            if range.iter().any(|&x| x < 0.0) {
                return Err(ctx.stop(
                    call.err_call(),
                    "Logarithmic axis must have positive limits",
                ));
            }
            if range.contains(&0.0) {
                let format_limit = |x: f64| {
                    if x == 0.0 {
                        "-inf".into()
                    } else {
                        format!("{:.6}", x.log10())
                            .trim_end_matches('0')
                            .trim_end_matches('.')
                            .to_owned()
                    }
                };
                let (a, b) = (format_limit(range[0]), format_limit(range[1]));
                ctx.warn_now(
                    call.err_call(),
                    format!(
                        "nonfinite axis={} limits [GScale({a},{b},..); log=TRUE] -- corrected now",
                        axis + 1,
                    ),
                )?;
            }
            *range = range.map(|x| if x == 0.0 { -320.0 } else { x.log10() });
        }
    }
    let options = args.get(4);
    let names = super::names(ctx, options)?;
    let mut styles = [None, None];
    let mut counts = None;
    for (i, name) in names.iter().enumerate() {
        let option = super::element(ctx, options, i)?;
        match name.as_deref() {
            Some("xaxs" | "yaxs") => {
                styles[usize::from(name.as_deref() == Some("yaxs"))] = Some(string(ctx, option)?);
            }
            Some("lab") => counts = Some(nums(ctx, option)?),
            _ => {}
        }
    }
    let par = state(active(ctx, call, false)?)?;
    for (axis, name) in ["xaxs", "yaxs"].into_iter().enumerate() {
        let style = styles[axis].as_deref().unwrap_or_else(|| par.text(name));
        if !matches!(style, "r" | "i") {
            let message = format!("axis style \"{style}\" unimplemented");
            return Err(ctx.stop(call.err_call(), message));
        }
    }
    if asp.is_finite() && asp > 0.0 && !log[0] && !log[1] {
        let [w, h] = size(par.geometry().plot);
        let dx = (xy[0][1] - xy[0][0]).abs();
        let dy = (xy[1][1] - xy[1][0]).abs();
        if dx > 0.0 || dy > 0.0 {
            let unit = (dx / w).max(dy * asp / h);
            let half = [w * unit * 0.5, h * unit * 0.5 / asp];
            for (i, range) in xy.iter_mut().enumerate() {
                let mid = range[0] * 0.5 + range[1] * 0.5;
                let extension = half[i].copysign(range[1] - range[0]);
                *range = [mid - extension, mid + extension];
            }
        }
    }
    par.log = log;
    for (axis, name) in ["xaxs", "yaxs"].into_iter().enumerate() {
        let style = styles[axis].as_deref().unwrap_or_else(|| par.text(name));
        let mut range = expand(xy[axis], style == "r");
        if log[axis] {
            if 10.0_f64.powf(range[0]) == 0.0 {
                range[0] = (1.01 * f64::MIN_POSITIVE).log10();
            }
            if !10.0_f64.powf(range[1]).is_finite() {
                range[1] = (0.99 * f64::MAX).log10();
            }
        }
        let count = counts
            .as_ref()
            .and_then(|x| x.get(axis))
            .copied()
            .unwrap_or_else(|| par.axis_count(axis));
        par.axp[axis] = if log[axis] {
            log_ticks(range, count)
        } else {
            linear_ticks(range, count)
        };
        par.set_usr(axis, range);
    }
    record(
        ctx,
        call,
        "C_rx_plot_window",
        &[args.get(0), args.get(1), args.get(2), args.get(3), options],
    )?;
    Ok(SEXP_NIL)
}

#[derive(Clone, Copy, PartialEq)]
enum Units {
    User,
    Inches,
    Device,
    Ndc,
    Nfc,
    Npc,
    Nic,
    Lines,
    Chars,
}

fn units(ctx: &mut Ctx, call: &Call, id: roxide_gc::SexpId, arg: &str) -> EvalResult<Units> {
    let name = string(ctx, id)?;
    let mut matches = [
        ("user", Units::User),
        ("inches", Units::Inches),
        ("device", Units::Device),
        ("ndc", Units::Ndc),
        ("nfc", Units::Nfc),
        ("npc", Units::Npc),
        ("nic", Units::Nic),
        ("lines", Units::Lines),
        ("chars", Units::Chars),
    ]
    .into_iter()
    .filter(|(s, _)| !name.is_empty() && s.starts_with(&name));
    match (matches.next(), matches.next()) {
        (Some((_, unit)), None) => Ok(unit),
        _ => Err(ctx.stop(call.err_call(), format!("invalid '{arg}' argument"))),
    }
}

fn transform(unit: Units, g: Geometry, par: &GPar, axis: usize) -> (f64, f64) {
    let step = 2 * axis;
    let r = match unit {
        Units::Inches => return (0.0, 1.0),
        Units::Device => return (0.0, 1.0 / 72.0),
        Units::Lines => return (0.0, par.line()),
        Units::Chars => return (0.0, par.char_size(1)),
        Units::Ndc => g.device,
        Units::Nic => g.inner,
        Units::Nfc => g.figure,
        Units::Npc | Units::User => g.plot,
    };
    let span = r[step + 1] - r[step];
    if unit == Units::User {
        let a = par.usr()[step];
        let b = par.usr()[step + 1];
        let scale = if (b - a).is_finite() {
            span / (b - a)
        } else {
            (span / 2.0) / (b / 2.0 - a / 2.0)
        };
        (r[step] - a * scale, scale)
    } else {
        (r[step], span)
    }
}

pub(super) fn user_point(par: &GPar, g: Geometry, x: f64, y: f64) -> (f64, f64) {
    let convert = |v: f64, axis: usize| {
        let (base, scale) = transform(Units::User, g, par, axis);
        let v = if par.log[axis] { v.log10() } else { v };
        (base + v * scale) * 72.0
    };
    (convert(x, 0), convert(y, 1))
}

pub(super) fn convert(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = nums(ctx, args.get(0))?;
    let from = units(ctx, call, args.get(1), "from")?;
    let to = units(ctx, call, args.get(2), "to")?;
    let axis = usize::from(flag(ctx, args.get(3))? == Some(true));
    let slot = active(ctx, call, true)?;
    let initialized = slot.frame_open;
    let par = state(slot)?;
    let g = par.geometry();
    let (a, b) = transform(from, g, par, axis);
    let (c, d) = transform(to, g, par, axis);
    let log = par.log[axis];
    let result: Vec<_> = x
        .into_iter()
        .map(|x| {
            if !x.is_finite() {
                return NA_REAL;
            }
            if !initialized && (from == Units::User || to == Units::User) {
                return f64::NAN;
            }
            let x = if log && from == Units::User {
                x.log10()
            } else {
                x
            };
            let y = (a + b * x - c) / d;
            if log && to == Units::User {
                10.0_f64.powf(y)
            } else {
                y
            }
        })
        .collect();
    Ok(ctx.session_mut().alloc_real_from(&result)?)
}

pub(super) fn strmetric(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let labels = strings(ctx, args.get(0))?;
    let unit = string(ctx, args.get(1))?;
    let candidates = ["user", "inches", "figure"];
    let Some(unit) = candidates
        .into_iter()
        .find(|s| !unit.is_empty() && s.starts_with(&unit))
    else {
        return Err(ctx.stop(call.err_call(), "invalid units"));
    };
    let cex = nums(ctx, args.get(2))?.first().copied();
    let font = nums(ctx, args.get(3))?.first().copied();
    let height = flag(ctx, args.get(4))? == Some(true);
    active(ctx, call, true)?;
    if unit == "user" {
        require_frame(ctx, call)?;
    }
    let slot = active(ctx, call, false)?;
    let p = state(slot)?;
    let mut gc = super::lowlevel::context(p);
    gc.cex *= cex.unwrap_or(1.0);
    gc.fontface = font.unwrap_or(f64::from(gc.fontface)) as u8;
    let axis = usize::from(height);
    let g = p.geometry();
    let divisor = match unit {
        "figure" => size(g.figure)[axis] * 72.0,
        "user" => transform(Units::User, g, p, axis).1 * 72.0,
        _ => 72.0,
    };
    let result: Vec<_> = labels
        .iter()
        .map(|s| match s {
            None => 0.0,
            Some(s) if height => {
                let cap = slot.device.metric_info('M', &gc).0;
                (cap + (s.split('\n').count() - 1) as f64 * gc.ps * gc.cex * 1.2 * gc.lineheight)
                    / divisor
            }
            Some(s) => {
                s.split('\n')
                    .map(|s| slot.device.str_width(s, &gc))
                    .fold(0.0, f64::max)
                    / divisor
            }
        })
        .collect();
    Ok(ctx.session_mut().alloc_real_from(&result)?)
}

pub(super) fn ax_ticks(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let side = nums(ctx, args.get(0))?.first().copied().unwrap_or(0.0) as usize;
    if !(1..=4).contains(&side) {
        return Err(ctx.stop(call.err_call(), "invalid 'side' argument"));
    }
    let axis = (side - 1) % 2;
    let par = state(active(ctx, call, true)?)?;
    let (default_axp, default_usr, default_log, default_intervals) = (
        par.axp[axis],
        [par.usr()[2 * axis], par.usr()[2 * axis + 1]],
        par.log[axis],
        par.axis_count(axis),
    );
    let axp = if args.get(1) == SEXP_NIL {
        default_axp
    } else {
        nums(ctx, args.get(1))?
            .try_into()
            .map_err(|_| ctx.stop(call.err_call(), "invalid 'axp'"))?
    };
    let usr = if args.get(2) == SEXP_NIL {
        default_usr
    } else {
        limits(ctx, call, args.get(2), "usr")?
    };
    let log = flag(ctx, args.get(3))?.unwrap_or(default_log);
    let intervals = nums(ctx, args.get(4))?
        .first()
        .copied()
        .unwrap_or(default_intervals);
    let [a, b, n] = axp;
    let vector_limit = ctx.host().limits().max_vector_bytes / std::mem::size_of::<f64>();
    if !n.is_finite() || n.abs() >= vector_limit as f64 {
        return Err(ctx.stop(call.err_call(), "invalid 'axp[3]'"));
    }
    let mut values = if !log || n < 0.0 {
        let n = n.abs().round() as usize;
        (0..=n)
            .map(|i| {
                if i == 0 {
                    a
                } else if i == n {
                    b
                } else {
                    let f = i as f64 / n as f64;
                    let value = a * (1.0 - f) + b * f;
                    if value.abs() < (b - a).abs() * f64::EPSILON {
                        0.0
                    } else {
                        value
                    }
                }
            })
            .collect::<Vec<_>>()
    } else {
        let factors: &[f64] = match n as i32 {
            1 => &[1.0],
            2 => &[1.0, 5.0],
            3 => &[1.0, 2.0, 5.0],
            _ => return Err(ctx.stop(call.err_call(), "invalid positive 'axp[3]'")),
        };
        let lo = usr[0].min(usr[1]);
        let hi = usr[0].max(usr[1]);
        let anchor = a.min(b);
        if !anchor.is_finite() || anchor <= 0.0 {
            return Err(ctx.stop(call.err_call(), "invalid logarithmic axis limits"));
        }
        let anchor_exp = anchor.log10();
        let step = if n == 1.0 {
            ((b.max(a).log10() - anchor_exp) / intervals.max(1.0))
                .floor()
                .max(1.0) as i32
        } else {
            1
        };
        let mut values = Vec::new();
        for &factor in &factors[1..] {
            let minor = anchor * (factor / 10.0);
            if minor.log10() >= lo - 1e-12 && minor.log10() <= hi + 1e-12 {
                values.push(minor);
            }
        }
        let mut delta = 0_i32;
        while anchor_exp + f64::from(delta) <= hi + 1e-12
            && anchor_exp + f64::from(delta) <= f64::MAX.log10()
        {
            let decade = 10.0_f64.powf(anchor_exp + f64::from(delta));
            for &factor in factors {
                let tick = factor * decade;
                if tick.is_finite() && tick.log10() <= hi + 1e-12 {
                    values.push(tick);
                }
            }
            delta = delta.saturating_add(step);
        }
        values
    };
    if log && n > 0.0 && usr[0] > usr[1] {
        values.reverse();
    }
    Ok(ctx.session_mut().alloc_real_from(&values)?)
}

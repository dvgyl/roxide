use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::ValueApi;

use super::{flag, limit, number, numbers};

fn choose_unit(cell: f64, high_bias: f64, five_bias: f64) -> f64 {
    let base = 10.0_f64.powf(cell.log10().floor());
    let mut unit = base;
    for (candidate, bias) in [
        (2.0 * base, high_bias),
        (5.0 * base, five_bias),
        (10.0 * base, high_bias),
    ] {
        if !candidate.is_finite() || candidate - cell >= bias * (cell - unit) {
            break;
        }
        unit = candidate;
    }
    unit
}

pub(super) fn pretty(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = numbers(ctx, call, args.get(0))?;
    let Some((lo, hi)) = x
        .into_iter()
        .filter(|v| v.is_finite())
        .fold(None, |bounds, x| {
            let (lo, hi): (f64, f64) = bounds.unwrap_or((x, x));
            Some((lo.min(x), hi.max(x)))
        })
    else {
        return Ok(ctx.session_mut().alloc_real_from(&[])?);
    };
    let n = number(ctx, args.get(1))?.trunc();
    let min_n = number(ctx, args.get(2))?.trunc();
    let shrink = number(ctx, args.get(3))?;
    let high_bias = number(ctx, args.get(4))?;
    let five_bias = number(ctx, args.get(5))?;
    let eps_correct = number(ctx, args.get(6))?.trunc();
    let f_min = number(ctx, args.get(7))?;
    let bounds = flag(ctx, call, args.get(8), "invalid 'bounds' argument")?;
    for (valid, name) in [
        (n.is_finite() && n >= 0.0 && n <= f64::from(i32::MAX), "n"),
        ((0.0..=n).contains(&min_n), "min.n"),
        (shrink.is_finite() && shrink > 0.0, "shrink.sml"),
        (high_bias.is_finite() && high_bias >= 0.0, "high.u.bias"),
        (five_bias.is_finite() && five_bias >= 0.0, "u5.bias"),
        (f_min.is_finite() && f_min > 0.0, "f.min"),
    ] {
        if !valid {
            return Err(ctx.stop(call.err_call(), format!("invalid '{name}' argument")));
        }
    }

    if !(0.0..=2.0).contains(&eps_correct) {
        return Err(ctx.stop(call.err_call(), "'eps.correct' must be 0, 1, or 2"));
    }
    let span = hi - lo;
    let extent = lo.abs().max(hi.abs());
    let small_factor = 1.0 + (1.0 / (1.0 + high_bias)).max(1.5 / (1.0 + five_bias));
    let small = extent == 0.0 || span < extent * (3.0 * small_factor * n.max(1.0) * f64::EPSILON);
    let mut cell = if small {
        let extent = if extent == 0.0 {
            1.0
        } else if extent > 10.0 {
            9.0 + extent / 10.0
        } else {
            extent
        };
        extent * shrink / min_n.max(1.0)
    } else if span.is_finite() {
        span / n.max(1.0)
    } else {
        hi / n.max(1.0) - lo / n.max(1.0)
    };
    let min_cell = (f_min * f64::MIN_POSITIVE).max(f64::from_bits(1));
    let max_cell = f64::MAX / 1.25;
    if cell < min_cell {
        ctx.warn_now(
            call.err_call(),
            format!("R_pretty(): very small range 'cell'={cell:.4e}, increased to {min_cell:.4e}"),
        )?;
        cell = min_cell;
    } else if cell > max_cell {
        ctx.warn_now(
            call.err_call(),
            format!("R_pretty(): very large range 'cell'={cell:.4e}, decreased to {max_cell:.4e}"),
        )?;
        cell = max_cell;
    }
    let unit = choose_unit(cell, high_bias, five_bias);
    let tolerance = 1e-10;
    let mut lower = (lo / unit + tolerance).floor();
    let mut upper = (hi / unit - tolerance).ceil();
    let (mut corrected_lo, mut corrected_hi) = (lo, hi);
    if eps_correct == 2.0 || (eps_correct == 1.0 && !small) {
        corrected_lo = (lo - (lo.abs() * f64::EPSILON).max(f64::MIN_POSITIVE)).max(-f64::MAX);
        corrected_hi = (hi + (hi.abs() * f64::EPSILON).max(f64::MIN_POSITIVE)).min(f64::MAX);
        lower = lower.min((corrected_lo / unit + tolerance).floor());
        upper = upper.max((corrected_hi / unit - tolerance).ceil());
    }
    let edge = (f64::MAX / unit).floor();
    let edge = if (edge * unit).is_finite() {
        edge
    } else {
        edge - 1.0
    };
    lower = lower.max(-edge);
    upper = upper.min(edge);
    let extra = (min_n - (upper - lower)).max(0.0);
    let left = if lo == 0.0 && hi != 0.0 {
        0.0
    } else if hi == 0.0 && lo != 0.0 {
        extra
    } else if lower >= 0.0 {
        (extra / 2.0).ceil()
    } else {
        (extra / 2.0).floor()
    };
    lower -= left;
    upper += extra - left;

    let divisions = (upper - lower).max(0.0);
    if !divisions.is_finite() || divisions >= usize::MAX as f64 {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    let count = divisions as usize + 1;
    limit::<f64>(ctx, call, count)?;
    let grid_start = (lower * unit).max(-f64::MAX);
    let grid_end = (upper * unit).min(f64::MAX);
    let start = if bounds {
        grid_start.min(corrected_lo)
    } else {
        grid_start
    };
    let end = if bounds {
        grid_end.max(corrected_hi)
    } else {
        grid_end
    };
    let can_use_int = unit >= 1.0
        && unit.fract() == 0.0
        && start.abs() <= f64::from(i32::MAX)
        && end.abs() <= f64::from(i32::MAX)
        && start.fract() == 0.0
        && end.fract() == 0.0
        && (count == 1 || (start > 0.0 && end > 0.0) || (start < 0.0 && end < 0.0));
    if can_use_int {
        let output: Vec<_> = (0..count)
            .map(|i| ((lower + i as f64) * unit) as i32)
            .collect();
        return Ok(ctx.session_mut().alloc_int_from(&output)?);
    }
    let regular = start == lower * unit && end == upper * unit;
    let step = (end - start) / divisions.max(1.0);
    let values: Vec<_> = (0..count)
        .map(|i| {
            let value = if regular {
                (lower + i as f64) * unit
            } else if i == 0 {
                start
            } else if i == count - 1 {
                end
            } else if step.is_finite() {
                start + (i as f64) * step
            } else {
                let step = end / divisions - start / divisions;
                if i as f64 <= divisions / 2.0 {
                    start + i as f64 * step
                } else {
                    end - (divisions - i as f64) * step
                }
            };
            let scale = end / divisions.max(1.0) - start / divisions.max(1.0);
            if eps_correct == 0.0 && (value / scale).abs() < 1e-14 {
                0.0
            } else {
                value
            }
        })
        .collect();
    Ok(ctx.session_mut().alloc_real_from(&values)?)
}

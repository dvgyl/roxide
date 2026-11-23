use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::ValueApi;

use super::{flag, nums};

pub(super) fn counts(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let values = nums(ctx, args.get(0))?;
    let mut edges = nums(ctx, args.get(1))?;
    let right = flag(ctx, args.get(2))? == Some(true);
    let closed = flag(ctx, args.get(3))? == Some(true);
    let fuzz = nums(ctx, args.get(4))?;
    let Some(&fuzz) = fuzz.first().filter(|d| d.is_finite() && **d >= 0.0) else {
        return Err(ctx.stop(call.err_call(), "invalid 'fuzz'"));
    };
    let bins = edges.len().saturating_sub(1);
    if bins == 0 || edges.iter().any(|edge| !edge.is_finite()) {
        return Err(ctx.stop(call.err_call(), "invalid number of 'breaks'"));
    }
    let mut widths: Vec<_> = edges.windows(2).map(|p| p[1] - p[0]).collect();
    let scale = match bins {
        1 | 2 => {
            let (min, max) = values
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), &x| {
                    (min.min(x), max.max(x))
                });
            if values.is_empty() { 0.0 } else { max - min }
        }
        3 | 4 => widths
            .iter()
            .copied()
            .filter(|w| *w > 0.0)
            .fold(f64::INFINITY, f64::min),
        _ => {
            widths.sort_by(f64::total_cmp);
            widths[bins / 2] * 0.5 + widths[(bins - 1) / 2] * 0.5
        }
    };
    let d = if fuzz == 0.0 { 0.0 } else { fuzz * scale };
    for (i, edge) in edges.iter_mut().enumerate() {
        let raise = if i == 0 {
            right && !closed
        } else if i == bins {
            right || closed
        } else {
            right
        };
        *edge += if raise { d } else { -d };
    }
    let mut counts = vec![0_i32; bins];
    for x in values {
        if x < edges[0] || x > edges[bins] || (!closed && x == edges[bins]) {
            return Err(ctx.stop(
                call.err_call(),
                "some 'x' not counted; maybe 'breaks' do not span range of 'x'",
            ));
        }
        let i = edges[1..bins].partition_point(|edge| *edge < x || (!right && *edge == x));
        counts[i] = counts[i]
            .checked_add(1)
            .ok_or_else(|| ctx.stop(call.err_call(), "histogram count exceeds integer range"))?;
    }
    Ok(ctx.session_mut().alloc_int_from(&counts)?)
}

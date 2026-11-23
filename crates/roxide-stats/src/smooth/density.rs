use super::{list, reals};
use crate::distn::support::{limit, number, numbers};
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{Cplx, fft::fft};
use std::f64::consts::{PI, TAU};

fn transform(ctx: &mut Ctx, call: &Call, data: &mut [Cplx], inverse: bool) -> EvalResult<()> {
    fft(
        data,
        inverse,
        ctx.host().limits().max_vector_bytes / std::mem::size_of::<Cplx>(),
    )
    .map_err(|err| ctx.stop(call.err_call(), err.to_string()))
}

fn kernel(x: f64, bw: f64, kind: i32) -> f64 {
    let x = (x / bw).abs();
    if kind == 1 {
        return (-0.5 * x * x).exp() / (TAU.sqrt() * bw);
    }
    let radius = match kind {
        2 => 5.0_f64.sqrt(),
        3 => 3.0_f64.sqrt(),
        4 => 6.0_f64.sqrt(),
        5 => 7.0_f64.sqrt(),
        6 => (1.0 / 3.0 - 2.0 / PI.powi(2)).sqrt().recip(),
        _ => (1.0 - 8.0 / PI.powi(2)).sqrt().recip(),
    };
    let t = x / radius;
    if t >= 1.0 {
        return 0.0;
    }
    let value = match kind {
        2 => 0.75 * (1.0 - t * t),
        3 => 0.5,
        4 => 1.0 - t,
        5 => 0.9375 * (1.0 - t * t).powi(2),
        6 => (1.0 + (PI * t).cos()) / 2.0,
        _ => PI / 4.0 * (PI / 2.0 * t).cos(),
    };
    value / (radius * bw)
}

pub(super) fn estimate(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = numbers(ctx, call, args.get(0))?;
    let weights = numbers(ctx, call, args.get(1))?;
    let bw = number(ctx, call, args.get(2))?;
    let kind = number(ctx, call, args.get(3))? as i32;
    let n = number(ctx, call, args.get(4))? as usize;
    let range = numbers(ctx, call, args.get(5))?;
    let at = numbers(ctx, call, args.get(6))?;
    let old = number(ctx, call, args.get(7))? != 0.0;
    if n == 0 || x.len() != weights.len() || range.len() != 2 || !(bw > 0.0 && bw.is_finite()) {
        return Err(ctx.stop(call.err_call(), "invalid density data"));
    }
    let Some(n) = n.max(512).checked_next_power_of_two() else {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    };
    limit::<Cplx>(ctx, call, n.saturating_mul(2))?;
    let size = n * 2;
    let spacing = (range[1] - range[0]) / (n - 1) as f64;
    if !(spacing > 0.0 && spacing.is_finite()) {
        return Err(ctx.stop(call.err_call(), "invalid density grid"));
    }
    let mut bins = vec![Cplx { re: 0.0, im: 0.0 }; size];
    for (x, w) in x.into_iter().zip(weights) {
        let t = (x - range[0]) / spacing;
        if t.is_finite() && t > -1.0 && t < n as f64 {
            let i = t.floor() as isize;
            let f = t - i as f64;
            if i >= 0 {
                bins[i as usize].re += (1.0 - f) * w;
            }
            if i + 1 < n as isize {
                bins[(i + 1) as usize].re += f * w;
            }
        }
    }
    let kernel_spacing = if old {
        spacing * (size - 2) as f64 / (size - 1) as f64
    } else {
        spacing
    };
    let mut window: Vec<_> = (0..size)
        .map(|i| Cplx {
            re: kernel(i.min(size - i) as f64 * kernel_spacing, bw, kind),
            im: 0.0,
        })
        .collect();
    transform(ctx, call, &mut bins, false)?;
    transform(ctx, call, &mut window, false)?;
    for (a, b) in bins.iter_mut().zip(window) {
        *a = Cplx {
            re: a.re * b.re - a.im * b.im,
            im: a.re * b.im + a.im * b.re,
        };
    }
    transform(ctx, call, &mut bins, true)?;
    let result: Vec<_> = at
        .into_iter()
        .map(|x| {
            let t = (x - range[0]) / spacing;
            let i = (t.floor() as usize).min(n - 2);
            let f = t - i as f64;
            let left = bins[i].re.max(0.0);
            let right = bins[i + 1].re.max(0.0);
            (left + f * (right - left)) / size as f64
        })
        .collect();
    reals(ctx, &result)
}

pub(super) fn pair_counts(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = numbers(ctx, call, args.get(0))?;
    let nb = number(ctx, call, args.get(1))? as usize;
    if x.len() < 2 || nb == 0 || x.iter().any(|v| !v.is_finite()) {
        return Err(ctx.stop(call.err_call(), "invalid bandwidth data"));
    }
    limit::<f64>(ctx, call, nb)?;
    let min = x.iter().copied().fold(f64::INFINITY, f64::min);
    let max = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let width = (max - min) * 1.01 / nb as f64;
    let mut counts = vec![0.0; nb];
    if width == 0.0 {
        counts[0] = (x.len() as f64) * ((x.len() - 1) as f64) / 2.0;
    } else if x.len() <= nb / 2 && x.len() <= 2048 {
        let coords: Vec<_> = x.iter().map(|v| (v / width).trunc()).collect();
        for (i, a) in coords.iter().enumerate() {
            for b in &coords[..i] {
                let j = (a - b).abs() as usize;
                if let Some(value) = counts.get_mut(j) {
                    *value += 1.0;
                }
            }
        }
    } else {
        let size = nb
            .saturating_mul(2)
            .checked_next_power_of_two()
            .unwrap_or(usize::MAX);
        limit::<Cplx>(ctx, call, size)?;
        let mut bins = vec![Cplx { re: 0.0, im: 0.0 }; size];
        for x in &x {
            let i = ((x / width).trunc() - (min / width).trunc()) as usize;
            if let Some(bin) = bins.get_mut(i) {
                bin.re += 1.0;
            }
        }
        transform(ctx, call, &mut bins, false)?;
        for z in &mut bins {
            *z = Cplx {
                re: z.re * z.re + z.im * z.im,
                im: 0.0,
            };
        }
        transform(ctx, call, &mut bins, true)?;
        for (c, z) in counts.iter_mut().zip(bins) {
            *c = (z.re / size as f64).round();
        }
        counts[0] = (counts[0] - x.len() as f64) * 0.5;
    }
    let width = reals(ctx, &[width])?;
    let counts = reals(ctx, &counts)?;
    list(ctx, &[width, counts])
}

pub(super) fn bandwidth_score(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let h = number(ctx, call, args.get(0))?;
    let n = number(ctx, call, args.get(1))?;
    let width = number(ctx, call, args.get(2))?;
    let counts = numbers(ctx, call, args.get(3))?;
    let kind = number(ctx, call, args.get(4))? as i32;
    let mut sum = 0.0;
    for (i, count) in counts.into_iter().enumerate() {
        let d = (i as f64 * width / h).powi(2);
        if d >= 1000.0 {
            break;
        }
        let term = match kind {
            1 => (-d / 4.0).exp() - 8.0_f64.sqrt() * (-d / 2.0).exp(),
            2 => (-d / 4.0).exp() * (d * d - 12.0 * d + 12.0),
            3 => (-d / 2.0).exp() * (d * d - 6.0 * d + 3.0),
            _ => (-d / 2.0).exp() * (d.powi(3) - 15.0 * d * d + 45.0 * d - 15.0),
        };
        sum += count * term;
    }
    let score = match kind {
        1 => (0.5 + sum / n) / (n * h * PI.sqrt()),
        2 => (0.5 + sum / (64.0 * n)) / (n * h * PI.sqrt()),
        3 => (2.0 * sum + 3.0 * n) / (n * (n - 1.0) * TAU.sqrt() * h.powi(5)),
        _ => (2.0 * sum - 15.0 * n) / (n * (n - 1.0) * TAU.sqrt() * h.powi(7)),
    };
    reals(ctx, &[score])
}

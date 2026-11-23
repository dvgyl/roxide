use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{
    core::Flags,
    dist::normal::{pnorm, qnorm},
};
use roxide_value::ValueApi;

use super::{LOWER, UPPER};
use crate::distn::support::numbers;

fn polynomial(x: f64, coefficients: &[f64]) -> f64 {
    coefficients
        .iter()
        .rev()
        .fold(0.0, |value, &coefficient| coefficient + x * value)
}

pub(super) fn test(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut x = numbers(ctx, call, args.get(0))?;
    x.retain(|v| !v.is_nan());
    if !(3..=5000).contains(&x.len()) {
        return Err(ctx.stop(call.err_call(), "sample size must be between 3 and 5000"));
    }
    x.sort_by(f64::total_cmp);
    if x[0] == x[x.len() - 1] {
        return Err(ctx.stop(call.err_call(), "all 'x' values are identical"));
    }
    let answer = shapiro(&x);
    Ok(ctx.session_mut().alloc_real_from(&answer)?)
}

fn shapiro(x: &[f64]) -> [f64; 2] {
    let len = x.len();
    let n = len as f64;
    let mut flags = Flags::default();
    let mut weights: Vec<_> = (1..=len / 2)
        .map(|i| -qnorm((i as f64 - 0.375) / (n + 0.25), 0.0, 1.0, LOWER, &mut flags))
        .collect();
    if len == 3 {
        weights[0] = 0.5_f64.sqrt();
    } else {
        // Royston's normal-score coefficient approximations.
        let squares = 2.0 * weights.iter().map(|w| w * w).sum::<f64>();
        let r = n.sqrt().recip();
        let norm = squares.sqrt();
        let a = weights[0] / norm
            + polynomial(
                r,
                &[0.0, 0.221157, -0.147981, -2.07119, 4.434685, -2.706056],
            );
        let mut raw = squares - 2.0 * weights[0].powi(2);
        let mut target = 1.0 - 2.0 * a.powi(2);
        let count = if len > 5 {
            let b = weights[1] / norm
                + polynomial(
                    r,
                    &[0.0, 0.042981, -0.293762, -1.752461, 5.682633, -3.582633],
                );
            raw -= 2.0 * weights[1].powi(2);
            target -= 2.0 * b.powi(2);
            weights[1] = b;
            2
        } else {
            1
        };
        weights[0] = a;
        let scale = (target / raw).sqrt();
        for w in &mut weights[count..] {
            *w *= scale;
        }
    }
    let mut range = x[len - 1] - x[0];
    let scale = if range.is_infinite() && x[0].is_finite() && x[len - 1].is_finite() {
        range = x[len - 1] / 2.0 - x[0] / 2.0;
        0.5
    } else {
        1.0
    };
    // Divide by the range before centering to match the reference rounding.
    let normalized = |v: f64| v * scale / range;
    let average = x.iter().map(|&v| normalized(v)).sum::<f64>() / n;
    let cross: f64 = weights
        .iter()
        .enumerate()
        .map(|(i, w)| w * (normalized(x[len - i - 1]) - normalized(x[i])))
        .sum();
    let sum: f64 = x.iter().map(|&v| (normalized(v) - average).powi(2)).sum();
    let norm = sum.sqrt();
    let w1 = ((norm - cross) * (norm + cross) / sum).max(0.0);
    if !average.is_finite() {
        return [f64::NAN, f64::NAN];
    }
    let w = 1.0 - w1;
    let p = if len == 3 {
        (1.0 - 6.0 / std::f64::consts::PI * w.sqrt().acos()).clamp(0.0, 1.0)
    } else {
        let mut y = w1.ln();
        let (mean, sd) = if len <= 11 {
            let gamma = -2.273 + 0.459 * n;
            if y >= gamma {
                return [w, 1e-19];
            }
            y = -(gamma - y).ln();
            (
                polynomial(n, &[0.5440, -0.39978, 0.025054, -0.0006714]),
                polynomial(n, &[1.3822, -0.77857, 0.062767, -0.0020322]).exp(),
            )
        } else {
            let log_n = n.ln();
            (
                polynomial(log_n, &[-1.5861, -0.31082, -0.083751, 0.0038915]),
                polynomial(log_n, &[-0.4803, -0.082676, 0.0030302]).exp(),
            )
        };
        pnorm(y, mean, sd, UPPER, &mut flags)
    };
    [w, p]
}

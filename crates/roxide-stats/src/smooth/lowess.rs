use super::{list, median, reals};
use crate::distn::support::{number, numbers};
use roxide_eval::{Args, Call, Ctx, EvalResult};

fn local(x: &[f64], y: &[f64], robust: &[f64], at: usize, left: usize, right: usize) -> f64 {
    let q = x[at];
    let radius = (q - x[left]).max(x[right] - q);
    let mut sum = 0.0;
    let mut wy = 0.0;
    let mut wx = 0.0;
    let mut wxx = 0.0;
    let mut wxy = 0.0;
    for j in left..x.len() {
        let d = x[j] - q;
        if d > 0.999 * radius {
            break;
        }
        let w = if d.abs() <= 0.001 * radius {
            1.0
        } else if d.abs() <= 0.999 * radius {
            (1.0 - (d / radius).abs().powi(3)).powi(3)
        } else {
            0.0
        };
        let w = w * robust[j];
        sum += w;
        wy += w * y[j];
        wx += w * d;
        wxx += w * d * d;
        wxy += w * d * y[j];
    }
    if sum == 0.0 {
        return y[at];
    }
    let mean = wx / sum;
    let var = wxx / sum - mean * mean;
    let fit = wy / sum;
    if radius > 0.0 && var.sqrt() > 0.001 * (x[x.len() - 1] - x[0]) {
        fit - mean * (wxy / sum - mean * fit) / var
    } else {
        fit
    }
}

fn smooth(x: &[f64], y: &[f64], span: f64, iterations: usize, delta: f64) -> Vec<f64> {
    let n = x.len();
    if n < 2 {
        return y.to_vec();
    }
    let count = ((n as f64 * span + 1e-7) as usize).clamp(2, n);
    let mut result = vec![0.0; n];
    let mut robust = vec![1.0; n];
    let mut residual = vec![0.0; n];
    for iter in 0..=iterations {
        let mut left = 0;
        let mut right = count - 1;
        let mut i = 0;
        let mut last = 0;
        loop {
            while right + 1 < n && x[i] - x[left] > x[right + 1] - x[i] {
                left += 1;
                right += 1;
            }
            result[i] = local(x, y, &robust, i, left, right);
            if i > last + 1 {
                for j in last + 1..i {
                    let f = (x[j] - x[last]) / (x[i] - x[last]);
                    result[j] = f * result[i] + (1.0 - f) * result[last];
                }
            }
            last = i;
            let cutoff = x[last] + delta;
            i = last + 1;
            while i < n && x[i] <= cutoff {
                if x[i] == x[last] {
                    result[i] = result[last];
                    last = i;
                }
                i += 1;
            }
            if last == n - 1 {
                break;
            }
            i = (i - 1).max(last + 1);
        }
        if iter == iterations {
            break;
        }
        for ((r, y), fit) in residual.iter_mut().zip(y).zip(&result) {
            *r = (y - fit).abs();
        }
        let mean = residual.iter().sum::<f64>() / n as f64;
        let cmad = 6.0 * median(&mut residual);
        if cmad < 1e-7 * mean {
            break;
        }
        for ((w, y), fit) in robust.iter_mut().zip(y).zip(&result) {
            let r = (y - fit).abs();
            *w = if r <= 0.001 * cmad {
                1.0
            } else if r <= 0.999 * cmad {
                (1.0 - (r / cmad).powi(2)).powi(2)
            } else {
                0.0
            };
        }
    }
    result
}

pub(super) fn fit(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = numbers(ctx, call, args.get(0))?;
    let y = numbers(ctx, call, args.get(1))?;
    let span = number(ctx, call, args.get(2))?;
    let iter = number(ctx, call, args.get(3))?;
    let delta = number(ctx, call, args.get(4))?;
    if x.is_empty()
        || x.len() != y.len()
        || !(span.is_finite() && span > 0.0)
        || !iter.is_finite()
        || !(0.0..=1000.0).contains(&iter)
        || !(delta.is_finite() && delta >= 0.0)
    {
        return Err(ctx.stop(call.err_call(), "invalid lowess inputs"));
    }
    let result = smooth(&x, &y, span, iter as usize, delta);
    let x = reals(ctx, &x)?;
    let y = reals(ctx, &result)?;
    list(ctx, &[x, y])
}

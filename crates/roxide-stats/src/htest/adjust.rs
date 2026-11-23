use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::ValueApi;

use crate::distn::support::{limit, number, numbers};

pub(super) fn adjust(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut p = numbers(ctx, call, args.get(0))?;
    let method = number(ctx, call, args.get(1))? as usize;
    let n = number(ctx, call, args.get(2))?;
    if n.is_nan() || n < p.len() as f64 {
        return Err(ctx.stop(call.err_call(), "n >= lp is not TRUE"));
    }
    if n <= 1.0 || method == 8 {
        return Ok(ctx.session_mut().alloc_real_from(&p)?);
    }
    let mut order: Vec<_> = (0..p.len()).filter(|&i| !p[i].is_nan()).collect();
    order.sort_by(|&i, &j| p[i].total_cmp(&p[j]));
    let len = order.len();
    if len == 0 {
        return Ok(ctx.session_mut().alloc_real_from(&p)?);
    }
    let mut prior = f64::INFINITY;
    match method {
        1 => {
            prior = f64::NEG_INFINITY;
            for (i, &j) in order.iter().enumerate() {
                prior = prior.max((n - i as f64) * p[j]);
                p[j] = prior.min(1.0);
            }
        }
        3 if n > 2.0 => {
            if n > 10_000.0 {
                return Err(ctx.stop(call.err_call(), "Hommel adjustment size limit exceeded"));
            }
            let size = n as usize;
            limit::<f64>(ctx, call, size)?;
            let mut sorted: Vec<_> = order.iter().map(|&i| p[i]).collect();
            sorted.resize(size, 1.0);
            let simes = sorted
                .iter()
                .enumerate()
                .map(|(i, &v)| n * v / (i + 1) as f64)
                .fold(f64::INFINITY, f64::min);
            let mut adjusted = vec![simes; size];
            for m in (2..size).rev() {
                let prefix = size - m + 1;
                let tail = sorted[prefix..]
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| m as f64 * v / (i + 2) as f64)
                    .fold(f64::INFINITY, f64::min);
                let boundary = (m as f64 * sorted[prefix - 1]).min(tail);
                for (i, (out, &v)) in adjusted.iter_mut().zip(&sorted).enumerate() {
                    let candidate = if i < prefix {
                        (m as f64 * v).min(tail)
                    } else {
                        boundary
                    };
                    *out = out.max(candidate);
                }
            }
            for (i, j) in order.into_iter().enumerate() {
                p[j] = p[j].max(adjusted[i]);
            }
        }
        _ => {
            let harmonic = if method == 6 {
                let capped = n.min(100_000.0) as usize;
                let sum: f64 = (1..=capped).map(|i| 1.0 / i as f64).sum();
                if n == f64::INFINITY {
                    n
                } else if n > capped as f64 {
                    harmonic_delta(n.floor(), capped as f64) + sum
                } else {
                    sum
                }
            } else {
                1.0
            };
            for (i, &j) in order.iter().enumerate().rev() {
                let rank = i as f64 + 1.0;
                let factor = match method {
                    2 | 3 => n - i as f64,
                    4 => n,
                    _ => n / rank * harmonic,
                };
                let v = factor * p[j];
                if method == 4 {
                    p[j] = v.min(1.0);
                } else {
                    prior = prior.min(v);
                    p[j] = prior.min(1.0);
                }
            }
        }
    }
    Ok(ctx.session_mut().alloc_real_from(&p)?)
}

fn harmonic_delta(n: f64, m: f64) -> f64 {
    (n / m).ln() + (n.recip() - m.recip()) / 2.0 - (n.recip().powi(2) - m.recip().powi(2)) / 12.0
}

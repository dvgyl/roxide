use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{
    core::Flags,
    dist::{continuous::pt, normal::pnorm},
};
use roxide_value::{NA_REAL, ValueApi};

use super::{Alternative, LOWER, Ranks, UPPER, normal_tail, ranks};
use crate::distn::support::{limit, logical, numbers};

pub(in crate::htest) fn cor(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = numbers(ctx, call, args.get(0))?;
    let y = numbers(ctx, call, args.get(1))?;
    if x.len() != y.len() || x.len() < 2 {
        return Err(ctx.stop(call.err_call(), "not enough finite observations"));
    }
    let kendall = logical(ctx, call, args.get(2))? == Some(true);
    let alt = Alternative::read(ctx, call, args.get(3))?;
    let exact = logical(ctx, call, args.get(4))?.unwrap_or(!kendall || x.len() < 50);
    let correction = logical(ctx, call, args.get(5))? == Some(true);
    let rx = ranks(&x);
    let ry = ranks(&y);
    let ties = rx.ties.iter().chain(&ry.ties).any(|&n| n > 1);
    if exact && ties {
        ctx.warn_now(call.err_call(), "cannot compute exact p-value with ties")?;
    }
    let answer = if kendall {
        if x.len() > 10_000 {
            return Err(ctx.stop(call.err_call(), "Kendall test size limit exceeded"));
        }
        kendall_test(ctx, call, &x, &y, rx, ry, alt, exact && !ties, correction)?
    } else {
        spearman(&rx.values, &ry.values, alt, exact && !ties, correction)
    };
    Ok(ctx.session_mut().alloc_real_from(&answer)?)
}

#[allow(clippy::too_many_arguments)]
fn kendall_test(
    ctx: &mut Ctx,
    call: &Call,
    x: &[f64],
    y: &[f64],
    rx: Ranks,
    ry: Ranks,
    alt: Alternative,
    exact: bool,
    correct: bool,
) -> EvalResult<[f64; 4]> {
    let len = x.len();
    let n = len as f64;
    let mut s = 0_i64;
    for j in 1..len {
        for i in 0..j {
            if x[i] != x[j] && y[i] != y[j] {
                s += if (x[i] > x[j]) == (y[i] > y[j]) {
                    1
                } else {
                    -1
                };
            }
        }
    }
    let pairs = len * (len - 1) / 2;
    let tie_moments = |ties: &[usize]| {
        ties.iter().fold([0.0; 3], |[a, b, c], &t| {
            let t = t as f64;
            let v = t * (t - 1.0);
            [a + v, b + v * (t - 2.0), c + v * (2.0 * t + 5.0)]
        })
    };
    let [ax, bx, cx] = tie_moments(&rx.ties);
    let [ay, by, cy] = tie_moments(&ry.ties);
    let denom = n * (n - 1.0);
    let tau = 2.0 * s as f64 / ((denom - ax) * (denom - ay)).sqrt();
    if exact {
        let t = ((pairs as i64 + s) / 2) as usize;
        let tail = t.min(pairs - t);
        if (tail + 1)
            .saturating_mul(len)
            .saturating_mul(len.min(tail + 1))
            > 120_000_000
        {
            return Err(ctx.stop(call.err_call(), "exact Kendall test size limit exceeded"));
        }
        limit::<f64>(ctx, call, 2 * (tail + 1))?;
        let mut dp = vec![0.0; tail + 1];
        let mut next = vec![0.0; tail + 1];
        dp[0] = 1.0;
        for i in 2..=len {
            let max = tail.min(i * (i - 1) / 2);
            for (j, value) in next.iter_mut().enumerate().take(max + 1) {
                *value = dp[j.saturating_sub(i - 1)..=j].iter().sum::<f64>() / i as f64;
            }
            std::mem::swap(&mut dp, &mut next);
        }
        let small: f64 = dp.iter().sum();
        let large = 1.0 - (small - dp[tail]);
        let (lower, upper) = if t <= pairs / 2 {
            (small, large)
        } else {
            (large, small)
        };
        return Ok([t as f64, alt.probability(lower, upper), tau, 1.0]);
    }
    let var = (denom * (2.0 * n + 5.0) - cx - cy) / 18.0
        + ax * ay / (2.0 * denom)
        + if len > 2 {
            bx * by / (9.0 * denom * (n - 2.0))
        } else {
            0.0
        };
    let s = s as f64;
    let z = (s - if correct && s != 0.0 { s.signum() } else { 0.0 }) / var.sqrt();
    Ok([z, normal_tail(z, alt), tau, 0.0])
}

fn spearman(x: &[usize], y: &[usize], alt: Alternative, exact: bool, correct: bool) -> [f64; 4] {
    let n = x.len() as f64;
    let center = n + 1.0;
    let mut cross = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    for (&a, &b) in x.iter().zip(y) {
        let a = a as f64 - center;
        let b = b as f64 - center;
        cross += a * b;
        sx += a * a;
        sy += b * b;
    }
    if sx == 0.0 || sy == 0.0 {
        return [NA_REAL, NA_REAL, NA_REAL, 0.0];
    }
    let r = (cross / (sx.sqrt() * sy.sqrt())).clamp(-1.0, 1.0);
    let scale = n * (n * n - 1.0) / 6.0;
    let s = (1.0 - r) * scale;
    let p = if exact && x.len() <= 9 {
        let mut perm: Vec<_> = (0..x.len()).collect();
        let mut counts = [0; 3];
        permutation_counts(&mut perm, 0, s.round() as usize, &mut counts);
        let total = counts[2] as f64;
        alt.probability(counts[1] as f64 / total, counts[0] as f64 / total)
    } else if exact && x.len() <= 1290 {
        let lower = spearman_upper(n, s.round());
        let upper = 1.0 - spearman_upper(n, s.round() + 2.0);
        alt.probability(lower, upper)
    } else {
        let adjusted = 1.0 - s / (scale + f64::from(correct));
        let t = adjusted * ((n - 2.0) / (1.0 - adjusted * adjusted)).sqrt();
        let mut flags = Flags::default();
        alt.probability(
            pt(t, n - 2.0, LOWER, &mut flags),
            pt(t, n - 2.0, UPPER, &mut flags),
        )
    };
    [s, p, r, 0.0]
}

fn permutation_counts(p: &mut [usize], i: usize, observed: usize, counts: &mut [usize; 3]) {
    if i < p.len() {
        for j in i..p.len() {
            p.swap(i, j);
            permutation_counts(p, i + 1, observed, counts);
            p.swap(i, j);
        }
    } else {
        let s: usize = p
            .iter()
            .enumerate()
            .map(|(j, &v)| j.abs_diff(v).pow(2))
            .sum();
        counts[0] += usize::from(s <= observed);
        counts[1] += usize::from(s >= observed);
        counts[2] += 1;
    }
}

// Edgeworth approximation for Spearman's rank statistic (Best and Roberts, 1975).
fn spearman_upper(n: f64, s: f64) -> f64 {
    if s <= 0.0 {
        return 1.0;
    }
    if s > n * (n * n - 1.0) / 3.0 {
        return 0.0;
    }
    let b = 1.0 / n;
    let x = ((s - 1.0) / (n * (n * n - 1.0) / 6.0) - 1.0) * (n - 1.0).sqrt();
    let y = x * x;
    let u = x
        * b
        * (0.2274
            + b * (0.2531 + 0.1745 * b)
            + y * (-0.0758 + b * (0.1033 + 0.3932 * b)
                - y * b
                    * (0.0879 + 0.0151 * b
                        - y * (0.0072 - 0.0831 * b + y * b * (0.0131 - 0.00046 * y)))));
    (pnorm(x, 0.0, 1.0, UPPER, &mut Flags::default()) + u * (-y / 2.0).exp()).clamp(0.0, 1.0)
}

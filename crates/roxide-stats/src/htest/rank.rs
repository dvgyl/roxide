use std::convert::Infallible;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{
    core::Flags,
    dist::{
        continuous::pt,
        normal::{pnorm, qnorm},
    },
    roots::zeroin,
};
use roxide_value::{NA_REAL, SEXP_NIL, ValueApi};

use super::{Alternative, LOWER, UPPER};
use crate::distn::support::{limit, logical, number, numbers};

struct Ranks {
    values: Vec<usize>,
    ties: Vec<usize>,
}

fn ranks(values: &[f64]) -> Ranks {
    let mut order: Vec<_> = (0..values.len()).collect();
    order.sort_by(|&i, &j| values[i].total_cmp(&values[j]));
    let mut ranks = vec![0; values.len()];
    let mut ties = Vec::new();
    let mut first = 0;
    while first < order.len() {
        let mut end = first + 1;
        while end < order.len() && values[order[end]] == values[order[first]] {
            end += 1;
        }
        ties.push(end - first);
        for &i in &order[first..end] {
            ranks[i] = first + end + 1;
        }
        first = end;
    }
    Ranks {
        values: ranks,
        ties,
    }
}

fn rounded(x: f64, digits: f64) -> f64 {
    if x.is_finite() && x != 0.0 && digits < 16.0 {
        let power = digits.clamp(1.0, 15.0) - 1.0 - x.abs().log10().floor();
        let scale = 10.0_f64.powf(power);
        if scale.is_finite() && scale > 0.0 {
            return (x * scale).round_ties_even() / scale;
        }
    }
    x
}

struct RankTest {
    statistic: usize,
    weights: Vec<usize>,
    sample_size: Option<usize>,
    mean: f64,
    deviation: f64,
}

impl RankTest {
    fn at(x: &[f64], y: &[f64], shift: f64, digits: f64, exact: bool) -> Self {
        let m = x.len();
        let mut values: Vec<_> = x.iter().map(|&v| rounded(v - shift, digits)).collect();
        if y.is_empty() {
            if !exact {
                values.retain(|&v| v != 0.0);
            }
            let scores = ranks(&values.iter().map(|v| v.abs()).collect::<Vec<_>>());
            let statistic = values
                .iter()
                .zip(&scores.values)
                .filter(|(v, _)| **v > 0.0)
                .map(|(_, &r)| r)
                .sum();
            let weights: Vec<_> = values
                .iter()
                .zip(scores.values)
                .filter(|(v, _)| **v != 0.0)
                .map(|(_, r)| r)
                .collect();
            let mean = weights.iter().sum::<usize>() as f64 / 4.0;
            let deviation = weights
                .iter()
                .map(|&w| (w as f64).powi(2))
                .sum::<f64>()
                .sqrt()
                / 4.0;
            Self {
                statistic,
                weights,
                sample_size: None,
                mean,
                deviation,
            }
        } else {
            values.extend(y.iter().map(|&v| rounded(v, digits)));
            let scores = ranks(&values);
            let n = y.len();
            let tie_sum: f64 = scores
                .ties
                .iter()
                .map(|&t| (t as f64).powi(3) - t as f64)
                .sum();
            let total = (m + n) as f64;
            let product = m as f64 * n as f64;
            let deviation =
                (product / 12.0 * (total + 1.0 - tie_sum / (total * (total - 1.0)))).sqrt();
            let statistic = scores.values[..m].iter().sum::<usize>() - m * (m + 1);
            Self {
                statistic,
                weights: scores.values,
                sample_size: Some(m),
                mean: product / 2.0,
                deviation,
            }
        }
    }

    fn standard(&self, alt: Alternative, correct: bool) -> f64 {
        let d = self.statistic as f64 / 2.0 - self.mean;
        let adjustment = if correct {
            match alt {
                Alternative::Less => -0.5,
                Alternative::Greater => 0.5,
                Alternative::TwoSided => {
                    if d == 0.0 {
                        0.0
                    } else {
                        0.5 * d.signum()
                    }
                }
            }
        } else {
            0.0
        };
        (d - adjustment) / self.deviation
    }

    fn distribution(&self, ctx: &mut Ctx, call: &Call) -> EvalResult<(Vec<f64>, usize)> {
        if let Some(m) = self.sample_size {
            let size = self.weights.len();
            let count = m.min(size - m);
            let max = 2 * count * size;
            if (max + 1).saturating_mul(count + 1).saturating_mul(size) > 55_000_000 {
                return Err(ctx.stop(call.err_call(), "exact Wilcoxon test size limit exceeded"));
            }
            limit::<f64>(ctx, call, (max + 1) * (count + 1))?;
            let mut rows = vec![vec![0.0; max + 1]; count + 1];
            rows[0][0] = 1.0;
            for (i, &w) in self.weights.iter().enumerate() {
                for k in (1..=count.min(i + 1)).rev() {
                    let (old, new) = rows.split_at_mut(k);
                    for (dest, &source) in new[0][w..].iter_mut().zip(&old[k - 1]) {
                        *dest += source;
                    }
                }
            }
            let mut p = rows.pop().unwrap_or_default();
            let offset = count * (count + 1);
            let support = 2 * m * (size - m);
            p.drain(..offset);
            p.truncate(support + 1);
            if m != count {
                p.reverse();
            }
            Ok((p, self.statistic))
        } else {
            let sum: usize = self.weights.iter().sum();
            if sum.saturating_mul(self.weights.len()) > 25_000_000 {
                return Err(ctx.stop(call.err_call(), "exact Wilcoxon test size limit exceeded"));
            }
            limit::<f64>(ctx, call, sum + 1)?;
            let mut p = vec![0.0; sum + 1];
            p[0] = 1.0;
            let mut total = 0;
            for &weight in &self.weights {
                for i in (0..=total).rev() {
                    p[i + weight] += 0.5 * p[i];
                    p[i] *= 0.5;
                }
                total += weight;
            }
            Ok((p, self.statistic))
        }
    }
}

pub(super) fn wilcox(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut x = numbers(ctx, call, args.get(0))?;
    let mut y = if args.get(1) == SEXP_NIL {
        Vec::new()
    } else {
        numbers(ctx, call, args.get(1))?
    };
    x.retain(|v| !v.is_nan());
    y.retain(|v| !v.is_nan());
    if x.is_empty() || (args.get(1) != SEXP_NIL && y.is_empty()) {
        return Err(ctx.stop(call.err_call(), "not enough (non-missing) observations"));
    }
    if x.len() + y.len() > 1_000_000 {
        return Err(ctx.stop(call.err_call(), "Wilcoxon test size limit exceeded"));
    }
    let alt = Alternative::read(ctx, call, args.get(2))?;
    let mu = number(ctx, call, args.get(3))?;
    let exact = logical(ctx, call, args.get(4))?.unwrap_or(x.len() < 50 && y.len() < 50);
    if exact && x.len() + y.len() > 1000 {
        return Err(ctx.stop(call.err_call(), "exact Wilcoxon test size limit exceeded"));
    }
    let correction = number(ctx, call, args.get(5))?;
    let correct = correction != 0.0;
    let level = number(ctx, call, args.get(6))?;
    let tol = number(ctx, call, args.get(7))?;
    let digits = number(ctx, call, args.get(8))?;
    let ranks = RankTest::at(&x, &y, mu, digits, exact);
    let p = if exact {
        let (distribution, at) = ranks.distribution(ctx, call)?;
        exact_tail(&distribution, at, alt)
    } else {
        let z = ranks.standard(alt, correct);
        normal_tail(z, alt)
    };
    let mut out = [
        ranks.statistic as f64 / 2.0,
        p,
        f64::from(exact),
        NA_REAL,
        NA_REAL,
        NA_REAL,
        level,
    ];
    if !level.is_nan() {
        if !tol.is_finite() || tol <= 0.0 {
            return Err(ctx.stop(call.err_call(), "invalid 'tol.root'"));
        }
        let all_tied = !exact && x.windows(2).all(|w| w[0] == w[1]) && y.is_empty();
        let interval = if all_tied {
            ctx.warn_now(
                call.err_call(),
                "cannot compute confidence interval when all observations are zero or tied",
            )?;
            [f64::NAN, f64::NAN, x[0], 0.0]
        } else if exact {
            let mut scores = ranks.weights;
            scores.sort_unstable();
            let signed_conditioned =
                y.is_empty() && (scores.len() < x.len() || scores.windows(2).any(|w| w[0] == w[1]));
            exact_interval(ctx, call, &x, &y, alt, level, signed_conditioned)?
        } else {
            if x.len() + y.len() > 50_000 {
                return Err(ctx.stop(
                    call.err_call(),
                    "Wilcoxon confidence interval size limit exceeded",
                ));
            }
            let interval = approximate_interval(&x, &y, alt, level, correct, tol, digits);
            if interval[3] != level {
                ctx.warn_now(call.err_call(), "requested conf.level not achievable")?;
            }
            interval
        };
        out[3..].copy_from_slice(&interval);
    }
    Ok(ctx.session_mut().alloc_real_from(&out)?)
}

fn exact_tail(p: &[f64], at: usize, alt: Alternative) -> f64 {
    let total: f64 = p.iter().sum();
    let lower = p.iter().take(at + 1).sum::<f64>() / total;
    let upper = p.iter().skip(at).sum::<f64>() / total;
    alt.probability(lower, upper)
}

fn normal_tail(z: f64, alt: Alternative) -> f64 {
    if z.is_nan() {
        return z;
    }
    let mut flags = Flags::default();
    alt.probability(
        pnorm(z, 0.0, 1.0, LOWER, &mut flags),
        pnorm(z, 0.0, 1.0, UPPER, &mut flags),
    )
}

fn exact_interval(
    ctx: &mut Ctx,
    call: &Call,
    x: &[f64],
    y: &[f64],
    alt: Alternative,
    level: f64,
    signed_conditioned: bool,
) -> EvalResult<[f64; 4]> {
    let count = if y.is_empty() {
        x.len() * (x.len() + 1) / 2
    } else {
        x.len() * y.len()
    };
    if count > 2_000_000 {
        return Err(ctx.stop(
            call.err_call(),
            "Wilcoxon confidence interval size limit exceeded",
        ));
    }
    limit::<f64>(ctx, call, count)?;
    let mut shifts = Vec::with_capacity(count);
    if y.is_empty() {
        for (i, &a) in x.iter().enumerate() {
            shifts.extend(x[i..].iter().map(|&b| a / 2.0 + b / 2.0));
        }
    } else {
        for a in x {
            shifts.extend(y.iter().map(|b| a - b));
        }
    }
    shifts.sort_by(f64::total_cmp);
    let med = shifts[(count - 1) / 2] / 2.0 + shifts[count / 2] / 2.0;
    if signed_conditioned && shifts[0] == shifts[count - 1] {
        return Ok([
            if alt == Alternative::Less {
                f64::NEG_INFINITY
            } else {
                med
            },
            f64::INFINITY,
            med,
            1.0,
        ]);
    }
    // Invert the continuous-null reference distribution at integer order statistics.
    let size = x.len() + y.len();
    let reference: Vec<_> = (1..=size).map(|i| i as f64).collect();
    let test = RankTest::at(
        &reference[..x.len()],
        &reference[x.len()..],
        0.0,
        f64::INFINITY,
        true,
    );
    let (p, _) = test.distribution(ctx, call)?;
    let total: f64 = p.iter().sum();
    let a = alt.alpha(level);
    let mut used = 0.0;
    let mut k = 0;
    for (i, &mass) in p.iter().enumerate().step_by(2) {
        if used + mass / total > a + 1e-15 {
            break;
        }
        used += mass / total;
        k = i / 2 + 1;
    }
    k = k.min(count);
    let lower = if alt == Alternative::Less || (k == 0 && !signed_conditioned) {
        f64::NEG_INFINITY
    } else {
        shifts[k.saturating_sub(1)]
    };
    let upper = if alt == Alternative::Greater || k == 0 {
        f64::INFINITY
    } else {
        shifts[count - k]
    };
    Ok([
        lower,
        upper,
        med,
        1.0 - used
            * if alt == Alternative::TwoSided {
                2.0
            } else {
                1.0
            },
    ])
}

fn approximate_interval(
    x: &[f64],
    y: &[f64],
    alt: Alternative,
    level: f64,
    correct: bool,
    tol: f64,
    digits: f64,
) -> [f64; 4] {
    let range = |v: &[f64]| {
        v.iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &a| {
                (lo.min(a), hi.max(a))
            })
    };
    let (mut lo, mut hi) = range(x);
    if !y.is_empty() {
        let (a, b) = range(y);
        lo -= b;
        hi -= a;
    }
    if !lo.is_finite() || !hi.is_finite() {
        return [NA_REAL, NA_REAL, NA_REAL, level];
    }
    if lo == hi {
        return [lo, hi, lo, level];
    }
    let z = |shift, direction, correction| {
        RankTest::at(x, y, shift, digits, false).standard(direction, correction)
    };
    let solve = |direction, correction, target: f64| {
        let f = |shift| z(shift, direction, correction) - target;
        let a = f(lo);
        let b = f(hi);
        if a.is_nan() || b.is_nan() {
            return NA_REAL;
        }
        if a <= 0.0 {
            return lo;
        }
        if b >= 0.0 {
            return hi;
        }
        let result = zeroin(
            |shift| Ok::<_, Infallible>(f(shift)),
            lo,
            hi,
            a,
            b,
            tol,
            128,
        );
        match result {
            Ok(value) => value.root,
            Err(never) => match never {},
        }
    };
    let mut coverage = level;
    let bound = |coverage| qnorm(alt.alpha(coverage), 0.0, 1.0, UPPER, &mut Flags::default());
    let low_score = z(lo, Alternative::Greater, correct);
    let high_score = z(hi, Alternative::Less, correct);
    // Reduce coverage when the standardized statistic cannot reach a requested bound.
    while coverage > 0.0 {
        let target = bound(coverage);
        if (alt == Alternative::Less || target <= low_score)
            && (alt == Alternative::Greater || -target >= high_score)
        {
            break;
        }
        coverage = ((coverage * 10.0).ceil() - 1.0) / 10.0;
    }
    let target = bound(coverage);
    let lower = if alt == Alternative::Less {
        f64::NEG_INFINITY
    } else {
        solve(Alternative::Greater, correct, target)
    };
    let upper = if alt == Alternative::Greater {
        f64::INFINITY
    } else {
        solve(Alternative::Less, correct, -target)
    };
    [
        lower,
        upper,
        solve(Alternative::TwoSided, false, 0.0),
        coverage,
    ]
}

pub(super) fn cor(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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
        kendall_test(
            &x,
            &y,
            rx,
            ry,
            alt,
            exact && !ties && x.len() <= 100,
            correction,
        )
    } else {
        spearman(&rx.values, &ry.values, alt, exact && !ties, correction)
    };
    Ok(ctx.session_mut().alloc_real_from(&answer)?)
}

fn kendall_test(
    x: &[f64],
    y: &[f64],
    rx: Ranks,
    ry: Ranks,
    alt: Alternative,
    exact: bool,
    correct: bool,
) -> [f64; 4] {
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
        let mut dp = vec![0.0; pairs + 1];
        dp[0] = 1.0;
        let mut max = 0;
        for i in 2..=len {
            max += i - 1;
            let mut next = vec![0.0; pairs + 1];
            for (j, value) in next.iter_mut().enumerate().take(max + 1) {
                *value = dp[j.saturating_sub(i - 1)..=j].iter().sum::<f64>() / i as f64;
            }
            dp = next;
        }
        let t = ((pairs as i64 + s) / 2) as usize;
        return [t as f64, exact_tail(&dp, t, alt), tau, 1.0];
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
    [z, normal_tail(z, alt), tau, 0.0]
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
    let r = (cross / (sx * sy).sqrt()).clamp(-1.0, 1.0);
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
        let adjusted = (r - if correct { 1.0 / scale } else { 0.0 }).clamp(-1.0, 1.0);
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

use std::convert::Infallible;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{
    core::Flags,
    dist::normal::{dnorm, pnorm, qnorm},
    roots::zeroin,
};
use roxide_value::{NA_REAL, SEXP_NIL, ValueApi};

use super::{Alternative, LOWER, UPPER};
use crate::distn::support::{limit, logical, number, numbers};

mod correlation;

pub(super) use correlation::cor;

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

#[derive(Clone, Copy)]
enum RankTestKind {
    SignedRank,
    RankSum { first_size: usize },
}

struct RankTest {
    statistic: usize,
    weights: Vec<usize>,
    kind: RankTestKind,
    mean: f64,
    deviation: f64,
    tied: bool,
}

impl RankTest {
    fn at(x: &[f64], y: &[f64], shift: f64, digits: f64, exact: bool) -> Self {
        let m = x.len();
        let mut values: Vec<_> = x.iter().map(|&v| rounded(v - shift, digits)).collect();
        if y.is_empty() {
            let zeros = values.contains(&0.0);
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
                kind: RankTestKind::SignedRank,
                mean,
                deviation,
                tied: zeros || scores.ties.iter().any(|&size| size > 1),
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
                kind: RankTestKind::RankSum { first_size: m },
                mean: product / 2.0,
                deviation,
                tied: scores.ties.iter().any(|&size| size > 1),
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

    fn asymptotic_tail(&self, alt: Alternative, correction: Correction) -> f64 {
        let z = self.standard(alt, correction.enabled);
        if self.tied || correction.terms == 0 {
            return normal_tail(z, alt);
        }
        let cumulant = |power| match self.kind {
            RankTestKind::RankSum { first_size: m } => {
                let n = self.weights.len() - m;
                let coefficient = if power == 4 {
                    -1.0 / 120.0
                } else {
                    1.0 / 252.0
                };
                coefficient
                    * (1..=m)
                        .map(|i| {
                            (((n + i) as f64) / self.deviation).powi(power)
                                - ((i as f64) / self.deviation).powi(power)
                        })
                        .sum::<f64>()
            }
            RankTestKind::SignedRank => {
                let coefficient = if power == 4 { -2.0 } else { 16.0 };
                coefficient
                    * self
                        .weights
                        .iter()
                        .map(|&w| (w as f64 / (4.0 * self.deviation)).powi(power))
                        .sum::<f64>()
            }
        };
        let fourth = cumulant(4);
        let mut hermite = [1.0, z, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        for i in 2..hermite.len() {
            hermite[i] = z * hermite[i - 1] - (i - 1) as f64 * hermite[i - 2];
        }
        let mut series = fourth * hermite[3] / 24.0;
        if correction.terms >= 2 {
            series += cumulant(6) * hermite[5] / 720.0;
        }
        if correction.terms >= 3 {
            series += fourth.powi(2) * hermite[7] / 1152.0;
        }
        let mut flags = Flags::default();
        let change = dnorm(z, 0.0, 1.0, false, &mut flags) * series;
        alt.probability(
            (pnorm(z, 0.0, 1.0, LOWER, &mut flags) - change).clamp(0.0, 1.0),
            (pnorm(z, 0.0, 1.0, UPPER, &mut flags) + change).clamp(0.0, 1.0),
        )
    }

    fn distribution(&self, ctx: &mut Ctx, call: &Call) -> EvalResult<(Vec<f64>, usize)> {
        match self.kind {
            RankTestKind::RankSum { first_size: m } => {
                let size = self.weights.len();
                let count = m.min(size - m);
                let max = 2 * count * size;
                if (max + 1).saturating_mul(count + 1).saturating_mul(size) > 55_000_000 {
                    return Err(
                        ctx.stop(call.err_call(), "exact Wilcoxon test size limit exceeded")
                    );
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
            }
            RankTestKind::SignedRank => {
                let sum: usize = self.weights.iter().sum();
                if sum.saturating_mul(self.weights.len()) > 25_000_000 {
                    return Err(
                        ctx.stop(call.err_call(), "exact Wilcoxon test size limit exceeded")
                    );
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
}

#[derive(Clone, Copy)]
struct Correction {
    enabled: bool,
    terms: u8,
}

impl Correction {
    fn read(ctx: &mut Ctx, call: &Call, args: &Args) -> EvalResult<Self> {
        let id = args.get(5);
        if matches!(
            ctx.session().type_of(id)?,
            roxide_value::SexpType::Int | roxide_value::SexpType::Real
        ) {
            if ctx.session().xlength(id)? > 1 {
                return Err(ctx.stop(call.err_call(), "the condition has length > 1"));
            }
            let n = number(ctx, call, id)?;
            if !(0.0..=3.0).contains(&n) || n.fract() != 0.0 {
                return Err(ctx.stop(
                    call.err_call(),
                    "'correct' must be an integer between 0 and 3",
                ));
            }
            return Ok(Self {
                enabled: true,
                terms: n as u8,
            });
        }
        Ok(Self {
            enabled: ctx.session().type_of(id)? == roxide_value::SexpType::Lgl
                && ctx.session().xlength(id)? == 1
                && logical(ctx, call, id)? == Some(true),
            terms: 0,
        })
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
    let correction = Correction::read(ctx, call, args)?;
    let interval_requested = args.get(6) != SEXP_NIL;
    let level = number(ctx, call, args.get(6))?;
    if interval_requested
        && (ctx.session().xlength(args.get(6))? != 1 || !(level > 0.0 && level < 1.0))
    {
        return Err(ctx.stop(
            call.err_call(),
            "'conf.level' must be a single number between 0 and 1",
        ));
    }
    let tol = number(ctx, call, args.get(7))?;
    let digits = number(ctx, call, args.get(8))?;
    let ranks = RankTest::at(&x, &y, mu, digits, exact);
    let p = if exact {
        let (distribution, at) = ranks.distribution(ctx, call)?;
        exact_tail(&distribution, at, alt)
    } else {
        ranks.asymptotic_tail(alt, correction)
    };
    let mut out = [
        ranks.statistic as f64 / 2.0,
        p,
        f64::from(exact),
        NA_REAL,
        NA_REAL,
        NA_REAL,
        level,
        f64::from(correction.enabled),
    ];
    if interval_requested {
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
            let conditioned = y.is_empty() && ranks.tied;
            exact_interval(ctx, call, &x, &y, alt, level, conditioned)?
        } else {
            if x.len() + y.len() > 50_000 {
                return Err(ctx.stop(
                    call.err_call(),
                    "Wilcoxon confidence interval size limit exceeded",
                ));
            }
            let interval =
                approximate_interval(&x, &y, alt, level, correction.enabled, tol, digits);
            if interval[3] != level {
                ctx.warn_now(call.err_call(), "requested conf.level not achievable")?;
            }
            interval
        };
        out[3..7].copy_from_slice(&interval);
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
    if signed_conditioned {
        let a = alt.alpha(level);
        shifts.dedup();
        let (lower, left) = if alt == Alternative::Less {
            (f64::NEG_INFINITY, 0.0)
        } else {
            signed_bound(ctx, call, x, &shifts, a, true)?
        };
        let (upper, right) = if alt == Alternative::Greater {
            (f64::INFINITY, 0.0)
        } else {
            signed_bound(ctx, call, x, &shifts, a, false)?
        };
        return Ok([lower, upper, med, 1.0 - left - right]);
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
    let lower = if alt == Alternative::Less || k == 0 {
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

fn signed_bound(
    ctx: &mut Ctx,
    call: &Call,
    x: &[f64],
    shifts: &[f64],
    alpha: f64,
    lower: bool,
) -> EvalResult<(f64, f64)> {
    let count = shifts.len();
    let p_min = 0.5_f64.powi(x.len() as i32);
    if p_min > alpha {
        return Ok((if lower { shifts[0] } else { f64::INFINITY }, 0.0));
    }
    // Test both the Walsh averages and the open intervals between them.
    let probe = |ctx: &mut Ctx, i: usize| -> EvalResult<(f64, f64)> {
        let at = if lower { i } else { 2 * (count - 1) - i };
        let mut shift = shifts[at / 2];
        if at % 2 != 0 {
            shift = shift / 2.0 + shifts[at / 2 + 1] / 2.0;
        }
        let rank = RankTest::at(x, &[], shift, f64::INFINITY, true);
        let (p, at) = rank.distribution(ctx, call)?;
        let index = if lower { p.len() - 1 - at } else { at };
        let mut cdf = 0.0;
        let mut rejected = 0.0;
        let tail: f64 = p[..=index].iter().sum();
        for mass in p {
            cdf += mass;
            if cdf > alpha + 1e-15 {
                break;
            }
            rejected = cdf;
        }
        Ok((tail, rejected))
    };
    let mut first = 0;
    let mut end = 2 * count - 1;
    while first < end {
        let mid = first + (end - first) / 2;
        if probe(ctx, mid)?.0 > alpha + 1e-15 {
            end = mid;
        } else {
            first = mid + 1;
        }
    }
    if first == 2 * count - 1 {
        return Ok((
            if lower {
                shifts[count - 1]
            } else {
                f64::INFINITY
            },
            0.0,
        ));
    }
    if lower {
        // At a unique minimum, the zero rank is absent at the boundary.
        // Use the first interval to include the rank as it changes sign.
        if first == 0 && count > 1 && x.iter().filter(|&&v| v == shifts[0]).count() == 1 {
            first = 1;
        }
        let rejected = probe(ctx, first)?.1;
        Ok((shifts[first.div_ceil(2)], rejected))
    } else {
        let index = count - 1 - first / 2;
        // Use the endpoint tail if it rejects and all signed ranks are present.
        let zeros = x.iter().filter(|&&v| v == shifts[index]).count();
        let endpoint_tail = probe(ctx, 2 * (count - 1 - index))?.0;
        let rejected = if zeros == 0 && endpoint_tail <= alpha + 1e-15 {
            endpoint_tail
        } else if index + 1 < count {
            probe(ctx, 2 * (count - index - 1) - 1)?.1
        } else if zeros == 1 {
            probe(ctx, 0)?.1
        } else {
            p_min
        };
        Ok((shifts[index], rejected))
    }
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
    for _ in 0..=10 {
        if coverage <= 0.0 {
            break;
        }
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

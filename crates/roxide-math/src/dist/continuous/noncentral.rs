//! Noncentral chi-square, beta, F, and Student's t distributions.

use crate::dist::normal::{pnorm, qnorm};
use crate::rng::Rng;
use crate::special::aux::{bd0, stirlerr};

use super::beta::beta_factor;
use super::gamma::{dgamma, pgamma, rgamma};
use super::ratios::{f_coordinates, rchisq};
use super::*;

mod student_t;

pub use student_t::{dnt, pnt, qnt, rnt};

const INF: f64 = f64::INFINITY;
const MAX_QUANTILE_COMPONENTS: usize = 500_000;
const LARGE_NONCENTRALITY: f64 = 100_000.0;

// Use the signed likelihood root of the chi-square cumulant generating function.
// Near the mean its correction has a removable singularity. Use the
// skewness and kurtosis corrections to the normal probability there.
fn large_chisq_normal_score(x: f64, df: f64, ncp: f64) -> f64 {
    let scale = df.max(ncp);
    let (d, n) = (df / scale, ncp / scale);
    let y = x / scale;
    let variance = 2.0 * (d + 2.0 * n);
    let difference = ((x - ncp) - df) / scale;
    let z = difference * scale.sqrt() / variance.sqrt();
    if z.abs() <= 0.5 {
        let skewness = 8.0 * (d + 3.0 * n) / (variance * variance.sqrt() * scale.sqrt());
        let kurtosis = (48.0 * (d + 4.0 * n) / (variance * variance)) / scale;
        let squared = z * z;
        let first = skewness * (1.0 - squared) / 6.0;
        let second = -kurtosis * z * (squared - 3.0) / 24.0
            - skewness * skewness * z * (squared * (squared - 10.0) + 15.0) / 72.0;
        return z + first + second + 0.5 * z * first * first;
    }

    // At the saddlepoint, x = df * t + ncp * t².
    let t = 2.0 * (y.sqrt() / (d / y.sqrt() + (d / y.sqrt()).hypot(2.0 * n.sqrt())));
    if t <= 0.0 || !t.is_finite() {
        return z;
    }
    let delta = difference / (d + n * (t + 1.0));
    let log_remainder = if delta.abs() < 1e-3 {
        0.5 + delta * (-1.0 / 3.0 + delta * (0.25 - delta / 5.0))
    } else {
        (delta - t.ln()) / (delta * delta)
    };
    let root_scale = (d * log_remainder + n).sqrt();
    let root = delta * scale.sqrt() * root_scale;
    let ratio = (0.5 * d + n * t).sqrt() / root_scale;
    if root.is_finite() && ratio > 0.0 && ratio.is_finite() {
        root + ratio.ln() / root
    } else {
        root
    }
}

fn poisson_log_mass(k: f64, mean: f64) -> f64 {
    if k == 0.0 {
        -mean
    } else {
        -bd0(k, mean) - stirlerr(k) - 0.5 * k.ln() - LOG_SQRT_2PI
    }
}

fn poisson_mixture(
    mean: f64,
    mut component: impl FnMut(f64, &mut Flags) -> f64,
    flags: &mut Flags,
    budget: &mut usize,
) -> f64 {
    if *budget == 0 {
        flags.set(Flags::NO_CONVERGENCE);
        return f64::NAN;
    }
    *budget -= 1;
    if mean == 0.0 {
        return component(0.0, flags);
    }
    // Above this limit consecutive integers no longer fit in a double.
    if mean >= 4_503_599_627_370_496.0 {
        flags.set(Flags::PRECISION);
        return component(mean, flags);
    }
    let mode = mean.floor();
    let modal_mass = poisson_log_mass(mode, mean);
    let mut sum = modal_mass + component(mode, flags);
    for direction in [-1.0, 1.0] {
        let mut k = mode;
        let mut log_weight = modal_mass;
        let mut previous = INF;
        let mut converged = false;
        for _ in 0..100_000 {
            let next = k + direction;
            if next < 0.0 {
                converged = true;
                break;
            }
            log_weight += if direction < 0.0 {
                (k / mean).ln()
            } else {
                (mean / next).ln()
            };
            if *budget == 0 {
                flags.set(Flags::NO_CONVERGENCE);
                return f64::NAN;
            }
            *budget -= 1;
            let term = log_weight + component(next, flags);
            // Component terms decrease at least geometrically beyond this point.
            let ratio = (term - previous).exp();
            sum = log_add(sum, term);
            if ratio < 1.0 && term - (-ratio).ln_1p() < sum - 38.0 {
                converged = true;
                break;
            }
            previous = term;
            k = next;
        }
        if !converged {
            flags.set(Flags::PRECISION);
        }
    }
    sum
}

fn positive_quantile(
    target: f64,
    lower: bool,
    seed: f64,
    mut evaluate: impl FnMut(f64) -> (f64, f64),
) -> f64 {
    let first = seed.ln().clamp(-745.0, 709.0);
    let mut low = first;
    let mut high = first;
    let rising = (evaluate(seed).0 < target) == lower;
    let mut stride = 1.0;
    for _ in 0..12 {
        if rising {
            high = (first + stride).min(710.0);
            if (evaluate(high.exp()).0 >= target) == lower || high == 710.0 {
                break;
            }
        } else {
            low = (first - stride).max(-746.0);
            if (evaluate(low.exp()).0 <= target) == lower || low == -746.0 {
                break;
            }
        }
        stride *= 2.0;
    }
    let z = invert(low, high, low.midpoint(high), target, lower, |z| {
        let (lp, ld) = evaluate(z.exp());
        let slope = (ld + z - lp).exp();
        (lp, if lower { slope } else { -slope })
    });
    z.exp()
}

/// Evaluate the noncentral chi-square density.
pub fn dnchisq(x: f64, df: f64, ncp: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || df.is_nan() || ncp.is_nan() {
        return x + df + ncp;
    }
    if df < 0.0 || ncp < 0.0 || !df.is_finite() || !ncp.is_finite() {
        return invalid(flags);
    }
    if x < 0.0 || x.is_infinite() {
        return zero(log);
    }
    if x == 0.0 {
        if df < 2.0 {
            return INF;
        }
        if df == 2.0 {
            return on_scale(-0.5 * ncp - std::f64::consts::LN_2, log);
        }
        return zero(log);
    }
    let mut budget = usize::MAX;
    let value = poisson_mixture(
        0.5 * ncp,
        |k, flags| dgamma(x, 0.5 * df + k, 2.0, true, flags),
        flags,
        &mut budget,
    );
    on_scale(value, log)
}

// R truncates the low-noncentrality sum by the total Poisson mass.
fn poisson_small(
    mean: f64,
    mut component: impl FnMut(f64, &mut Flags) -> f64,
    flags: &mut Flags,
) -> f64 {
    let mut mass = (-mean).exp();
    let mut weight = mass;
    let mut sum = -mean + component(0.0, flags);
    for k in 1..=256 {
        let k = f64::from(k);
        weight *= mean / k;
        if weight == 0.0 {
            break;
        }
        mass += weight;
        sum = log_add(sum, weight.ln() + component(k, flags));
        if mass >= 1.0 - 1e-15 {
            break;
        }
    }
    sum - mass.ln()
}

/// Evaluate a noncentral chi-square tail probability.
pub fn pnchisq(x: f64, df: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    let mut budget = usize::MAX;
    pnchisq_budget(x, df, ncp, tail, flags, &mut budget)
}

fn pnchisq_budget(
    x: f64,
    df: f64,
    ncp: f64,
    tail: Tail,
    flags: &mut Flags,
    budget: &mut usize,
) -> f64 {
    if x.is_nan() || df.is_nan() || ncp.is_nan() {
        return x + df + ncp;
    }
    if df < 0.0 || ncp < 0.0 || !df.is_finite() || !ncp.is_finite() {
        return invalid(flags);
    }
    if x < 0.0 || x.is_infinite() {
        return step(x > 0.0, tail);
    }
    if x == 0.0 {
        return if df == 0.0 {
            from_log(-0.5 * ncp, true, tail)
        } else {
            step(false, tail)
        };
    }
    if ncp >= LARGE_NONCENTRALITY {
        return pnorm(large_chisq_normal_score(x, df, ncp), 0.0, 1.0, tail, flags);
    }
    let log_tail = Tail { log: true, ..tail };
    let mean = 0.5 * ncp;
    let component = |k, flags: &mut Flags| pgamma(x, 0.5 * df + k, 2.0, log_tail, flags);
    let value = if ncp < 80.0 {
        poisson_small(mean, component, flags)
    } else {
        poisson_mixture(mean, component, flags, budget)
    };
    on_scale(if value > 0.0 { 0.0 } else { value }, tail.log)
}

/// Evaluate a noncentral chi-square quantile.
pub fn qnchisq(p: f64, df: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || df.is_nan() || ncp.is_nan() {
        return p + df + ncp;
    }
    if df < 0.0 || ncp < 0.0 || !df.is_finite() || !ncp.is_finite() || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, 0.0, INF) {
        return x;
    }
    if ncp == 0.0 {
        if df == 0.0 {
            return invalid(flags);
        }
        return super::qchisq(p, df, tail, flags);
    }
    let (lp, lq) = probability_logs(p, tail);
    if df == 0.0 && lp <= -0.5 * ncp {
        return 0.0;
    }
    let lower = lp <= lq;
    let log_tail = Tail { lower, log: true };
    if ncp >= LARGE_NONCENTRALITY {
        let target = qnorm(lp.min(lq), 0.0, 1.0, log_tail, flags);
        return positive_quantile(target, true, (df + ncp).min(f64::MAX), |x| {
            let score = if x == 0.0 {
                f64::NEG_INFINITY
            } else if x.is_infinite() {
                f64::INFINITY
            } else {
                large_chisq_normal_score(x, df, ncp)
            };
            (score, f64::NAN)
        });
    }
    let mut budget = MAX_QUANTILE_COMPONENTS;
    let value = positive_quantile(lp.min(lq), lower, (df + ncp).min(f64::MAX), |x| {
        (
            pnchisq_budget(x, df, ncp, log_tail, flags, &mut budget),
            if ncp < 80.0 && x.is_finite() && x > 0.0 {
                poisson_small(
                    0.5 * ncp,
                    |k, flags| dgamma(x, 0.5 * df + k, 2.0, true, flags),
                    flags,
                )
            } else if x.is_finite() && x > 0.0 {
                poisson_mixture(
                    0.5 * ncp,
                    |k, flags| dgamma(x, 0.5 * df + k, 2.0, true, flags),
                    flags,
                    &mut budget,
                )
            } else {
                dnchisq(x, df, ncp, true, flags)
            },
        )
    });
    if budget == 0 { invalid(flags) } else { value }
}

/// Draw from a noncentral chi-square distribution.
pub fn rnchisq(rng: &mut Rng, df: f64, ncp: f64, flags: &mut Flags) -> f64 {
    if df < 0.0 || df.is_nan() || ncp < 0.0 || !ncp.is_finite() {
        return invalid(flags);
    }
    if ncp == 0.0 {
        return rgamma(rng, 0.5 * df, 2.0, flags);
    }
    let k = crate::dist::discrete::rpois(rng, 0.5 * ncp, flags);
    let mut value = if k > 0.0 {
        rgamma(rng, k, 2.0, flags)
    } else {
        k
    };
    if df > 0.0 {
        value += rgamma(rng, 0.5 * df, 2.0, flags);
    }
    value
}

fn beta_parameters(a: f64, b: f64, ncp: f64) -> bool {
    a > 0.0 && b > 0.0 && ncp >= 0.0 && a.is_finite() && b.is_finite() && ncp.is_finite()
}

fn n_beta_density(
    x: f64,
    y: f64,
    a: f64,
    b: f64,
    ncp: f64,
    flags: &mut Flags,
    budget: &mut usize,
) -> f64 {
    poisson_mixture(
        0.5 * ncp,
        |k, flags| beta_factor(x, y, a + k, b, flags),
        flags,
        budget,
    )
}

// The next beta CDF bounds every component that follows it in the series.
fn truncated_beta_lower(
    x: f64,
    y: f64,
    a: f64,
    b: f64,
    mean: f64,
    flags: &mut Flags,
    budget: &mut usize,
) -> f64 {
    let start = (mean - 7.0 * mean.sqrt()).floor().max(0.0);
    let mut weight = poisson_log_mass(start, mean).exp();
    let mut remainder = 1.0 - weight;
    let shape = a + start;
    let mut value = super::pbeta_raw(x, y, shape, b, Tail::default(), flags);
    let mut decrease = (beta_factor(x, y, shape, b, flags) - shape.ln()).exp();
    let mut sum = weight * value;
    for i in 1..=100_000 {
        if *budget == 0 {
            flags.set(Flags::NO_CONVERGENCE);
            return f64::NAN;
        }
        *budget -= 1;
        let j = f64::from(i);
        let next = (value - decrease).max(0.0);
        if next * remainder <= 1e-9 {
            break;
        }
        value = next;
        decrease *= x + (x / (shape + j)) * (b - 1.0);
        weight *= mean / (start + j);
        remainder -= weight;
        sum = weight.mul_add(value, sum);
    }
    sum.min(1.0)
}

fn n_beta_tail(
    (x, y): (f64, f64),
    a: f64,
    b: f64,
    ncp: f64,
    tail: Tail,
    flags: &mut Flags,
    budget: &mut usize,
) -> f64 {
    if ncp < 20_000.0 {
        let lower = truncated_beta_lower(x, y, a, b, 0.5 * ncp, flags, budget);
        if lower > 0.0 {
            if !tail.lower && lower > 1.0 - 1e-10 {
                flags.set(Flags::PRECISION);
            }
            return match (tail.lower, tail.log) {
                (true, false) => lower,
                (true, true) => lower.ln(),
                (false, false) => 1.0 - lower,
                (false, true) => (-lower).ln_1p(),
            };
        }
    }
    let log_tail = Tail { log: true, ..tail };
    let lp = poisson_mixture(
        0.5 * ncp,
        |k, flags| super::pbeta_raw(x, y, a + k, b, log_tail, flags),
        flags,
        budget,
    );
    on_scale(if lp > 0.0 { 0.0 } else { lp }, tail.log)
}

/// Evaluate a noncentral beta density.
pub fn dnbeta(x: f64, a: f64, b: f64, ncp: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || a.is_nan() || b.is_nan() || ncp.is_nan() {
        return x + a + b + ncp;
    }
    if !beta_parameters(a, b, ncp) {
        return invalid(flags);
    }
    if !(0.0..=1.0).contains(&x) {
        return zero(log);
    }
    if x == 0.0 {
        return on_scale(super::dbeta(x, a, b, true, flags) - 0.5 * ncp, log);
    }
    if x == 1.0 {
        if b == 1.0 {
            let value = a + 0.5 * ncp;
            return if log { value.ln() } else { value };
        }
        return super::dbeta(x, a, b, log, flags);
    }
    let y = complement(x);
    let mut budget = usize::MAX;
    on_scale(
        n_beta_density(x, y, a, b, ncp, flags, &mut budget) - x.ln() - y.ln(),
        log,
    )
}

/// Evaluate a noncentral beta tail probability.
pub fn pnbeta(x: f64, a: f64, b: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || a.is_nan() || b.is_nan() || ncp.is_nan() {
        return x + a + b + ncp;
    }
    if a < 0.0 || b <= 0.0 || ncp < 0.0 || !ncp.is_finite() {
        return invalid(flags);
    }
    if x <= 0.0 || x >= 1.0 {
        return step(x >= 1.0, tail);
    }
    if !beta_parameters(a, b, ncp) {
        return invalid(flags);
    }
    let mut budget = usize::MAX;
    n_beta_tail((x, complement(x)), a, b, ncp, tail, flags, &mut budget)
}

fn n_beta_odds(lp: f64, lq: f64, a: f64, b: f64, ncp: f64, flags: &mut Flags) -> f64 {
    let lower = lp <= lq;
    let log_tail = Tail { lower, log: true };
    let target = lp.min(lq);
    let use_mixture = !lower && target < -23.0;
    let seed = super::beta::beta_quantile_odds(lp, lq, a + 0.5 * ncp, b, flags);
    let mut budget = MAX_QUANTILE_COMPONENTS;
    let value = invert(
        -746.0,
        746.0,
        seed.clamp(-745.0, 745.0),
        target,
        lower,
        |z| {
            let (x, y) = super::beta::logistic_coordinates(z);
            let lp = if use_mixture {
                poisson_mixture(
                    0.5 * ncp,
                    |k, flags| super::pbeta_raw(x, y, a + k, b, log_tail, flags),
                    flags,
                    &mut budget,
                )
            } else {
                n_beta_tail((x, y), a, b, ncp, log_tail, flags, &mut budget)
            };
            let slope = (n_beta_density(x, y, a, b, ncp, flags, &mut budget) - lp).exp();
            (lp, if lower { slope } else { -slope })
        },
    );
    if budget == 0 { invalid(flags) } else { value }
}

/// Evaluate a noncentral beta quantile.
pub fn qnbeta(p: f64, a: f64, b: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || a.is_nan() || b.is_nan() || ncp.is_nan() {
        return p + a + b + ncp;
    }
    if !beta_parameters(a, b, ncp) || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, 0.0, 1.0) {
        return x;
    }
    let (lp, lq) = probability_logs(p, tail);
    super::beta::logistic_coordinates(n_beta_odds(lp, lq, a, b, ncp, flags)).0
}

/// Draw from a noncentral beta distribution.
pub fn rnbeta(rng: &mut Rng, a: f64, b: f64, ncp: f64, flags: &mut Flags) -> f64 {
    if a < 0.0 || b < 0.0 || ncp < 0.0 || a.is_nan() || b.is_nan() || !ncp.is_finite() {
        return invalid(flags);
    }
    let x = rnchisq(rng, 2.0 * a, ncp, flags);
    x / (x + rchisq(rng, 2.0 * b, flags))
}

fn f_parameters(df1: f64, df2: f64, ncp: f64) -> bool {
    df1 > 0.0 && df2 > 0.0 && df1.is_finite() && ncp >= 0.0 && ncp.is_finite()
}

/// Evaluate a noncentral F density.
pub fn dnf(x: f64, df1: f64, df2: f64, ncp: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || df1.is_nan() || df2.is_nan() || ncp.is_nan() {
        return x + df1 + df2 + ncp;
    }
    if df1 == INF && df2 > 0.0 && ncp >= 0.0 && ncp.is_finite() {
        if x == 0.0 {
            return invalid(flags);
        }
        return super::df(x, df1, df2, log, flags);
    }
    if !f_parameters(df1, df2, ncp) || x == INF {
        return invalid(flags);
    }
    if x < 0.0 {
        return zero(log);
    }
    if x == 0.0 {
        return on_scale(super::df(x, df1, df2, true, flags) - 0.5 * ncp, log);
    }
    if df2.is_infinite() {
        return on_scale(dnchisq(x * df1, df1, ncp, true, flags) + df1.ln(), log);
    }
    let (z, y) = f_coordinates(x, df1, df2);
    let mut budget = usize::MAX;
    on_scale(
        n_beta_density(z, y, 0.5 * df1, 0.5 * df2, ncp, flags, &mut budget) - x.ln(),
        log,
    )
}

/// Evaluate a noncentral F tail probability.
pub fn pnf(x: f64, df1: f64, df2: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || df1.is_nan() || df2.is_nan() || ncp.is_nan() {
        return x + df1 + df2 + ncp;
    }
    if df1 <= 0.0 || df2 <= 0.0 || ncp < 0.0 || !ncp.is_finite() {
        return invalid(flags);
    }
    if x <= 0.0 || x == INF {
        return step(x > 0.0, tail);
    }
    if df1 == INF {
        return invalid(flags);
    }
    if df2.is_infinite() {
        return pnchisq(x * df1, df1, ncp, tail, flags);
    }
    let (z, y) = f_coordinates(x, df1, df2);
    let mut budget = usize::MAX;
    n_beta_tail((z, y), 0.5 * df1, 0.5 * df2, ncp, tail, flags, &mut budget)
}

/// Evaluate a noncentral F quantile.
pub fn qnf(p: f64, df1: f64, df2: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || df1.is_nan() || df2.is_nan() || ncp.is_nan() {
        return p + df1 + df2 + ncp;
    }
    if df1 <= 0.0 || df2 <= 0.0 || ncp < 0.0 || !ncp.is_finite() || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, 0.0, INF) {
        return x;
    }
    if df1 == INF {
        return invalid(flags);
    }
    if df2 > 1e8 {
        return qnchisq(p, df1, ncp, tail, flags) / df1;
    }
    let (lp, lq) = probability_logs(p, tail);
    (n_beta_odds(lp, lq, 0.5 * df1, 0.5 * df2, ncp, flags) + (df2.ln() - df1.ln())).exp()
}

/// Draw from a noncentral F distribution.
pub fn rnf(rng: &mut Rng, df1: f64, df2: f64, ncp: f64, flags: &mut Flags) -> f64 {
    if !f_parameters(df1, df2, ncp) {
        return invalid(flags);
    }
    let numerator = rnchisq(rng, df1, ncp, flags) / df1;
    if df2.is_infinite() {
        return numerator;
    }
    numerator / (rchisq(rng, df2, flags) / df2)
}

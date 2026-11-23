//! Noncentral chi-square, beta, F, and Student's t distributions.

use crate::dist::normal::{dnorm, pnorm, qnorm};
use crate::rng::Rng;
use crate::special::aux::{bd0, stirlerr};

use super::beta::{beta_factor, rbeta};
use super::gamma::{dgamma, pgamma, rgamma};
use super::quadrature::{log_chisq_scale, log_integral};
use super::ratios::{f_coordinates, rchisq};
use super::*;

const INF: f64 = f64::INFINITY;

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
) -> f64 {
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

fn rpoisson(rng: &mut Rng, mean: f64, flags: &mut Flags) -> f64 {
    if mean == 0.0 {
        return 0.0;
    }
    if mean < 30.0 {
        let mut count = 0.0;
        let mut sum = rng.exp_rand();
        for _ in 0..100_000 {
            if sum > mean {
                return count;
            }
            count += 1.0;
            sum += rng.exp_rand();
        }
    } else if mean < 4_503_599_627_370_496.0 {
        // Use two geometric tails with a common center. The ratio of the
        // Poisson mass to this proposal peaks at the adjacent integers
        // around mean*exp(±rate).
        let mode = mean.floor();
        let rate = mean.sqrt().recip();
        let envelope = |k: f64| poisson_log_mass(k, mean) + rate * (k - mode).abs();
        let left = (mean * (-rate).exp()).floor();
        let right = (mean * rate.exp()).floor();
        let bound = envelope(mode).max(envelope(left)).max(envelope(right)) + 1e-12;
        for _ in 0..100_000 {
            let magnitude = (rng.exp_rand() / rate).floor();
            let negative = rng.unif_rand() < 0.5;
            if negative && magnitude == 0.0 {
                continue;
            }
            let k = if negative {
                mode - magnitude
            } else {
                mode + magnitude
            };
            if k >= 0.0 && rng.unif_rand().ln() <= envelope(k) - bound {
                return k;
            }
        }
    } else {
        flags.set(Flags::PRECISION);
        return mean.sqrt().mul_add(rng.norm_rand(), mean).round();
    }
    flags.set(Flags::NO_CONVERGENCE);
    f64::NAN
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
    let value = poisson_mixture(
        0.5 * ncp,
        |k, flags| dgamma(x, 0.5 * df + k, 2.0, true, flags),
        flags,
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
    let log_tail = Tail { log: true, ..tail };
    let mean = 0.5 * ncp;
    let component = |k, flags: &mut Flags| pgamma(x, 0.5 * df + k, 2.0, log_tail, flags);
    let value = if ncp < 80.0 {
        poisson_small(mean, component, flags)
    } else {
        poisson_mixture(mean, component, flags)
    };
    on_scale(value.min(0.0), tail.log)
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
    positive_quantile(lp.min(lq), lower, (df + ncp).min(f64::MAX), |x| {
        (
            pnchisq(x, df, ncp, log_tail, flags),
            if ncp < 80.0 && x.is_finite() && x > 0.0 {
                poisson_small(
                    0.5 * ncp,
                    |k, flags| dgamma(x, 0.5 * df + k, 2.0, true, flags),
                    flags,
                )
            } else {
                dnchisq(x, df, ncp, true, flags)
            },
        )
    })
}

/// Draw from a noncentral chi-square distribution.
pub fn rnchisq(rng: &mut Rng, df: f64, ncp: f64, flags: &mut Flags) -> f64 {
    if df < 0.0 || ncp < 0.0 || !df.is_finite() || !ncp.is_finite() {
        return invalid(flags);
    }
    let k = rpoisson(rng, 0.5 * ncp, flags);
    rgamma(rng, k, 2.0, flags) + rchisq(rng, df, flags)
}

fn beta_parameters(a: f64, b: f64, ncp: f64) -> bool {
    a > 0.0 && b > 0.0 && ncp >= 0.0 && a.is_finite() && b.is_finite() && ncp.is_finite()
}

fn n_beta_density(x: f64, y: f64, a: f64, b: f64, ncp: f64, flags: &mut Flags) -> f64 {
    poisson_mixture(
        0.5 * ncp,
        |k, flags| beta_factor(x, y, a + k, b, flags),
        flags,
    )
}

// The next beta CDF bounds every component that follows it in the series.
fn truncated_beta_lower(x: f64, y: f64, a: f64, b: f64, mean: f64, flags: &mut Flags) -> f64 {
    let start = (mean - 7.0 * mean.sqrt()).floor().max(0.0);
    let mut weight = poisson_log_mass(start, mean).exp();
    let mut remainder = 1.0 - weight;
    let shape = a + start;
    let mut value = super::pbeta_raw(x, y, shape, b, Tail::default(), flags);
    let mut decrease = (beta_factor(x, y, shape, b, flags) - shape.ln()).exp();
    let mut sum = weight * value;
    for i in 1..=100_000 {
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

fn n_beta_tail(x: f64, y: f64, a: f64, b: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if ncp < 20_000.0 {
        let lower = truncated_beta_lower(x, y, a, b, 0.5 * ncp, flags);
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
    );
    on_scale(lp.min(0.0), tail.log)
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
    on_scale(
        n_beta_density(x, y, a, b, ncp, flags) - x.ln() - y.ln(),
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
    n_beta_tail(x, complement(x), a, b, ncp, tail, flags)
}

fn n_beta_odds(lp: f64, lq: f64, a: f64, b: f64, ncp: f64, flags: &mut Flags) -> f64 {
    let lower = lp <= lq;
    let log_tail = Tail { lower, log: true };
    let target = lp.min(lq);
    let use_mixture = !lower && target < -23.0;
    let seed = super::beta::beta_quantile_odds(lp, lq, a + 0.5 * ncp, b, flags);
    invert(
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
                )
            } else {
                n_beta_tail(x, y, a, b, ncp, log_tail, flags)
            };
            let slope = (n_beta_density(x, y, a, b, ncp, flags) - lp).exp();
            (lp, if lower { slope } else { -slope })
        },
    )
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
    let k = rpoisson(rng, 0.5 * ncp, flags);
    rbeta(rng, a + k, b, flags)
}

fn f_parameters(df1: f64, df2: f64, ncp: f64) -> bool {
    df1 > 0.0 && df2 > 0.0 && df1.is_finite() && ncp >= 0.0 && ncp.is_finite()
}

/// Evaluate a noncentral F density.
pub fn dnf(x: f64, df1: f64, df2: f64, ncp: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || df1.is_nan() || df2.is_nan() || ncp.is_nan() {
        return x + df1 + df2 + ncp;
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
    on_scale(
        n_beta_density(z, y, 0.5 * df1, 0.5 * df2, ncp, flags) - x.ln(),
        log,
    )
}

/// Evaluate a noncentral F tail probability.
pub fn pnf(x: f64, df1: f64, df2: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || df1.is_nan() || df2.is_nan() || ncp.is_nan() {
        return x + df1 + df2 + ncp;
    }
    if !f_parameters(df1, df2, ncp) {
        return invalid(flags);
    }
    if x <= 0.0 || x == INF {
        return step(x > 0.0, tail);
    }
    if df2.is_infinite() {
        return pnchisq(x * df1, df1, ncp, tail, flags);
    }
    let (z, y) = f_coordinates(x, df1, df2);
    n_beta_tail(z, y, 0.5 * df1, 0.5 * df2, ncp, tail, flags)
}

/// Evaluate a noncentral F quantile.
pub fn qnf(p: f64, df1: f64, df2: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || df1.is_nan() || df2.is_nan() || ncp.is_nan() {
        return p + df1 + df2 + ncp;
    }
    if !f_parameters(df1, df2, ncp) || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, 0.0, INF) {
        return x;
    }
    if df2.is_infinite() {
        return qnchisq(p, df1, ncp, tail, flags) / df1;
    }
    let (lp, lq) = probability_logs(p, tail);
    (n_beta_odds(lp, lq, 0.5 * df1, 0.5 * df2, ncp, flags) + (df2 / df1).ln()).exp()
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

// Expand the normal integral in beta ratios. The odd and even terms
// come from the two parts of the noncentral normal density.
fn t_series(x: f64, df: f64, ncp: f64, flags: &mut Flags) -> Option<f64> {
    let mean = 0.5 * ncp * ncp;
    if mean > 100.0 || df > 400_000.0 || x == 0.0 {
        return None;
    }
    let delta = if x < 0.0 { -ncp } else { ncp };
    let square = x * x;
    let (z, y) = if square.is_normal() && (df + square).is_finite() {
        (square / (df + square), df / (df + square))
    } else {
        super::beta::logistic_coordinates(2.0 * x.abs().ln() - df.ln())
    };
    let b = 0.5 * df;
    let mut p = 0.5 * (-mean).exp();
    let mut q = (2.0 / std::f64::consts::PI).sqrt() * p * delta;
    let mut remainder = 0.5 - p;
    let mut odd = super::pbeta_raw(z, y, 0.5, b, Tail::default(), flags);
    let log_y = if y > 0.5 { (-z).ln_1p() } else { y.ln() };
    let power = (b * log_y).exp();
    let mut even = -(b * log_y).exp_m1();
    let mut odd_delta = (beta_factor(z, y, 0.5, b, flags) + std::f64::consts::LN_2).exp();
    let mut even_delta = b * z * power;
    let mut sum = p.mul_add(odd, q * even);
    for j in 1..=1000 {
        let n = f64::from(j);
        odd -= odd_delta;
        even -= even_delta;
        odd_delta *= z * ((n - 0.5 + b) / (n + 0.5));
        even_delta *= z * ((n + b) / (n + 1.0));
        p *= mean / n;
        q *= mean / (n + 0.5);
        remainder -= p;
        sum += p.mul_add(odd, q * even);
        let error = 2.0 * remainder * (odd - odd_delta);
        if error <= 1e-12 {
            break;
        }
        if j == 1000 {
            flags.set(Flags::NO_CONVERGENCE);
        }
    }
    let base = pnorm(-delta, 0.0, 1.0, Tail::default(), flags);
    let cdf = (sum + base).min(1.0);
    if !cdf.is_finite() || cdf < 1e-290 || cdf == 1.0 {
        return None;
    }
    Some(cdf)
}

fn t_mixture(x: f64, df: f64, ncp: f64, density: bool, lower: bool) -> f64 {
    let root_df = df.sqrt();
    let log_center = (root_df.max(ncp.abs()).max(1.0).ln() - x.abs().ln()).min(0.0);
    let shift = root_df * log_center;
    let range = 4.0_f64.max(root_df.recip());
    let f = |w: f64| {
        let h = w / root_df;
        let z = if h.abs() < 0.5 {
            x.mul_add(h.exp_m1(), x - ncp)
        } else {
            (x.abs().ln() + h).exp().copysign(x) - ncp
        };
        let normal = if density {
            dnorm(z, 0.0, 1.0, true, &mut Flags::default()) + h
        } else {
            pnorm(
                z,
                0.0,
                1.0,
                Tail { lower, log: true },
                &mut Flags::default(),
            )
        };
        log_chisq_scale(w, df) + normal
    };
    log_integral(shift, 0.0, range, f)
}

/// Evaluate the noncentral Student's t density.
pub fn dnt(x: f64, df: f64, ncp: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || df.is_nan() || ncp.is_nan() {
        return x + df + ncp;
    }
    if df <= 0.0 || !ncp.is_finite() {
        return invalid(flags);
    }
    if df.is_infinite() {
        return dnorm(x, ncp, 1.0, log, flags);
    }
    if ncp == 0.0 {
        return super::dt(x, df, log, flags);
    }
    if x.is_infinite() {
        return zero(log);
    }
    if x.abs() > 1e-4 {
        let a = t_series(x, df, ncp, flags);
        let b = t_series(x * ((df + 2.0) / df).sqrt(), df + 2.0, ncp, flags);
        if let (Some(a), Some(b)) = (a, b) {
            let density = ((b - a) / x).abs() * df;
            if density > 0.0 {
                return if log { density.ln() } else { density };
            }
        }
    }
    on_scale(t_mixture(x, df, ncp, true, true), log)
}

/// Evaluate a noncentral Student's t tail probability.
pub fn pnt(x: f64, df: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || df.is_nan() || ncp.is_nan() {
        return x + df + ncp;
    }
    if df <= 0.0 || !ncp.is_finite() {
        return invalid(flags);
    }
    if df.is_infinite() || x == 0.0 {
        return pnorm(x, ncp, 1.0, tail, flags);
    }
    if ncp == 0.0 {
        return super::pt(x, df, tail, flags);
    }
    if x.is_infinite() {
        return step(x > 0.0, tail);
    }
    if let Some(cdf) = t_series(x, df, ncp, flags) {
        let lower = x >= 0.0;
        if cdf > 1.0 - 1e-10 {
            flags.set(Flags::PRECISION);
        }
        return if lower == tail.lower {
            if tail.log { cdf.ln() } else { cdf }
        } else if tail.log {
            (-cdf).ln_1p()
        } else {
            1.0 - cdf
        };
    }
    let lower = if x >= ncp {
        false
    } else {
        tail.lower || x < 0.0
    };
    from_log(t_mixture(x, df, ncp, false, lower).min(0.0), lower, tail)
}

/// Evaluate a noncentral Student's t quantile.
pub fn qnt(p: f64, df: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || df.is_nan() || ncp.is_nan() {
        return p + df + ncp;
    }
    if df <= 0.0 || !ncp.is_finite() || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, -INF, INF) {
        return x;
    }
    if df.is_infinite() {
        return qnorm(p, ncp, 1.0, tail, flags);
    }
    if ncp == 0.0 {
        return super::qt(p, df, tail, flags);
    }
    let (lp, lq) = probability_logs(p, tail);
    let lower = lp <= lq;
    let log_tail = Tail { lower, log: true };
    let target = lp.min(lq);
    let seed = ncp + super::qt(p, df, tail, flags);
    let convert = |z: f64| z.sinh();
    let z = invert(
        -710.0,
        710.0,
        seed.asinh().clamp(-709.0, 709.0),
        target,
        lower,
        |z| {
            let x = convert(z);
            let (lp, ld) = if target < -26.0 {
                (
                    t_mixture(x, df, ncp, false, lower),
                    t_mixture(x, df, ncp, true, true),
                )
            } else {
                (
                    pnt(x, df, ncp, log_tail, flags),
                    dnt(x, df, ncp, true, flags),
                )
            };
            let slope = (ld - lp + z.cosh().ln()).exp();
            (lp, if lower { slope } else { -slope })
        },
    );
    convert(z)
}

/// Draw from a noncentral Student's t distribution.
pub fn rnt(rng: &mut Rng, df: f64, ncp: f64, flags: &mut Flags) -> f64 {
    if df <= 0.0 || df.is_nan() || !ncp.is_finite() {
        return invalid(flags);
    }
    let normal = rng.norm_rand() + ncp;
    if df.is_infinite() {
        return normal;
    }
    normal / (rchisq(rng, df, flags) / df).sqrt()
}

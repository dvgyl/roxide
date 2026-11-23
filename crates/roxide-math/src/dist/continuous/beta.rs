//! Beta density and incomplete beta ratios.

use crate::dist::normal::{pnorm, standard_quantile};
use crate::rng::Rng;
use crate::special::aux::{lgamma1p, log1pexp, log1pmx, stirlerr};
use crate::special::{lbeta, lgamma};

use super::gamma::{pgamma, rgamma};
use super::*;

const INF: f64 = f64::INFINITY;
const TOL: f64 = f64::EPSILON;

/// Evaluate the beta density, with point masses for degenerate shapes.
pub fn dbeta(x: f64, a: f64, b: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || a.is_nan() || b.is_nan() {
        return x + a + b;
    }
    if a < 0.0 || b < 0.0 {
        return invalid(flags);
    }
    if !(0.0..=1.0).contains(&x) {
        return zero(log);
    }
    if let Some(at) = atom(a, b) {
        return if x == at { INF } else { zero(log) };
    }
    if a == 0.0 && b == 0.0 {
        return if x == 0.0 || x == 1.0 { INF } else { zero(log) };
    }
    if x == 0.0 {
        return if a < 1.0 {
            INF
        } else if a == 1.0 {
            if log { b.ln() } else { b }
        } else {
            zero(log)
        };
    }
    if x == 1.0 {
        return dbeta(0.0, b, a, log, flags);
    }
    let y = complement(x);
    on_scale(
        beta_factor(x, y, a, b, flags) - coordinate_log(x, y) - coordinate_log(y, x),
        log,
    )
}

fn atom(a: f64, b: f64) -> Option<f64> {
    if a.is_infinite() && b.is_infinite() {
        Some(0.5)
    } else if (a == 0.0 && b > 0.0) || b.is_infinite() {
        Some(0.0)
    } else if (b == 0.0 && a > 0.0) || a.is_infinite() {
        Some(1.0)
    } else {
        None
    }
}

/// Evaluate a beta tail probability.
pub fn pbeta(x: f64, a: f64, b: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || a.is_nan() || b.is_nan() {
        return x + a + b;
    }
    if a < 0.0 || b < 0.0 {
        return invalid(flags);
    }
    if x >= 1.0 {
        return step(true, tail);
    }
    if let Some(at) = atom(a, b) {
        return step(at == 0.0 || x >= at, tail);
    }
    if a == 0.0 && b == 0.0 {
        return if tail.log {
            -std::f64::consts::LN_2
        } else {
            0.5
        };
    }
    if x <= 0.0 {
        return step(false, tail);
    }
    pbeta_raw(x, complement(x), a, b, tail, flags)
}

/// Evaluate a beta tail from independently computed coordinates `x` and `1 - x`.
pub fn pbeta_raw(x: f64, y: f64, a: f64, b: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || y.is_nan() || a.is_nan() || b.is_nan() {
        return x + y + a + b;
    }
    if !(0.0..=1.0).contains(&x)
        || !(0.0..=1.0).contains(&y)
        || ((x + y) - 1.0).abs() > 4.0 * TOL
        || a < 0.0
        || b < 0.0
    {
        return invalid(flags);
    }
    if a == 0.0 || b == 0.0 || a.is_infinite() || b.is_infinite() {
        return pbeta(x, a, b, tail, flags);
    }
    if x == 0.0 || y == 0.0 {
        return step(y == 0.0, tail);
    }
    let (lp, lower) = beta_tail(x, y, a, b, flags);
    from_log(if lp > 0.0 { 0.0 } else { lp }, lower, tail)
}

fn beta_tail(mut x: f64, mut y: f64, mut a: f64, mut b: f64, flags: &mut Flags) -> (f64, bool) {
    let mut lower = true;
    if a == b && x == y {
        return (-std::f64::consts::LN_2, true);
    }
    if b == 1.0 {
        return (a * coordinate_log(x, y), true);
    }
    if a == 1.0 {
        return (b * coordinate_log(y, x), false);
    }
    if a.min(b) <= 1.0 {
        if x > 0.5 {
            (x, y, a, b) = (y, x, b, a);
            lower = false;
        }
        if a.max(b) <= 1.0 {
            if a >= 0.2_f64.min(b) || a * x.ln() <= 0.9_f64.ln() {
                return (bpser(x, y, a, b, flags), lower);
            }
        } else if b <= 1.0 || (x < 0.1 && a * (b * x).ln() <= 0.7_f64.ln()) {
            return (bpser(x, y, a, b, flags), lower);
        }
        let complement = if x >= 0.3 {
            bpser(y, x, b, a, flags)
        } else {
            shifted_bgrat(y, x, b, a, flags)
        };
        return (complement, !lower);
    }

    let mut lambda = beta_deviation(x, y, a, b);
    if lambda < 0.0 {
        (x, y, a, b) = (y, x, b, a);
        lambda = -lambda;
        lower = false;
    }
    let value = if b < 40.0 {
        if b * x <= 0.7 {
            bpser(x, y, a, b, flags)
        } else {
            let n = b.ceil() as u32 - 1;
            let s = b - f64::from(n);
            let remainder = if x <= 0.7 {
                bpser(x, y, a, s, flags)
            } else {
                shifted_bgrat(x, y, a, s, flags)
            };
            log_add(bup(y, x, s, a, n, flags), remainder)
        }
    } else if a.min(b) > 100.0 && lambda <= 0.03 * a.min(b) {
        basym(a, b, lambda)
    } else {
        bfrac(x, y, a, b, lambda, flags)
    };
    (value, lower)
}

fn coordinate_log(x: f64, y: f64) -> f64 {
    if x > 0.5 { (-y).ln_1p() } else { x.ln() }
}

fn beta_deviation(x: f64, y: f64, a: f64, b: f64) -> f64 {
    // Use the smaller coordinate. Its complement can lose useful bits.
    if x <= y {
        let bx = b * x;
        a.mul_add(-x, a - bx) - b.mul_add(x, -bx)
    } else {
        let ay = a * y;
        b.mul_add(y, ay - b) + a.mul_add(y, -ay)
    }
}

fn beta_deviance(x: f64, y: f64, a: f64, b: f64, lambda: f64) -> f64 {
    let da = -lambda / a;
    let db = lambda / b;
    if da.abs().max(db.abs()) <= 0.5 {
        return -a * log1pmx(da) - b * log1pmx(db);
    }
    let lo = a.min(b);
    let hi = a.max(b);
    let ratio = lo / hi;
    let log_hi = -ratio.ln_1p();
    let log_lo = ratio.ln() + log_hi;
    let (lp, lq) = if a <= b {
        (log_lo, log_hi)
    } else {
        (log_hi, log_lo)
    };
    -a * (coordinate_log(x, y) - lp) - b * (coordinate_log(y, x) - lq)
}

// Logarithm of x^a y^b / B(a,b), with the terms about the mean combined.
pub(super) fn beta_factor(x: f64, y: f64, a: f64, b: f64, flags: &mut Flags) -> f64 {
    if a.min(b) >= 8.0 {
        let lambda = beta_deviation(x, y, a, b);
        let effective = a.min(b) / (1.0 + a.min(b) / a.max(b));
        0.5 * effective.ln() - LOG_SQRT_2PI + stirlerr(a + b)
            - stirlerr(a)
            - stirlerr(b)
            - beta_deviance(x, y, a, b, lambda)
    } else {
        a * coordinate_log(x, y) + b * coordinate_log(y, x) - lbeta(a, b, flags)
    }
}

fn bpser(x: f64, y: f64, a: f64, b: f64, flags: &mut Flags) -> f64 {
    let mut coefficient = 1.0;
    let mut sum = 0.0;
    for n in 1..=10_000 {
        let n = f64::from(n);
        coefficient *= x * (1.0 - b / n);
        let term = coefficient / (a + n);
        sum += term;
        if term.abs() * a <= TOL * (1.0 + a * sum).abs() {
            break;
        }
        if n == 10_000.0 {
            flags.set(Flags::NO_CONVERGENCE);
        }
    }
    let log_abeta = if a.max(b) <= 0.5 {
        lgamma1p(a) + lgamma1p(b) - lgamma1p(a + b) + (a / b).ln_1p()
    } else {
        a.ln() + lbeta(a, b, flags)
    };
    a * coordinate_log(x, y) - log_abeta + (a * sum).ln_1p()
}

fn bup(x: f64, y: f64, a: f64, b: f64, n: u32, flags: &mut Flags) -> f64 {
    if n == 0 {
        return f64::NEG_INFINITY;
    }
    let mut lp = beta_factor(x, y, a, b, flags) - a.ln();
    let mut sum = lp;
    for j in 1..n {
        let j = f64::from(j);
        lp += (x + x * (b - 1.0) / (a + j)).ln();
        sum = log_add(sum, lp);
    }
    sum
}

fn shifted_bgrat(x: f64, y: f64, a: f64, b: f64, flags: &mut Flags) -> f64 {
    if a >= 15.0 {
        bgrat(x, y, a, b, flags)
    } else {
        log_add(bup(x, y, a, b, 20, flags), bgrat(x, y, a + 20.0, b, flags))
    }
}

fn bgrat(x: f64, y: f64, a: f64, b: f64, flags: &mut Flags) -> f64 {
    if b == 1.0 {
        return a * coordinate_log(x, y);
    }
    let center = a + 0.5 * (b - 1.0);
    let u = -center * coordinate_log(x, y);
    let base = pgamma(u, b, 1.0, LOG_UPPER, flags) + lgamma(b, flags);
    if base == f64::NEG_INFINITY {
        return base;
    }
    let scale = 0.5 / center;
    let edge = u * scale;
    let mut moment = 1.0;
    let mut edge_term = (b * u.ln() - u - base).exp() * scale;
    let mut sum = 1.0;
    let mut sinh = [0.0; 32];
    let mut coef = [0.0; 32];
    sinh[0] = 1.0;
    coef[0] = 1.0;
    for j in 1..sinh.len() {
        let k = (2 * j) as f64;
        sinh[j] = sinh[j - 1] / (k * (k + 1.0));
        let mut next = 0.0;
        for i in 1..=j {
            next += (b * i as f64 - j as f64) * sinh[i] * coef[j - i];
        }
        coef[j] = next / j as f64;
        let order = b + k - 2.0;
        moment = (order * scale * moment + edge_term) * ((order + 1.0) * scale) + edge_term * edge;
        edge_term *= edge * edge;
        let correction = coef[j] * moment;
        sum += correction;
        if correction.abs() <= TOL * sum.abs() && j >= 3 {
            break;
        }
        if j == sinh.len() - 1 {
            flags.set(Flags::PRECISION);
        }
    }
    base - lbeta(a, b, flags) - b * center.ln() + sum.ln()
}

fn bfrac(x: f64, y: f64, a: f64, b: f64, lambda: f64, flags: &mut Flags) -> f64 {
    let first = (a / (a + 1.0)) * (1.0 + lambda);
    let mut c = first;
    let mut d = 0.0;
    let mut value = first;
    for k in 1..=10_000 {
        let n = f64::from(k);
        let denominator = a + 2.0 * n - 1.0;
        let scaled = n * ((b - n) * x / denominator);
        let alpha = ((a + n - 1.0) / denominator) * scaled * ((a + n - 1.0) * x + b * x);
        let beta = n + scaled + ((a + n) / (a + 2.0 * n + 1.0)) * (1.0 + lambda + n * (1.0 + y));
        let cd = alpha.mul_add(d, beta);
        let cc = beta + alpha / c;
        d = if cd.abs() < 1e-300 {
            1e300_f64.copysign(cd)
        } else {
            cd.recip()
        };
        c = if cc.abs() < 1e-300 {
            1e-300_f64.copysign(cc)
        } else {
            cc
        };
        let ratio = c * d;
        value *= ratio;
        if (ratio - 1.0).abs() <= 2.0 * TOL {
            break;
        }
        if k == 10_000 {
            flags.set(Flags::NO_CONVERGENCE);
        }
    }
    beta_factor(x, y, a, b, flags) - value.ln()
}

// DiDonato–Morris (16). Integrate the inverse series in the signed
// square root of the deviance. Scale each moment before the recurrence.
fn basym(a: f64, b: f64, lambda: f64) -> f64 {
    const TERMS: usize = 24;
    let ratio = a.min(b) / a.max(b);
    let min_share = ratio / (1.0 + ratio);
    let max_share = 1.0 / (1.0 + ratio);
    let (p, q, h_p, h_q) = if a <= b {
        (min_share, max_share, 1.0, ratio)
    } else {
        (max_share, min_share, ratio, 1.0)
    };
    let deviance = -a * log1pmx(-lambda / a) - b * log1pmx(lambda / b);
    let z = deviance.sqrt() * std::f64::consts::SQRT_2;
    let base = pnorm(-z, 0.0, 1.0, LOG_TAIL, &mut Flags::default());
    if base.is_infinite() {
        return base;
    }
    let variance = max_share / a.min(b);
    let v = z * variance.sqrt();
    let mut a_poly = [0.0; TERMS];
    let mut inverse = [0.0; TERMS];
    let mut reciprocal = [0.0; TERMS];
    let mut moment = [0.0; TERMS];
    a_poly[0] = 1.0;
    inverse[0] = 1.0;
    reciprocal[0] = 1.0;
    moment[0] = 1.0;
    let mills = (-deviance - LOG_SQRT_2PI - base).exp() * variance.sqrt();
    let mut edge = mills;
    let mut sum = 1.0;
    let mut prior = 1.0;
    for n in 1..TERMS {
        a_poly[n] = 2.0 / (n + 2) as f64 * (q * h_p.powi(n as i32) + p * (-h_q).powi(n as i32));
        // Coefficient of s^n in A(s)^(-(n+1)/2).
        let power = -0.5 * (n + 1) as f64;
        let mut powered = [0.0; TERMS];
        powered[0] = 1.0;
        for j in 1..=n {
            let mut coefficient = 0.0;
            for i in 1..=j {
                coefficient += ((power + 1.0) * i as f64 - j as f64) * a_poly[i] * powered[j - i];
            }
            powered[j] = coefficient / j as f64;
        }
        inverse[n] = powered[n] / (n + 1) as f64;
        reciprocal[n] = -(1..=n).map(|k| inverse[k] * reciprocal[n - k]).sum::<f64>();
        moment[n] = if n == 1 {
            edge
        } else {
            edge + (n - 1) as f64 * variance * moment[n - 2]
        };
        edge *= v;
        let correction = reciprocal[n] * moment[n];
        sum += correction;
        if n % 2 == 0 && correction.abs() + prior <= TOL * sum.abs() {
            break;
        }
        prior = correction.abs();
    }
    base + sum.ln() + stirlerr(a + b) - stirlerr(a) - stirlerr(b)
}

/// Evaluate a beta quantile.
pub fn qbeta(p: f64, a: f64, b: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || a.is_nan() || b.is_nan() {
        return p + a + b;
    }
    if a < 0.0 || b < 0.0 || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, 0.0, 1.0) {
        return x;
    }
    if let Some(at) = atom(a, b) {
        return at;
    }
    let (lp, lq) = probability_logs(p, tail);
    if a == 0.0 && b == 0.0 {
        return if lp < lq {
            0.0
        } else if lp > lq {
            1.0
        } else {
            0.5
        };
    }
    let x = logistic_coordinates(beta_quantile_odds(lp, lq, a, b, flags)).0;
    if x == 0.0 && a < 1.0 {
        0.5 * f64::MIN_POSITIVE
    } else {
        x
    }
}

pub(super) fn logistic_coordinates(log_odds: f64) -> (f64, f64) {
    let z = (-log_odds.abs()).exp();
    let big = (1.0 + z).recip();
    let small = z * big;
    if log_odds <= 0.0 {
        (small, big)
    } else {
        (big, small)
    }
}

pub(super) fn beta_quantile_odds(lp: f64, lq: f64, a: f64, b: f64, flags: &mut Flags) -> f64 {
    if (a + b).is_infinite() && a.min(b) > 1e32 {
        return (a / b).ln();
    }
    let swap = lp > lq;
    let (lp, lq, a, b) = if swap { (lq, lp, b, a) } else { (lp, lq, a, b) };
    let odds = if a == 1.0 {
        let ly = lq / b;
        log1mexp(-ly) - ly
    } else if b == 1.0 {
        let lx = lp / a;
        lx - log1mexp(-lx)
    } else {
        let normalization = lbeta(a, b, flags);
        let power = (lp + a.ln() + normalization) / a;
        let reflected = -(lq + b.ln() + normalization) / b;
        if power < -700.0 && power + b.ln() < -40.0 {
            return if swap { -power } else { power };
        }
        if reflected > 700.0 && a.ln() - reflected < -40.0 {
            return if swap { -reflected } else { reflected };
        }
        let mean_odds = a.ln() - b.ln();
        let (mean, other) = logistic_coordinates(mean_odds);
        let sd = (mean * other / (1.0 + a + b)).sqrt();
        let normal = standard_quantile(lp, LOG_TAIL);
        let initial = normal.mul_add(sd, mean);
        let seed = if power < -4.0 {
            power
        } else if reflected > 4.0 {
            reflected
        } else if initial > 0.0 && initial < 1.0 {
            initial.ln() - (-initial).ln_1p()
        } else {
            mean_odds
        };
        invert(-746.0, 746.0, seed.clamp(-746.0, 746.0), lp, true, |z| {
            let (x, y) = logistic_coordinates(z);
            let value = pbeta_raw(x, y, a, b, LOG_TAIL, flags);
            let slope = (beta_factor(x, y, a, b, flags) - value).exp();
            (
                value,
                halley_slope(slope, beta_deviation(x, y, a, b) - slope, value - lp),
            )
        })
    };
    if swap { -odds } else { odds }
}

/// Draw from a beta distribution with the specified shapes.
pub fn rbeta(rng: &mut Rng, a: f64, b: f64, flags: &mut Flags) -> f64 {
    if a.is_nan() || b.is_nan() || a < 0.0 || b < 0.0 {
        return invalid(flags);
    }
    if let Some(at) = atom(a, b) {
        return at;
    }
    if a == 0.0 && b == 0.0 {
        return if rng.unif_rand() < 0.5 { 0.0 } else { 1.0 };
    }
    let mut draw = |shape: f64| {
        if shape < 1.0 {
            rgamma(rng, shape + 1.0, 1.0, flags).ln() + rng.unif_rand().ln() / shape
        } else {
            rgamma(rng, shape, 1.0, flags).ln()
        }
    };
    let x = draw(a);
    let y = draw(b);
    if x == f64::NEG_INFINITY && y == f64::NEG_INFINITY {
        let (p, _) = logistic_coordinates(a.ln() - b.ln());
        return if rng.unif_rand() < p { 1.0 } else { 0.0 };
    }
    (-log1pexp(y - x)).exp()
}

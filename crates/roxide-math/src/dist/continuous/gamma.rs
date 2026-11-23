//! Gamma density, incomplete gamma ratios, and sampling.

use crate::dist::normal::{mills_ratio, pnorm, standard_quantile};
use crate::rng::Rng;
use crate::special::aux::{EULER, bd0, lgamma1p, log1pmx, stirlerr};
use crate::special::lgamma;

use super::*;

const INF: f64 = f64::INFINITY;
const TINY: f64 = 1e-300;
const TOL: f64 = f64::EPSILON;

/// Evaluate the gamma density with the specified shape and scale.
pub fn dgamma(x: f64, shape: f64, scale: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || shape.is_nan() || scale.is_nan() {
        return x + shape + scale;
    }
    if shape < 0.0 || scale <= 0.0 {
        return invalid(flags);
    }
    if x < 0.0 {
        return zero(log);
    }
    if shape == 0.0 {
        return if x == 0.0 { INF } else { zero(log) };
    }
    if x == 0.0 {
        return if shape < 1.0 {
            INF
        } else if shape == 1.0 {
            if log { -scale.ln() } else { scale.recip() }
        } else {
            zero(log)
        };
    }
    let z = x / scale;
    if x.is_infinite() || z.is_infinite() || shape.is_infinite() {
        return zero(log);
    }
    let lx = if z.is_normal() {
        z.ln()
    } else {
        x.ln() - scale.ln()
    };
    on_scale(gamma_factor(z, lx, shape) - x.ln(), log)
}

// Logarithm of z^a exp(-z) / Gamma(a).
fn gamma_factor(z: f64, lz: f64, a: f64) -> f64 {
    if a >= 8.0 && z > 0.0 && z / a > 0.01 {
        0.5 * a.ln() - LOG_SQRT_2PI - stirlerr(a) - bd0(a, z)
    } else {
        a.mul_add(lz, -z) + a.ln() - lgamma1p(a)
    }
}

/// Evaluate a gamma tail probability with the specified shape and scale.
pub fn pgamma(x: f64, shape: f64, scale: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || shape.is_nan() || scale.is_nan() {
        return x + shape + scale;
    }
    if shape < 0.0 || scale <= 0.0 {
        return invalid(flags);
    }
    let z = x / scale;
    if z.is_nan() {
        return invalid(flags);
    }
    if x <= 0.0 || z.is_infinite() {
        return step(x > 0.0, tail);
    }
    if shape == 0.0 {
        return step(true, tail);
    }
    if shape.is_infinite() {
        return if z < 1.0 {
            invalid(flags)
        } else {
            step(false, tail)
        };
    }
    let lz = if z < f64::MIN_POSITIVE {
        x.ln() - scale.ln()
    } else {
        z.ln()
    };
    let (lp, lower) = if z == 0.0 && shape > 0.5 {
        (shape * lz - lgamma1p(shape), true)
    } else {
        gamma_tail(z, lz, shape, flags)
    };
    let lp = if (0.0..=8.0 * TOL).contains(&lp) {
        0.0
    } else {
        lp
    };
    from_log(lp, lower, tail)
}

fn gamma_tail(z: f64, lz: f64, a: f64, flags: &mut Flags) -> (f64, bool) {
    if a == 1.0 {
        return (-z, false);
    }
    if a <= 0.5 && z <= 1.1 {
        let correction = if a < f64::EPSILON {
            -EULER
        } else {
            lgamma1p(a) / a
        };
        let h_per_shape = lz - correction;
        let h = a * h_per_shape;
        let mut term = -z;
        let mut sum = term / (a + 1.0);
        for k in 2..=64 {
            let n = f64::from(k);
            term *= -z / n;
            let next = term / (a + n);
            sum += next;
            if next.abs() <= sum.abs() * TOL {
                break;
            }
        }
        let exprel = if h == 0.0 { 1.0 } else { h.exp_m1() / h };
        let log_q = a.ln() + (-h_per_shape * exprel - h.exp() * sum).ln();
        if log_q < 0.75_f64.ln() {
            return (log_q, false);
        }
        return (h + (a * sum).ln_1p(), true);
    }
    let delta = (z - a) / a;
    if a >= 1000.0 && delta.abs() <= 0.25 {
        return temme(a, delta);
    }
    if z < a + 1.0 {
        let mut term = 1.0;
        let mut sum = 1.0;
        for k in 1..=100_000 {
            let n = f64::from(k);
            term *= z / (a + n);
            sum += term;
            let next_ratio = z / (a + n + 1.0);
            if term * next_ratio <= TOL * sum * (1.0 - next_ratio) {
                return (gamma_factor(z, lz, a) - a.ln() + sum.ln(), true);
            }
        }
        flags.set(Flags::NO_CONVERGENCE);
        (gamma_factor(z, lz, a) - a.ln() + sum.ln(), true)
    } else {
        let first = (z - a) + 1.0;
        let mut value = 1.0;
        let mut c = 1.0;
        let mut d = 0.0;
        for k in 1..=100_000 {
            let n = f64::from(k);
            let b = 1.0 + (2.0 * n) / first;
            let alpha = (n / first) * ((a - n) / first);
            d = floor_magnitude(alpha.mul_add(d, b)).recip();
            c = floor_magnitude(alpha / c + b);
            let ratio = c * d;
            value *= ratio;
            if (ratio - 1.0).abs() <= TOL {
                return (gamma_factor(z, lz, a) - first.ln() - value.ln(), false);
            }
        }
        flags.set(Flags::NO_CONVERGENCE);
        (gamma_factor(z, lz, a) - first.ln() - value.ln(), false)
    }
}

fn floor_magnitude(value: f64) -> f64 {
    if value.abs() < TINY {
        TINY.copysign(value)
    } else {
        value
    }
}

// DLMF 8.12.7–8.12.13. Generate the inverse series from
// delta * d(delta)/d(eta) = eta * (1 + delta).
const fn temme_coefficients() -> [[f64; 22]; 6] {
    let mut inverse = [0.0; 24];
    inverse[1] = 1.0;
    let mut n = 2;
    while n < inverse.len() {
        let mut sum = 0.0;
        let mut k = 2;
        while k < n {
            sum += inverse[k] * inverse[n + 1 - k];
            k += 1;
        }
        inverse[n] = inverse[n - 1] / (n + 1) as f64 - 0.5 * sum;
        n += 1;
    }
    let g = [
        1.0,
        -1.0 / 12.0,
        1.0 / 288.0,
        139.0 / 51840.0,
        -571.0 / 2488320.0,
        -163879.0 / 209018880.0,
    ];
    let mut c = [[0.0; 22]; 6];
    c[0][0] = -1.0 / 3.0;
    let mut j = 1;
    while j < 22 {
        c[0][j] = (j + 2) as f64 * inverse[j + 2];
        j += 1;
    }
    let mut k = 1;
    while k < 6 {
        j = 0;
        while j < 22 - 2 * k {
            c[k][j] = g[k] * c[0][j] + (j + 2) as f64 * c[k - 1][j + 2];
            j += 1;
        }
        k += 1;
    }
    c
}

fn temme(a: f64, delta: f64) -> (f64, bool) {
    const C: [[f64; 22]; 6] = temme_coefficients();
    let deviance = -a * log1pmx(delta);
    let eta = (-2.0 * log1pmx(delta)).sqrt().copysign(delta);
    let mut correction = 0.0;
    for row in C.iter().rev() {
        let coefficient = row
            .iter()
            .rev()
            .fold(0.0_f64, |sum, c| sum.mul_add(eta, *c));
        correction = correction / a + coefficient;
    }
    let z = -std::f64::consts::SQRT_2 * deviance.sqrt();
    let base = pnorm(z, 0.0, 1.0, LOG_TAIL, &mut Flags::default());
    let lower = delta < 0.0;
    if base.is_infinite() {
        return (base, lower);
    }
    let relative = mills_ratio(-z, base) / a.sqrt() * correction;
    (
        base + if lower {
            (-relative).ln_1p()
        } else {
            relative.ln_1p()
        },
        lower,
    )
}

/// Evaluate a gamma quantile with the specified shape and scale.
pub fn qgamma(p: f64, shape: f64, scale: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || shape.is_nan() || scale.is_nan() {
        return p + shape + scale;
    }
    if !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, 0.0, INF) {
        return x;
    }
    if shape < 0.0 || scale <= 0.0 {
        return invalid(flags);
    }
    if shape == 0.0 {
        return 0.0;
    }
    if shape.is_infinite() || scale.is_infinite() {
        return INF;
    }
    let (lp, lq) = probability_logs(p, tail);
    if shape == 1.0 {
        return (-lq) * scale;
    }
    let lower = lp < lq;
    let target = lp.min(lq);
    let power_seed = (lp + lgamma1p(shape)) / shape;
    let normal = standard_quantile(target, Tail { lower, log: true });
    let wilson = 1.0 - 1.0 / (9.0 * shape) + normal / (3.0 * shape.sqrt());
    let seed = if power_seed < -2.0 {
        power_seed
    } else if !lower && -lq > 30.0 * shape {
        (-lq + (shape - 1.0) * (-lq).ln() - lgamma(shape, flags)).ln()
    } else {
        shape.ln() + 3.0 * wilson.max(0.1).ln()
    };
    if power_seed < -700.0 && seed == power_seed {
        return (power_seed + scale.ln()).exp();
    }
    let log_tail = Tail { lower, log: true };
    let z = invert(
        -746.0,
        710.0,
        seed.clamp(-746.0, 710.0),
        target,
        lower,
        |lz| {
            let z = lz.exp();
            let value = pgamma(z, shape, 1.0, log_tail, flags);
            let slope = (gamma_factor(z, lz, shape) - value).exp();
            let slope = if lower { slope } else { -slope };
            (
                value,
                halley_slope(slope, shape - z - slope, value - target),
            )
        },
    );
    if z < -700.0 || z > 700.0 {
        (z + scale.ln()).exp()
    } else {
        z.exp() * scale
    }
}

/// Draw from a gamma distribution using the Ahrens–Dieter GS and GD methods.
pub fn rgamma(rng: &mut Rng, shape: f64, scale: f64, flags: &mut Flags) -> f64 {
    if shape.is_nan() || scale.is_nan() {
        return invalid(flags);
    }
    if shape == 0.0 || scale == 0.0 {
        return 0.0;
    }
    if shape < 0.0 || scale < 0.0 {
        return invalid(flags);
    }
    if shape.is_infinite() || scale.is_infinite() {
        return INF;
    }
    if shape < 1.0 {
        let b = std::f64::consts::E.recip().mul_add(shape, 1.0);
        for _ in 0..100_000 {
            let p = b * rng.unif_rand();
            let (x, limit) = if p < 1.0 {
                let x = (p.ln() / shape).exp();
                (x, x)
            } else {
                let x = -((b - p) / shape).ln();
                (x, (1.0 - shape) * x.ln())
            };
            if rng.exp_rand() >= limit {
                return x * scale;
            }
        }
    } else {
        let v = shape - 0.5;
        let s = v.sqrt();
        let t = rng.norm_rand();
        let x = 0.5_f64.mul_add(t, s);
        if t >= 0.0 {
            return x * x * scale;
        }
        let u = rng.unif_rand();
        if (-12.0_f64).mul_add(s, 32.0_f64.sqrt()) * u <= t * t * t {
            return x * x * scale;
        }
        let q0 = if v >= 64.0 {
            (1.0 / 24.0 - (7.0 / 2880.0 - 31.0 / (40320.0 * v * v)) / (v * v)) / v
        } else {
            v.mul_add(v.ln(), LOG_SQRT_2PI - v) - lgamma(shape, flags)
        };
        let quotient = |t: f64| {
            let r = (0.5 * t) / s;
            let remainder = if r.abs() < 0.25 {
                let series = (3..=32)
                    .rev()
                    .fold(0.0_f64, |sum, n| (-r).mul_add(sum, 1.0 / f64::from(n)));
                r * r * r * series
            } else {
                (0.5 * r).mul_add(r, log1pmx(r))
            };
            (2.0 * remainder).mul_add(v, q0)
        };
        if x > 0.0 && (-u).ln_1p() <= quotient(t) {
            return x * x * scale;
        }
        let (b, sigma, c) = if shape <= 3.686 {
            (
                0.178_f64.mul_add(v, 0.463 + s),
                1.235,
                0.160_f64.mul_add(s, 0.195 / s - 0.079),
            )
        } else if shape <= 13.022 {
            (
                0.0076_f64.mul_add(v, 1.654),
                1.68_f64.mul_add(s.recip(), 0.275),
                0.062_f64.mul_add(s.recip(), 0.024),
            )
        } else {
            (1.77, 0.75, 0.1515 / s)
        };
        for _ in 0..100_000 {
            let e = rng.exp_rand();
            let u = 2.0_f64.mul_add(rng.unif_rand(), -1.0);
            let t = sigma.copysign(u).mul_add(e, b);
            if t >= -0.718_744_837_717_19 {
                let q = quotient(t);
                if q > 0.0 && c * u.abs() <= q.exp_m1() * (-0.5 * t).mul_add(t, e).exp() {
                    let x = 0.5_f64.mul_add(t, s);
                    return x * x * scale;
                }
            }
        }
    }
    flags.set(Flags::NO_CONVERGENCE);
    f64::NAN
}

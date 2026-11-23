//! Closed-form continuous distributions.

use std::f64::consts::PI;

use crate::dist::normal::{dnorm, pnorm, qnorm, rnorm};
use crate::rng::Rng;
use crate::special::aux::{log1pexp, tanpi};

use super::*;

const INF: f64 = f64::INFINITY;

/// Evaluate the uniform density on the closed interval `[min, max]`.
pub fn dunif(x: f64, min: f64, max: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || min.is_nan() || max.is_nan() {
        return x + min + max;
    }
    if min >= max {
        return invalid(flags);
    }
    if x < min || x > max {
        return zero(log);
    }
    if log {
        -(max - min).ln()
    } else {
        (max - min).recip()
    }
}

/// Evaluate a uniform tail probability.
pub fn punif(x: f64, min: f64, max: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || min.is_nan() || max.is_nan() {
        return x + min + max;
    }
    if min > max || !min.is_finite() || !max.is_finite() {
        return invalid(flags);
    }
    if x < min || x >= max {
        return step(x >= max, tail);
    }
    let p = if tail.lower {
        (x - min) / (max - min)
    } else {
        (max - x) / (max - min)
    };
    if tail.log { p.ln() } else { p }
}

/// Evaluate a uniform quantile.
pub fn qunif(p: f64, min: f64, max: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || min.is_nan() || max.is_nan() {
        return p + min + max;
    }
    if min > max || !min.is_finite() || !max.is_finite() || !probability_valid(p, tail) {
        return invalid(flags);
    }
    let p = match (tail.lower, tail.log) {
        (true, true) => p.exp(),
        (true, false) => p,
        (false, true) => -p.exp_m1(),
        (false, false) => complement(p),
    };
    min + p * (max - min)
}

/// Draw from a uniform distribution on `[min, max]`.
pub fn runif(rng: &mut Rng, min: f64, max: f64, flags: &mut Flags) -> f64 {
    if min > max || !min.is_finite() || !max.is_finite() {
        return invalid(flags);
    }
    if min == max {
        return min;
    }
    rng.unif_rand().mul_add(max - min, min)
}

/// Evaluate the exponential density with the R `stats` rate parameter.
pub fn dexp(x: f64, rate: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || rate.is_nan() {
        return x + rate;
    }
    if rate.is_sign_negative() || rate.is_infinite() || x == INF && rate == 0.0 {
        return invalid(flags);
    }
    if x < 0.0 {
        return zero(log);
    }
    let scale = rate.recip();
    if log {
        -x / scale - scale.ln()
    } else {
        (-x / scale).exp() / scale
    }
}

/// Evaluate an exponential tail probability with the specified rate.
pub fn pexp(x: f64, rate: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || rate.is_nan() {
        return x + rate;
    }
    if rate.is_sign_negative() || x == INF && rate == 0.0 {
        return invalid(flags);
    }
    if x <= 0.0 {
        return step(false, tail);
    }
    from_log(-x / rate.recip(), false, tail)
}

/// Evaluate an exponential quantile with the specified rate.
pub fn qexp(p: f64, rate: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || rate.is_nan() {
        return p + rate;
    }
    if rate.is_sign_negative() || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if endpoints(p, tail, 0.0, INF) == Some(0.0) {
        return 0.0;
    }
    -probability_logs(p, tail).1 * rate.recip()
}

/// Draw from an exponential distribution with the specified rate.
pub fn rexp(rng: &mut Rng, rate: f64, flags: &mut Flags) -> f64 {
    let scale = rate.recip();
    if rate < 0.0 || !scale.is_finite() {
        return invalid(flags);
    }
    if scale == 0.0 {
        return 0.0;
    }
    rng.exp_rand() * scale
}

/// Evaluate the lognormal density with log-scale mean and standard deviation.
pub fn dlnorm(x: f64, meanlog: f64, sdlog: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || meanlog.is_nan() || sdlog.is_nan() {
        return x + meanlog + sdlog;
    }
    if sdlog < 0.0 {
        return invalid(flags);
    }
    if sdlog == 0.0 {
        return if x == meanlog.exp() { INF } else { zero(log) };
    }
    if x <= 0.0 {
        return zero(log);
    }
    let density = dnorm(x.ln(), meanlog, sdlog, log, flags);
    if log { density - x.ln() } else { density / x }
}

/// Evaluate a lognormal tail probability.
pub fn plnorm(x: f64, meanlog: f64, sdlog: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || meanlog.is_nan() || sdlog.is_nan() {
        return x + meanlog + sdlog;
    }
    if sdlog < 0.0 {
        return invalid(flags);
    }
    if x <= 0.0 {
        return step(false, tail);
    }
    pnorm(x.ln(), meanlog, sdlog, tail, flags)
}

/// Evaluate a lognormal quantile.
pub fn qlnorm(p: f64, meanlog: f64, sdlog: f64, tail: Tail, flags: &mut Flags) -> f64 {
    qnorm(p, meanlog, sdlog, tail, flags).exp()
}

/// Draw from a lognormal distribution.
pub fn rlnorm(rng: &mut Rng, meanlog: f64, sdlog: f64, flags: &mut Flags) -> f64 {
    rnorm(rng, meanlog, sdlog, flags).exp()
}

/// Evaluate the logistic density.
pub fn dlogis(x: f64, location: f64, scale: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || location.is_nan() || scale.is_nan() {
        return x + location + scale;
    }
    if scale <= 0.0 {
        return invalid(flags);
    }
    let z = ((x - location) / scale).abs();
    if z.is_nan() {
        return invalid(flags);
    }
    let e = (-z).exp();
    if log {
        -z - 2.0 * e.ln_1p() - scale.ln()
    } else {
        e / (scale * (1.0 + e).powi(2))
    }
}

/// Evaluate a logistic tail probability.
pub fn plogis(x: f64, location: f64, scale: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || location.is_nan() || scale.is_nan() {
        return x + location + scale;
    }
    if scale <= 0.0 {
        return invalid(flags);
    }
    let z = (x - location) / scale;
    if z.is_nan() {
        return invalid(flags);
    }
    let z = if tail.lower { -z } else { z };
    if tail.log {
        -log1pexp(z)
    } else {
        1.0 / (1.0 + z.exp())
    }
}

/// Evaluate a logistic quantile.
pub fn qlogis(p: f64, location: f64, scale: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || location.is_nan() || scale.is_nan() {
        return p + location + scale;
    }
    if !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, -INF, INF) {
        return x;
    }
    if scale < 0.0 {
        return invalid(flags);
    }
    let (lp, lq) = probability_logs(p, tail);
    let value = scale.mul_add(lp - lq, location);
    if value.is_nan() {
        invalid(flags)
    } else {
        value
    }
}

/// Draw from a logistic distribution.
pub fn rlogis(rng: &mut Rng, location: f64, scale: f64, flags: &mut Flags) -> f64 {
    if location.is_nan() || !scale.is_finite() {
        return invalid(flags);
    }
    if scale == 0.0 || location.is_infinite() {
        return location;
    }
    let u = rng.unif_rand();
    scale.mul_add((u / complement(u)).ln(), location)
}

/// Evaluate the Cauchy density.
pub fn dcauchy(x: f64, location: f64, scale: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || location.is_nan() || scale.is_nan() {
        return x + location + scale;
    }
    if scale <= 0.0 {
        return invalid(flags);
    }
    let z = (x - location) / scale;
    if z.is_nan() {
        return invalid(flags);
    }
    if log {
        let shape = if z.abs() > 1e150 {
            2.0 * z.abs().ln()
        } else {
            (z * z).ln_1p()
        };
        -(PI.ln() + scale.ln() + shape)
    } else if z.abs() > 1e150 || !(PI * scale).is_normal() {
        dcauchy(x, location, scale, true, flags).exp()
    } else {
        1.0 / (PI * scale * (1.0 + z * z))
    }
}

/// Evaluate a Cauchy tail probability.
pub fn pcauchy(x: f64, location: f64, scale: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || location.is_nan() || scale.is_nan() {
        return x + location + scale;
    }
    if scale <= 0.0 {
        return invalid(flags);
    }
    let z = (x - location) / scale;
    if z.is_nan() {
        return invalid(flags);
    }
    let z = if tail.lower { z } else { -z };
    if z.abs() <= 1.0 {
        let p = 0.5 + z.atan() / PI;
        return if tail.log { p.ln() } else { p };
    }
    let small = z.abs().recip().atan() / PI;
    if z < 0.0 {
        if tail.log { small.ln() } else { small }
    } else if tail.log {
        (-small).ln_1p()
    } else {
        1.0 - small
    }
}

/// Evaluate a Cauchy quantile.
pub fn qcauchy(p: f64, location: f64, scale: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || location.is_nan() || scale.is_nan() {
        return p + location + scale;
    }
    if scale < 0.0 || !scale.is_finite() || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if scale == 0.0 {
        return location;
    }
    if let Some(x) = endpoints(p, tail, -INF, INF) {
        return location + x;
    }
    let (lp, lq) = probability_logs(p, tail);
    let lower = lp < lq;
    let small = if tail.log {
        lp.min(lq).exp()
    } else {
        p.min(complement(p))
    };
    let delta = if small == 0.5 {
        0.0
    } else if small < f64::MIN_POSITIVE {
        (scale.ln() - PI.ln() - lp.min(lq)).exp()
    } else if small > 0.25 {
        scale * tanpi(0.5 - small)
    } else {
        scale / tanpi(small)
    };
    if lower {
        location - delta
    } else {
        location + delta
    }
}

/// Draw from a Cauchy distribution.
pub fn rcauchy(rng: &mut Rng, location: f64, scale: f64, flags: &mut Flags) -> f64 {
    if location.is_nan() || scale < 0.0 || !scale.is_finite() {
        return invalid(flags);
    }
    if scale == 0.0 || location.is_infinite() {
        return location;
    }
    scale.mul_add(tanpi(rng.unif_rand()), location)
}

/// Evaluate the Weibull density.
pub fn dweibull(x: f64, shape: f64, scale: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || shape.is_nan() || scale.is_nan() {
        return x + shape + scale;
    }
    if shape <= 0.0 || scale <= 0.0 {
        return invalid(flags);
    }
    if x < 0.0 || x == INF {
        return zero(log);
    }
    if x == 0.0 && shape < 1.0 {
        return INF;
    }
    let z = x / scale;
    let power = z.powf(shape);
    if power.is_infinite() {
        return zero(log);
    }
    if shape.is_infinite() && z != 1.0 {
        return invalid(flags);
    }
    if log {
        let slope = if shape == 1.0 || z == 1.0 {
            0.0
        } else {
            (shape - 1.0) * z.ln()
        };
        shape.ln() - scale.ln() + slope - power
    } else {
        shape / scale * z.powf(shape - 1.0) * (-power).exp()
    }
}

/// Evaluate a Weibull tail probability.
pub fn pweibull(x: f64, shape: f64, scale: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || shape.is_nan() || scale.is_nan() {
        return x + shape + scale;
    }
    if shape <= 0.0 || scale <= 0.0 {
        return invalid(flags);
    }
    if x <= 0.0 {
        return step(false, tail);
    }
    let power = (x / scale).powf(shape);
    if tail.lower && tail.log && power < f64::MIN_POSITIVE {
        return shape * (x.ln() - scale.ln());
    }
    from_log(-power, false, tail)
}

/// Evaluate a Weibull quantile.
pub fn qweibull(p: f64, shape: f64, scale: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || shape.is_nan() || scale.is_nan() {
        return p + shape + scale;
    }
    if shape <= 0.0 || scale <= 0.0 || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, 0.0, INF) {
        return x;
    }
    let (lp, lq) = probability_logs(p, tail);
    if lq == 0.0 {
        return (lp / shape + scale.ln()).exp();
    }
    scale * (-lq).powf(shape.recip())
}

/// Draw from a Weibull distribution.
pub fn rweibull(rng: &mut Rng, shape: f64, scale: f64, flags: &mut Flags) -> f64 {
    if !shape.is_finite() || shape <= 0.0 || !scale.is_finite() || scale < 0.0 {
        return invalid(flags);
    }
    if scale == 0.0 {
        return 0.0;
    }
    scale * (-rng.unif_rand().ln()).powf(shape.recip())
}

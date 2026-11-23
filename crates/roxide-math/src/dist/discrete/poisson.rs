//! Poisson masses, gamma tails, and the modified normal random generator.

use super::*;
use crate::dist::continuous::pgamma;
use crate::rng::Rng;
use crate::special::aux::{ebd0, stirlerr};

pub(super) fn poisson_log(x: f64, lambda: f64) -> f64 {
    if x == 0.0 {
        return -lambda;
    }
    if lambda == 0.0 || lambda == INF {
        return -INF;
    }
    let (deviance, residual) = ebd0(x, lambda);
    -deviance - (residual + stirlerr(x) + LOG_SQRT_2PI + 0.5 * x.ln())
}

/// Return the Poisson mass at `x`, or its logarithm.
pub fn dpois(x: f64, lambda: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || lambda.is_nan() {
        return x + lambda;
    }
    if lambda < 0.0 {
        return invalid(flags);
    }
    match observation(x, true, flags) {
        Some(x) => on_scale(poisson_log(x, lambda), log),
        None => zero(log),
    }
}

/// Return the requested Poisson tail probability at `q`.
pub fn ppois(q: f64, lambda: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if q.is_nan() || lambda.is_nan() {
        return q + lambda;
    }
    if lambda < 0.0 {
        return invalid(flags);
    }
    if q < 0.0 {
        return step(false, tail);
    }
    let x = count_floor(q);
    if x == INF || lambda == 0.0 {
        return step(true, tail);
    }
    pgamma(
        lambda,
        x + 1.0,
        1.0,
        Tail {
            lower: !tail.lower,
            log: tail.log,
        },
        flags,
    )
}

/// Return the smallest Poisson count at the requested probability.
pub fn qpois(p: f64, lambda: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || lambda.is_nan() {
        return p + lambda;
    }
    if lambda < 0.0 || !lambda.is_finite() || !valid_probability(p, tail) {
        return invalid(flags);
    }
    if lambda == 0.0 {
        return 0.0;
    }
    if let Some(x) = endpoints(p, tail, 0.0, INF) {
        return x;
    }
    let sd = lambda.sqrt();
    let guess = cornish_fisher(p, tail, lambda, sd, 1.0 / sd, flags);
    quantile_search(p, tail, guess, INF, |x, tail| ppois(x, lambda, tail, flags))
}

/// Draw a Poisson count from `rng`.
pub fn rpois(rng: &mut Rng, lambda: f64, flags: &mut Flags) -> f64 {
    if lambda < 0.0 || !lambda.is_finite() {
        return invalid(flags);
    }
    if lambda == 0.0 {
        return 0.0;
    }
    if lambda < 10.0 {
        let initial = (-lambda).exp();
        loop {
            let u = rng.unif_rand();
            let mut mass = initial;
            let mut total = mass;
            if u <= total {
                return 0.0;
            }
            for j in 1..=35 {
                mass *= lambda / f64::from(j);
                total += mass;
                if u <= total {
                    return f64::from(j);
                }
            }
        }
    }
    modified_normal(rng, lambda)
}

fn modified_normal(rng: &mut Rng, lambda: f64) -> f64 {
    // Ahrens and Dieter (1982), algorithm PD. Use the Poisson mass directly.
    let sd = lambda.sqrt();
    let normal_count = (lambda + sd * rng.norm_rand()).floor();
    let mut quotient_uniform = None;
    if normal_count >= 0.0 {
        if normal_count >= (lambda - 1.1484).floor() {
            return normal_count;
        }
        let u = rng.unif_rand();
        if 6.0 * lambda * lambda * u >= (lambda - normal_count).powi(3) {
            return normal_count;
        }
        quotient_uniform = Some(u);
    }
    let b1 = 1.0 / (24.0 * lambda);
    let b2 = 0.3 * b1 * b1;
    let c3 = b1 * b2 / 7.0;
    let c2 = b2 - 15.0 * c3;
    let c1 = b1 - 6.0 * b2 + 45.0 * c3;
    let c0 = 1.0 - b1 + 3.0 * b2 - 15.0 * c3;
    let normal_log = |x: f64| {
        let z = (x - lambda + 0.5) / sd;
        let square = z * z;
        (((c3 * square + c2) * square + c1) * square + c0).ln()
            - LOG_SQRT_2PI
            - sd.ln()
            - 0.5 * square
    };
    if let Some(u) = quotient_uniform
        && (-u).ln_1p() <= poisson_log(normal_count, lambda) - normal_log(normal_count)
    {
        return normal_count;
    }
    loop {
        let e = rng.exp_rand();
        let u = 2.0 * rng.unif_rand() - 1.0;
        let t = 1.8 + e.copysign(u);
        if t <= -0.6744 {
            continue;
        }
        let x = (lambda + sd * t).floor();
        let pmf = (poisson_log(x, lambda) + e).exp();
        let normal = (normal_log(x) + e).exp();
        if (0.1069 / lambda) * u.abs() <= pmf - normal {
            return x;
        }
    }
}

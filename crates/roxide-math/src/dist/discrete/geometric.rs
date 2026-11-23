//! Geometric masses, tails, and exponential–Poisson random draws.

use super::*;
use crate::dist::continuous::rexp;
use crate::rng::Rng;

fn valid(prob: f64) -> bool {
    prob > 0.0 && prob <= 1.0
}

/// Return the geometric mass at `x`, counting failures before the first success.
pub fn dgeom(x: f64, prob: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || prob.is_nan() {
        return x + prob;
    }
    if !valid(prob) {
        return invalid(flags);
    }
    match observation(x, true, flags) {
        Some(0.0) => on_scale(prob.ln(), log),
        Some(x) => on_scale(prob.ln() + x * (-prob).ln_1p(), log),
        None => zero(log),
    }
}

/// Return the requested geometric tail probability at `q`.
pub fn pgeom(q: f64, prob: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if q.is_nan() || prob.is_nan() {
        return q + prob;
    }
    if !valid(prob) {
        return invalid(flags);
    }
    if q < 0.0 {
        return step(false, tail);
    }
    let q = count_floor(q);
    from_log((q + 1.0) * (-prob).ln_1p(), false, tail)
}

/// Return the smallest geometric count at the requested probability.
pub fn qgeom(p: f64, prob: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || prob.is_nan() {
        return p + prob;
    }
    if !valid(prob) || !valid_probability(p, tail) {
        return invalid(flags);
    }
    if prob == 1.0 {
        return 0.0;
    }
    if let Some(x) = endpoints(p, tail, 0.0, INF) {
        return x;
    }
    (log_survival(p, tail) / (-prob).ln_1p() - 1.0 - 1e-12)
        .ceil()
        .max(0.0)
}

/// Draw a geometric count from `rng`.
pub fn rgeom(rng: &mut Rng, prob: f64, flags: &mut Flags) -> f64 {
    if !valid(prob) {
        return invalid(flags);
    }
    let mean = ((1.0 - prob) / prob) * rexp(rng, 1.0, flags);
    rpois(rng, mean, flags)
}

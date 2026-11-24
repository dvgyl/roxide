//! Binomial mass, beta tails, and the BTPE random generator.

use super::*;
use crate::dist::continuous::pbeta_raw;
use crate::rng::Rng;
use crate::special::aux::{ebd0, stirlerr};

pub(super) fn binomial_log(x: f64, n: f64, p: f64, q: f64) -> f64 {
    if p == 0.0 {
        return if x == 0.0 { 0.0 } else { -INF };
    }
    if q == 0.0 {
        return if x == n { 0.0 } else { -INF };
    }
    if x == 0.0 {
        return n * if p < 0.5 { (-p).ln_1p() } else { q.ln() };
    }
    if x == n {
        return n * if q < 0.5 { (-q).ln_1p() } else { p.ln() };
    }
    let y = n - x;
    let small = x.min(y);
    let (dx, ex) = ebd0(x, n * p);
    let (dy, ey) = ebd0(y, n * q);
    let correction = stirlerr(n)
        - stirlerr(x)
        - stirlerr(y)
        - ex
        - ey
        - LOG_SQRT_2PI
        - 0.5 * (small.ln() + (-small / n).ln_1p());
    (correction - dx) - dy
}

fn valid(size: f64, prob: f64) -> bool {
    size >= 0.0 && integral(size) && (0.0..=1.0).contains(&prob)
}

/// Return the binomial mass at `x`, or its logarithm.
pub fn dbinom(x: f64, size: f64, prob: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || size.is_nan() || prob.is_nan() {
        return x + size + prob;
    }
    if size == INF && (0.0..=1.0).contains(&prob) {
        return if observation(x, true, flags) == Some(0.0) && prob == 0.0 {
            on_scale(0.0, log)
        } else {
            zero(log)
        };
    }
    if !valid(size, prob) {
        return invalid(flags);
    }
    let size = size.round_ties_even();
    match observation(x, true, flags) {
        Some(x) if x <= size => on_scale(binomial_log(x, size, prob, 1.0 - prob), log),
        _ => zero(log),
    }
}

/// Return the requested binomial tail probability at `q`.
pub fn pbinom(q: f64, size: f64, prob: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if q.is_nan() || size.is_nan() || prob.is_nan() {
        return q + size + prob;
    }
    if !valid(size, prob) {
        if size.is_finite() && size >= 0.0 && !integral(size) {
            flags.set(Flags::NONINT_X);
        }
        return invalid(flags);
    }
    let size = size.round_ties_even();
    if q < 0.0 {
        return step(false, tail);
    }
    let q = count_floor(q);
    if q >= size {
        return step(true, tail);
    }
    pbeta_raw(
        prob,
        1.0 - prob,
        q + 1.0,
        size - q,
        Tail {
            lower: !tail.lower,
            log: tail.log,
        },
        flags,
    )
}

/// Return the smallest binomial count at the requested probability.
pub fn qbinom(p: f64, size: f64, prob: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || size.is_nan() || prob.is_nan() {
        return p + size + prob;
    }
    let size = size.round_ties_even();
    if !size.is_finite()
        || size < 0.0
        || !(0.0..=1.0).contains(&prob)
        || !probability_valid(p, tail)
    {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, 0.0, size) {
        return x;
    }
    if size == 0.0 || prob == 0.0 || prob == 1.0 {
        return size * prob;
    }
    let sd = (size * prob * (1.0 - prob)).sqrt();
    let guess = cornish_fisher(p, tail, size * prob, sd, (1.0 - 2.0 * prob) / sd, flags);
    quantile_search(p, tail, guess, size, |x, tail| {
        pbinom(x, size, prob, tail, flags)
    })
}

/// Draw a binomial count from `rng`.
pub fn rbinom(rng: &mut Rng, size: f64, prob: f64, flags: &mut Flags) -> f64 {
    if !valid(size, prob) || size != size.round_ties_even() {
        return invalid(flags);
    }
    if size == 0.0 || prob == 0.0 || prob == 1.0 {
        return size * prob;
    }
    if size >= INT_MAX {
        return qbinom(rng.unif_rand(), size, prob, UPPER, flags);
    }
    let p = prob.min(1.0 - prob);
    let x = if size * p < 30.0 {
        invert(rng, size, p)
    } else {
        btpe(rng, size, p)
    };
    if x.is_nan() {
        flags.set(Flags::NO_CONVERGENCE);
    }
    if prob > 0.5 { size - x } else { x }
}

fn invert(rng: &mut Rng, n: f64, p: f64) -> f64 {
    let q = 1.0 - p;
    let initial = (n * (-p).ln_1p()).exp();
    for _ in 0..100_000 {
        let mut u = rng.unif_rand();
        let mut mass = initial;
        for j in 0..=(n as u32).min(111) {
            if u < mass {
                return f64::from(j);
            }
            u -= mass;
            mass *= ((n - f64::from(j)) / f64::from(j + 1)) * (p / q);
        }
    }
    f64::NAN
}

fn btpe(rng: &mut Rng, n: f64, p: f64) -> f64 {
    // Kachitvichyanukul and Schmeiser (1988), triangle and three rejection regions.
    let q = 1.0 - p;
    let center = (n + 1.0) * p;
    let mode = center.floor();
    let middle = mode + 0.5;
    let width = (2.195 * (n * p * q).sqrt() - 4.6 * q).floor() + 0.5;
    let left = middle - width;
    let right = middle + width;
    let height = 0.134 + 20.5 / (15.3 + mode);
    let slope_left = (center - left) / (center - left * p);
    let slope_right = (right - center) / (right * q);
    let rate_left = slope_left * (1.0 + 0.5 * slope_left);
    let rate_right = slope_right * (1.0 + 0.5 * slope_right);
    let parallelogram = width * (1.0 + 2.0 * height);
    let left_tail = parallelogram + height / rate_left;
    let area = left_tail + height / rate_right;
    let peak = binomial_log(mode, n, p, q);
    for _ in 0..100_000 {
        let u = area * rng.unif_rand();
        let v = rng.unif_rand();
        if u <= width {
            return (middle - width * v + u).floor();
        }
        let (x, height_at_x) = if u <= parallelogram {
            let x = left + (u - width) / height;
            (x.floor(), height * v + 1.0 - (middle - x).abs() / width)
        } else if u <= left_tail {
            (
                (left + v.ln() / rate_left).floor(),
                v * (u - parallelogram) * rate_left,
            )
        } else {
            (
                (right - v.ln() / rate_right).floor(),
                v * (u - left_tail) * rate_right,
            )
        };
        if (0.0..=n).contains(&x)
            && height_at_x <= 1.0
            && height_at_x.ln() <= binomial_log(x, n, p, q) - peak
        {
            return x;
        }
    }
    f64::NAN
}

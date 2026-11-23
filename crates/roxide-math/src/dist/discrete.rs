//! Masses, tail probabilities, quantiles, and random draws for discrete distributions.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod binomial;
mod geometric;
mod hypergeometric;
mod multinomial;
mod negative_binomial;
mod poisson;
mod rank;

pub use binomial::{dbinom, pbinom, qbinom, rbinom};
pub use geometric::{dgeom, pgeom, qgeom, rgeom};
pub use hypergeometric::{dhyper, phyper, qhyper, rhyper};
pub use multinomial::{dmultinom, rmultinom};
pub use negative_binomial::{
    dnbinom, dnbinom_mu, pnbinom, pnbinom_mu, qnbinom, qnbinom_mu, rnbinom, rnbinom_mu,
};
pub use poisson::{dpois, ppois, qpois, rpois};
pub use rank::{dsignrank, dwilcox, psignrank, pwilcox, qsignrank, qwilcox, rsignrank, rwilcox};

use crate::core::{Flags, Tail};
use crate::dist::normal::qnorm;
use crate::special::aux::log1mexp;

const INF: f64 = f64::INFINITY;
const LOG_SQRT_2PI: f64 = 0.918_938_533_204_672_8;
const INT_MAX: f64 = i32::MAX as f64;
const UPPER: Tail = Tail {
    lower: false,
    log: false,
};

fn invalid(flags: &mut Flags) -> f64 {
    flags.set(Flags::NAN_PRODUCED);
    f64::NAN
}

fn on_scale(lp: f64, log: bool) -> f64 {
    if log { lp } else { lp.exp() }
}

fn zero(log: bool) -> f64 {
    if log { -INF } else { 0.0 }
}

fn step(at_or_above: bool, tail: Tail) -> f64 {
    if at_or_above == tail.lower {
        on_scale(0.0, tail.log)
    } else {
        zero(tail.log)
    }
}

fn integral(x: f64) -> bool {
    x.is_finite() && (x - x.round_ties_even()).abs() <= 1e-9 * x.abs().max(1.0)
}

fn observation(x: f64, warn: bool, flags: &mut Flags) -> Option<f64> {
    if x.is_finite() && !integral(x) {
        if warn {
            flags.set(Flags::NONINT_X);
        }
        None
    } else if x >= 0.0 && x.is_finite() {
        Some(x.round_ties_even())
    } else {
        None
    }
}

fn count_floor(x: f64) -> f64 {
    (x + 1e-7).floor()
}

fn valid_probability(p: f64, tail: Tail) -> bool {
    if tail.log {
        p <= 0.0
    } else {
        (0.0..=1.0).contains(&p)
    }
}

fn endpoints(p: f64, tail: Tail, min: f64, max: f64) -> Option<f64> {
    if p == zero(tail.log) {
        Some(if tail.lower { min } else { max })
    } else if p == on_scale(0.0, tail.log) {
        Some(if tail.lower { max } else { min })
    } else {
        None
    }
}

fn lower_probability(p: f64, tail: Tail) -> f64 {
    match (tail.lower, tail.log) {
        (true, false) => p,
        (true, true) => p.exp(),
        (false, false) => 0.5 - p + 0.5,
        (false, true) => -p.exp_m1(),
    }
}

fn log_survival(p: f64, tail: Tail) -> f64 {
    match (tail.lower, tail.log) {
        (true, false) => (-p).ln_1p(),
        (true, true) => log1mexp(-p),
        (false, false) => p.ln(),
        (false, true) => p,
    }
}

fn from_log(lp: f64, lower: bool, tail: Tail) -> f64 {
    let lp = lp.clamp(-INF, 0.0);
    match (lower == tail.lower, tail.log) {
        (true, _) => on_scale(lp, tail.log),
        (false, true) => log1mexp(-lp),
        (false, false) => -lp.exp_m1(),
    }
}

fn cornish_fisher(p: f64, tail: Tail, mean: f64, sd: f64, skew: f64, flags: &mut Flags) -> f64 {
    let z = qnorm(p, 0.0, 1.0, tail, flags);
    (mean + sd * (z + skew * (z * z - 1.0) / 6.0)).round_ties_even()
}

fn quantile_search(
    p: f64,
    tail: Tail,
    guess: f64,
    max: f64,
    mut cdf: impl FnMut(f64, Tail) -> f64,
) -> f64 {
    let target = match (tail.lower, tail.log) {
        (true, false) => p * (1.0 - 8.0 * f64::EPSILON),
        (false, false) => p * (1.0 + 8.0 * f64::EPSILON),
        (true, true) => p * (1.0 + 2.0 * f64::EPSILON),
        (false, true) => p * (1.0 - 2.0 * f64::EPSILON),
    };
    // Upper-tail comparisons are strict. Keep the requested scale at jumps.
    first_count(0.0, max, guess, |x| {
        let value = cdf(x, tail);
        (!value.is_nan()).then_some(if tail.lower {
            value >= target
        } else {
            value < target
        })
    })
    .unwrap_or(f64::NAN)
}

fn first_count(
    min: f64,
    max: f64,
    guess: f64,
    mut reached: impl FnMut(f64) -> Option<bool>,
) -> Option<f64> {
    let mut low = min - 1.0;
    let mut high = if guess.is_finite() {
        guess.clamp(min, max)
    } else {
        min
    };
    if !reached(high)? {
        loop {
            low = high;
            if low == max {
                return Some(max);
            }
            high = (2.0 * low + 1.0).max(low + 1.0).min(max);
            if high == INF || reached(high)? {
                break;
            }
        }
    }
    if high == INF {
        return Some(high);
    }
    loop {
        let mid = low.midpoint(high).floor();
        if mid <= low || mid >= high {
            return Some(high);
        }
        if reached(mid)? {
            high = mid;
        } else {
            low = mid;
        }
    }
}

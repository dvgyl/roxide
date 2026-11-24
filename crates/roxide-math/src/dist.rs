//! Pure scalar distribution kernels (`d/p/q/r*`) split into disjoint subfamilies.

pub mod continuous;
pub mod discrete;
pub mod normal;

use crate::core::{Flags, Tail};
use crate::special::aux::log1mexp;

fn invalid(flags: &mut Flags) -> f64 {
    flags.set(Flags::NAN_PRODUCED);
    f64::NAN
}

fn on_scale(log_value: f64, log: bool) -> f64 {
    if log { log_value } else { log_value.exp() }
}

fn zero(log: bool) -> f64 {
    on_scale(f64::NEG_INFINITY, log)
}

fn step(at_or_above: bool, tail: Tail) -> f64 {
    if at_or_above == tail.lower {
        on_scale(0.0, tail.log)
    } else {
        zero(tail.log)
    }
}

fn probability_valid(p: f64, tail: Tail) -> bool {
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

fn from_log(lp: f64, lower: bool, tail: Tail) -> f64 {
    match (lower == tail.lower, tail.log) {
        (true, _) => on_scale(lp, tail.log),
        (false, true) => log1mexp(-lp),
        (false, false) => -lp.exp_m1(),
    }
}

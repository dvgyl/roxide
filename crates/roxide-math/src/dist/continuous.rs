//! Scalar kernels for continuous distributions.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod beta;
mod elementary;
mod gamma;
mod noncentral;
mod quadrature;
mod ratios;
mod tukey;

pub use beta::{dbeta, pbeta, pbeta_raw, qbeta, rbeta};
pub use elementary::*;
pub use gamma::{dgamma, pgamma, qgamma, rgamma};
pub use noncentral::*;
pub use ratios::*;
pub use tukey::{ptukey, qtukey};

use super::{endpoints, from_log, invalid, on_scale, probability_valid, step, zero};
use crate::core::{Flags, Tail};
use crate::special::aux::log1mexp;

const LOG_TAIL: Tail = Tail {
    lower: true,
    log: true,
};
const LOG_UPPER: Tail = Tail {
    lower: false,
    log: true,
};
const LOG_SQRT_2PI: f64 = 0.918_938_533_204_672_8;

fn complement(p: f64) -> f64 {
    0.5 - p + 0.5
}

fn probability_logs(p: f64, tail: Tail) -> (f64, f64) {
    let (lp, lq) = if tail.log {
        (p, log1mexp(-p))
    } else {
        (p.ln(), (-p).ln_1p())
    };
    if tail.lower { (lp, lq) } else { (lq, lp) }
}

fn log_add(a: f64, b: f64) -> f64 {
    let high = a.max(b);
    if high.is_infinite() {
        high
    } else {
        high + (a.min(b) - high).exp().ln_1p()
    }
}

fn halley_slope(slope: f64, curvature_ratio: f64, residual: f64) -> f64 {
    let corrected = (-0.5 * residual).mul_add(curvature_ratio, slope);
    if (0.5..=1.5).contains(&(corrected / slope)) {
        corrected
    } else {
        slope
    }
}

// Invert a monotone log tail. The callback returns its value and slope.
fn invert(
    mut low: f64,
    mut high: f64,
    mut x: f64,
    target: f64,
    lower: bool,
    mut evaluate: impl FnMut(f64) -> (f64, f64),
) -> f64 {
    for _ in 0..128 {
        let (value, slope) = evaluate(x);
        let residual = value - target;
        if residual.abs() <= 2.0 * f64::EPSILON * target.abs() {
            break;
        }
        if (residual < 0.0) == lower {
            low = x;
        } else {
            high = x;
        }
        let mut next = x - residual / slope;
        if next == x {
            break;
        }
        if !(low < next && next < high) {
            next = low.midpoint(high);
        }
        if next == low || next == high {
            break;
        }
        x = next;
    }
    x
}

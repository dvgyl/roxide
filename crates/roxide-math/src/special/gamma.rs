//! The gamma function and its logarithm.

use std::f64::consts::PI;

use crate::Flags;

use super::aux::{lgamma1p, sinpi};
use super::domain;

pub(super) const LN_SQRT_2PI: f64 = 0.918_938_533_204_672_8;

/// Return `Gamma(x)` and flag domain errors at nonpositive integer poles.
pub fn gamma(x: f64, flags: &mut Flags) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    if x <= 0.0 && x.fract() == 0.0 || x == f64::NEG_INFINITY {
        return domain(flags);
    }
    if x > 0.0 && x < 2.247_436_222_559_854_5e-308 {
        flags.set(Flags::UNDERFLOW);
        return f64::INFINITY;
    }
    if x > 171.614_478_871_822_97 || x < -170.567_497_272_661_2 {
        return if x > 0.0 { f64::INFINITY } else { 0.0 };
    }
    if x > 50.0 {
        let log = (x - 0.5) * (x.ln() - 1.0) + LN_SQRT_2PI - 0.5;
        return (log + stirling_correction(x)).exp();
    }
    if x > 0.0 {
        return gamma_positive(x);
    }
    let sin = sinpi(x);
    check_near_pole(x, flags);
    if x > -0.5 {
        return lgamma1p(x).exp() / x;
    }
    // Divide in this order so the denominator does not overflow near -170.
    ((PI / -x) / gamma_positive(-x)) / sin
}

/// Return `ln(abs(Gamma(x)))`, with positive infinity at a pole.
pub fn lgamma(x: f64, flags: &mut Flags) -> f64 {
    lgamma_sign(x, flags).0
}

/// Return `ln(abs(Gamma(x)))` and the sign of `Gamma(x)`.
///
/// At poles, return positive infinity. The sign at a pole is set to one.
pub fn lgamma_sign(x: f64, flags: &mut Flags) -> (f64, i32) {
    if x.is_nan() {
        return (x, 1);
    }
    if x.is_infinite() || x <= 0.0 && x.fract() == 0.0 {
        return (f64::INFINITY, 1);
    }
    if x > 0.0 {
        return (lgamma_positive(x), 1);
    }
    check_near_pole(x, flags);
    let sin = sinpi(x);
    let log = if x > -0.5 {
        lgamma1p(x) - (-x).ln()
    } else {
        PI.ln() - (-x).ln() - sin.abs().ln() - lgamma_positive(-x)
    };
    (log, if sin.is_sign_negative() { -1 } else { 1 })
}

fn check_near_pole(x: f64, flags: &mut Flags) {
    if x < -10.0 && (x - x.round()).abs() < f64::EPSILON.sqrt() * -x {
        flags.set(Flags::PRECISION);
    }
}

pub(super) fn gamma_positive(mut x: f64) -> f64 {
    if x > 171.614_478_871_822_97 {
        return f64::INFINITY;
    }
    if x < 0.5 {
        return lgamma1p(x).exp() / x;
    }
    let mut product = 1.0;
    while x > 1.5 {
        x -= 1.0;
        product *= x;
    }
    product * lgamma1p(x - 1.0).exp()
}

pub(super) fn lgamma_positive(x: f64) -> f64 {
    if x >= 8.0 {
        return (x - 0.5).mul_add(x.ln() - 1.0, LN_SQRT_2PI - 0.5 + stirling_correction(x));
    }
    if x < 0.5 {
        return lgamma1p(x) - x.ln();
    }
    let steps = (x - 0.5) as u32;
    let reduced = x - f64::from(steps);
    let mut product = 1.0;
    for step in 0..steps {
        product *= reduced + f64::from(step);
    }
    lgamma1p(reduced - 1.0) + product.ln()
}

// DLMF 5.11.1. At x >= 8 the retained Bernoulli terms give double precision.
pub(super) fn stirling_correction(x: f64) -> f64 {
    const COEFFS: [f64; 10] = [
        1.0 / 12.0,
        -1.0 / 360.0,
        1.0 / 1260.0,
        -1.0 / 1680.0,
        1.0 / 1188.0,
        -691.0 / 360_360.0,
        1.0 / 156.0,
        -3617.0 / 122_400.0,
        43_867.0 / 244_188.0,
        -174_611.0 / 125_400.0,
    ];
    let reciprocal = 1.0 / x;
    let square = reciprocal * reciprocal;
    reciprocal
        * COEFFS
            .iter()
            .rev()
            .fold(0.0_f64, |sum, &coef| sum.mul_add(square, coef))
}

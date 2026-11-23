//! The logarithmic derivatives of the gamma function.

use std::f64::consts::PI;

use crate::Flags;

use super::domain;
use super::gamma::lgamma_positive;

const SCALED_EXPONENT_LIMIT: f64 = 700.617_442_068_873;

/// Return the first logarithmic derivative of the gamma function.
pub fn digamma(x: f64, flags: &mut Flags) -> f64 {
    psigamma(x, 0, flags)
}

/// Return the second logarithmic derivative of the gamma function.
pub fn trigamma(x: f64, flags: &mut Flags) -> f64 {
    psigamma(x, 1, flags)
}

/// Return the polygamma function of order `deriv`.
///
/// Orders range from zero to 100. At negative nonintegers, orders above five
/// return NaN.
pub fn psigamma(x: f64, deriv: i32, flags: &mut Flags) -> f64 {
    if x.is_nan() {
        return x;
    }
    if !(0..=100).contains(&deriv) {
        return domain(flags);
    }
    if x == f64::INFINITY {
        return if deriv == 0 { x } else { 0.0 };
    }
    if x <= 0.0 {
        if x == f64::NEG_INFINITY || x.fract() == 0.0 {
            return if deriv % 2 == 0 {
                domain(flags)
            } else {
                f64::INFINITY
            };
        }
        if deriv > 5 {
            return domain(flags);
        }

        // DLMF 5.15.6. The derivatives of cot(pi*x) are polynomials in cot(pi*x).
        let (sine, cosine) = (PI * x).sin_cos();
        let cot = cosine / sine;
        let square = cot * cot;
        let sec = 1.0 + square;
        let correction = match deriv {
            0 => cot,
            1 => -sec,
            2 => 2.0 * cot * sec,
            3 => -2.0 * sec * (1.0 + 3.0 * square),
            4 => 8.0 * cot * sec * (2.0 + 3.0 * square),
            _ => -8.0 * sec * (2.0 + square * (15.0 + 15.0 * square)),
        };
        let reflected = positive(1.0 - x, deriv);
        let sign = if deriv % 2 == 0 { 1.0 } else { -1.0 };
        return (-PI.powi(deriv + 1)).mul_add(correction, sign * reflected);
    }
    if -x.ln() * f64::from(deriv + 1) > SCALED_EXPONENT_LIMIT {
        return domain(flags);
    }
    positive(x, deriv)
}

const BERNOULLI_OVER_FACTORIAL: [f64; 9] = [
    1.0 / 12.0,
    -1.0 / 720.0,
    1.0 / 30_240.0,
    -1.0 / 1_209_600.0,
    1.0 / 47_900_160.0,
    -691.0 / 1_307_674_368_000.0,
    1.0 / 74_724_249_600.0,
    -3617.0 / 10_670_622_842_880_000.0,
    43_867.0 / 5_109_094_217_170_944_000.0,
];

fn positive(x: f64, deriv: i32) -> f64 {
    if deriv == 0 {
        return digamma_positive(x);
    }

    // Euler–Maclaurin expansion of the Hurwitz zeta function in DLMF 5.15.1.
    // Factor out x^(-s) to keep the terms in range for large orders.
    let s = deriv + 1;
    let log_x = x.ln();
    let factorial = (1..=deriv).map(f64::from).product::<f64>();
    let sign = if deriv % 2 == 0 { -1.0 } else { 1.0 };
    // At very large x the first integral term gives the scaled zeta sum.
    if x >= f64::from(s) / (2.0 * f64::EPSILON) {
        let order = f64::from(deriv);
        let scaled = (-order * log_x - order.ln()).exp();
        return if scaled == 0.0 {
            0.0
        } else {
            sign * (factorial * scaled)
        };
    }
    if f64::from(s) * log_x > SCALED_EXPONENT_LIMIT {
        return 0.0;
    }
    let steps = (12.0 + 0.5 * f64::from(s) - x).ceil().max(0.0) as u32;
    let end = x + f64::from(steps);
    let mut sum = 0.0;
    for step in (0..steps).rev() {
        sum += (x / (x + f64::from(step))).powi(s);
    }
    let mut tail = end / f64::from(deriv) + 0.5;
    let mut factor = f64::from(s) / end;
    for (k, coefficient) in BERNOULLI_OVER_FACTORIAL.iter().enumerate() {
        tail = factor.mul_add(*coefficient, tail);
        let i = f64::from(s + 2 * k as i32 + 1);
        factor *= (i / end) * ((i + 1.0) / end);
    }
    sum += (x / end).powi(s) * tail;

    let power = x.powi(s);
    let magnitude = if power.is_normal() {
        (factorial / power) * sum
    } else {
        ((-f64::from(s)).mul_add(log_x, lgamma_positive(f64::from(s))) + sum.ln()).exp()
    };
    sign * magnitude
}

fn digamma_positive(mut x: f64) -> f64 {
    // DLMF 5.11.2 and 5.15.5.
    let mut shift = 0.0;
    while x < 12.0 {
        shift -= 1.0 / x;
        x += 1.0;
    }
    let square = 1.0 / (x * x);
    let coefficients = [
        1.0 / 12.0,
        -1.0 / 120.0,
        1.0 / 252.0,
        -1.0 / 240.0,
        1.0 / 132.0,
        -691.0 / 32_760.0,
        1.0 / 12.0,
        -3617.0 / 8160.0,
    ];
    let tail = coefficients
        .iter()
        .rev()
        .fold(0.0_f64, |sum, &c| sum.mul_add(square, c));
    (x.ln() + shift) - (0.5 / x + square * tail)
}

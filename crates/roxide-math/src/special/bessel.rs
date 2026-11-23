//! Real Bessel functions of the first and second kinds.

mod modified;
mod ordinary;

use std::f64::consts::PI;

use crate::Flags;

use super::aux::sinpi;
use super::domain;
use super::gamma::{gamma_positive, lgamma_positive};

// Bound the cost of recurrence in the transition region for very large orders.
const MAX_RECURRENCE: usize = 20_000;

/// Return the modified Bessel function of the first kind.
/// If scaled, return `exp(-x)*I_nu(x)`.
pub fn bessel_i(x: f64, nu: f64, scaled: bool, flags: &mut Flags) -> f64 {
    if let Some(result) = check(x, nu, 1e9, flags) {
        return result;
    }
    if x > if scaled { 1e5 } else { 709.0 } {
        return if scaled { 0.0 } else { f64::INFINITY };
    }
    let order = nu.abs();
    let sin = sinpi(order);
    if x == 0.0 {
        return if order == 0.0 {
            1.0
        } else if nu >= 0.0 || sin == 0.0 {
            0.0
        } else {
            f64::INFINITY.copysign(sin)
        };
    }
    let positive = modified::i_positive(x, order, scaled, flags);
    if nu < 0.0 && sin != 0.0 {
        let k = modified::k_positive(x, order, if scaled { -2.0 * x } else { -x }, flags);
        positive + (2.0 / PI * sin) * k
    } else {
        positive
    }
}

/// Return the Bessel function of the first kind.
pub fn bessel_j(x: f64, nu: f64, flags: &mut Flags) -> f64 {
    if let Some(result) = check(x, nu, 1e9, flags) {
        return result;
    }
    ordinary::j(x, nu, flags)
}

/// Return the modified Bessel function of the second kind.
/// If scaled, return `exp(x)*K_nu(x)`.
pub fn bessel_k(x: f64, nu: f64, scaled: bool, flags: &mut Flags) -> f64 {
    if let Some(result) = check(x, nu, 1e9, flags) {
        return result;
    }
    if x == 0.0 {
        return f64::INFINITY;
    }
    if x.is_infinite() || !scaled && x > 705.342 {
        return 0.0;
    }
    modified::k_positive(x, nu.abs(), if scaled { 0.0 } else { -x }, flags)
}

/// Return the Bessel function of the second kind.
pub fn bessel_y(x: f64, nu: f64, flags: &mut Flags) -> f64 {
    if let Some(result) = check(x, nu, 1e7, flags) {
        return result;
    }
    ordinary::y(x, nu, flags)
}

fn check(x: f64, nu: f64, max_order: f64, flags: &mut Flags) -> Option<f64> {
    if x.is_nan() || nu.is_nan() {
        Some(x + nu)
    } else if x < 0.0 || nu.abs() > max_order {
        Some(domain(flags))
    } else {
        None
    }
}

fn series(x: f64, nu: f64, sign: f64) -> f64 {
    // DLMF 10.2.2 and 10.25.2.
    let half_x = 0.5 * x;
    let ratio = sign * half_x * half_x;
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut residual = 0.0;
    for k in 1..=512 {
        let k = f64::from(k);
        term *= ratio / (k * (nu + k));
        let corrected = term - residual;
        let next = sum + corrected;
        residual = (next - sum) - corrected;
        sum = next;
        if term.abs() <= f64::EPSILON * sum.abs() {
            break;
        }
    }
    let whole = nu as usize;
    let frac = nu.fract();
    let mut leading = if frac == 0.0 {
        1.0
    } else {
        half_x.powf(frac) / gamma_positive(1.0 + frac)
    };
    if whole <= 512 && half_x < 32.0 {
        for k in 1..=whole {
            leading *= half_x / (frac + k as f64);
            if leading == 0.0 {
                break;
            }
        }
        return leading * sum;
    }

    (nu.mul_add(x.ln() - std::f64::consts::LN_2, -lgamma_positive(nu + 1.0)) + sum.abs().ln())
        .exp()
        .copysign(sum)
}

fn asymptotic_terms(x: f64, nu: f64, mut add: impl FnMut(usize, f64)) {
    // DLMF 10.17.1. Stop before the asymptotic terms start to grow.
    let mut term = 1.0;
    for k in 1..=64 {
        let n = (2 * k - 1) as f64;
        let next = term * (((2.0 * nu - n) * (2.0 * nu + n)) / x / (8 * k) as f64);
        if next.abs() > term.abs() {
            break;
        }
        add(k, next);
        if next.abs() <= 0.25 * f64::EPSILON {
            break;
        }
        term = next;
    }
}

// Composite 16-point Gauss–Legendre quadrature. Nodes and weights come from
// the roots of P_16 and the weights 2/((1-node^2)*P'_16(node)^2).
fn integrate(end: f64, panels: usize, f: impl Fn(f64) -> f64) -> f64 {
    const NODES: [f64; 8] = [
        0.095_012_509_837_637_44,
        0.281_603_550_779_258_9,
        0.458_016_777_657_227_37,
        0.617_876_244_402_643_8,
        0.755_404_408_355_003,
        0.865_631_202_387_831_8,
        0.944_575_023_073_232_6,
        0.989_400_934_991_649_9,
    ];
    const WEIGHTS: [f64; 8] = [
        0.189_450_610_455_068_5,
        0.182_603_415_044_923_58,
        0.169_156_519_395_002_54,
        0.149_595_988_816_576_73,
        0.124_628_971_255_533_87,
        0.095_158_511_682_492_79,
        0.062_253_523_938_647_89,
        0.027_152_459_411_754_096,
    ];
    let half_width = end / (2 * panels) as f64;
    let mut sum = 0.0;
    let mut residual = 0.0;
    for panel in 0..panels {
        let mid = (2 * panel + 1) as f64 * half_width;
        for (&node, &weight) in NODES.iter().zip(&WEIGHTS) {
            let offset = node * half_width;
            let term = weight * (f(mid - offset) + f(mid + offset));
            if term.is_infinite() {
                return term;
            }
            let corrected = term - residual;
            let next = sum + corrected;
            if next.is_infinite() {
                return next;
            }
            residual = (next - sum) - corrected;
            sum = next;
        }
    }
    half_width * sum
}

fn x_sinh(x: f64, t: f64) -> f64 {
    if t < 700.0 {
        x * t.sinh()
    } else {
        (x.ln() + t - std::f64::consts::LN_2).exp()
    }
}

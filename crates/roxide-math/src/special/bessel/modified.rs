use std::f64::consts::PI;

use crate::Flags;

use super::{MAX_RECURRENCE, asymptotic_terms, integrate, series};

pub(super) fn i_positive(x: f64, nu: f64, scaled: bool, flags: &mut Flags) -> f64 {
    if x < 24.0 || 0.25 * x * x <= nu + 1.0 {
        let value = series(x, nu, 1.0);
        if value == 0.0 && nu >= 2.0 && series(x, nu - 1.0, 1.0) == 0.0 {
            flags.set(Flags::PRECISION);
        }
        return if scaled { value * (-x).exp() } else { value };
    }
    let count = nu as usize;
    if x >= 24.0 + nu * nu {
        let value = i_hankel(x, nu);
        return if scaled { value } else { value * x.exp() };
    }
    if count > MAX_RECURRENCE {
        flags.set(Flags::PRECISION);
        return 0.0;
    }

    let frac = nu.fract();
    let seed = i_hankel(x, frac);
    // DLMF 10.33.1. Evaluate the continued fraction for consecutive ratios.
    // Keep the ratios in reverse order, then multiply from I_frac upward.
    let top = count + 40 + (12.0 * x.sqrt()) as usize;
    let mut ratio = 0.0;
    let mut ratios = Vec::with_capacity(count);
    for n in (1..=top).rev() {
        ratio = 1.0 / (2.0 * (frac + n as f64) / x + ratio);
        if n <= count {
            ratios.push(ratio);
        }
    }
    let value = if scaled { seed } else { seed * x.exp() };
    ratios
        .iter()
        .rev()
        .fold(value, |value, ratio| value * ratio)
}

fn i_hankel(x: f64, nu: f64) -> f64 {
    let mut sum = 1.0;
    asymptotic_terms(x, nu, |k, term| {
        sum += if k % 2 == 0 { term } else { -term };
    });
    sum / ((2.0 * PI).sqrt() * x.sqrt())
}

pub(super) fn k_positive(x: f64, nu: f64, shift: f64, _flags: &mut Flags) -> f64 {
    if x >= 24.0 + nu * nu {
        return k_seed(x, nu) * shift.exp();
    }
    if nu > MAX_RECURRENCE as f64 {
        return k_debye(x, nu, shift);
    }
    let count = nu as usize;
    let frac = nu.fract();
    let mut previous = k_seed(x, frac);
    if count == 0 {
        return previous * shift.exp();
    }
    let mut value = k_seed(x, frac + 1.0);
    if value.is_infinite() {
        return value;
    }
    // DLMF 10.29.1. Keep a separate binary exponent during upward recurrence.
    let mut exponent = 0;
    for n in 1..count {
        let scale = 2.0 * (frac + n as f64) / x;
        if value > 1e150 / scale.max(1.0) {
            previous *= 2.0_f64.powi(-512);
            value *= 2.0_f64.powi(-512);
            exponent += 512;
        }
        (previous, value) = (value, scale.mul_add(value, previous));
        if value.is_infinite() || f64::from(exponent) > 1024.0 - shift / std::f64::consts::LN_2 {
            return f64::INFINITY;
        }
    }
    if exponent == 0 && shift > -708.0 {
        return value * shift.exp();
    }
    let factor = f64::from(exponent).mul_add(std::f64::consts::LN_2, shift);
    if factor.abs() < 708.0 {
        value * factor.exp()
    } else {
        (factor + value.ln()).exp()
    }
}

fn k_seed(x: f64, nu: f64) -> f64 {
    if nu == 0.5 {
        return (0.5 * PI).sqrt() / x.sqrt();
    }
    if x >= 24.0 {
        let mut sum = 1.0;
        asymptotic_terms(x, nu, |_, term| sum += term);
        return (0.5 * PI).sqrt() * sum / x.sqrt();
    }
    // DLMF 10.32.9. Use cosh(t)-1 = 2*sinh(t/2)^2 for scaled K.
    let end = 2.0 * (32.0_f64.sqrt() / x.sqrt()).asinh();
    integrate(end, (2.0 * end).ceil() as usize, |t| {
        let h = t * 0.5;
        let exponent = if h < 350.0 {
            let sinh = h.sinh();
            -(2.0 * x * sinh) * sinh
        } else {
            -(x.ln() + t - std::f64::consts::LN_2).exp()
        };
        let order = nu * t;
        0.5 * ((exponent + order).exp() + (exponent - order).exp())
    })
}

// DLMF 10.41.4. Compute the scaled exponent without subtracting x.
fn k_debye(x: f64, nu: f64, shift: f64) -> f64 {
    let hypotenuse = x.hypot(nu);
    let t = nu / hypotenuse;
    let square = t * t;
    let u1 = t * (3.0 - 5.0 * square) / 24.0;
    let u2 = square * (81.0 + square * (-462.0 + 385.0 * square)) / 1152.0;
    let ratio = nu / x;
    let hyperbolic = if ratio.is_finite() {
        ratio.asinh()
    } else {
        nu.ln() - x.ln() + std::f64::consts::LN_2
    };
    let exponent = nu * (hyperbolic - t / (1.0 + x / hypotenuse)) + shift;
    let log_prefactor = 0.5 * ((0.5 * PI).ln() - hypotenuse.ln());
    (exponent + log_prefactor + (-u1 / nu + (u2 / nu) / nu).ln_1p()).exp()
}

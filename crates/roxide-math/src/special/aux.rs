//! Accurate helper functions for special functions and probability kernels.

use std::f64::consts::PI;

use super::gamma::{LN_SQRT_2PI, lgamma_positive, stirling_correction};

/// Return `sin(pi*x)` with exact values at integers and half-integers.
pub fn sinpi(x: f64) -> f64 {
    let mut r = x.abs() % 2.0;
    let mut sign = 1.0_f64.copysign(x);
    if r >= 1.0 {
        r -= 1.0;
        if r == 0.0 {
            return 0.0_f64.copysign(x);
        }
        sign = -sign;
    }
    r = r.min(1.0 - r);
    if x.is_finite() {
        sign * sin_quarter(r)
    } else {
        x * 0.0
    }
}

fn sin_quarter(r: f64) -> f64 {
    if r <= 0.25 {
        (PI * r).sin()
    } else {
        (PI * (0.5 - r)).cos()
    }
}

/// Return `cos(pi*x)` with exact values at integers and half-integers.
pub fn cospi(x: f64) -> f64 {
    if !x.is_finite() {
        return x * 0.0;
    }
    let r = x.abs() % 2.0;
    let r = r.min(2.0 - r);
    if r == 0.5 {
        return 0.0;
    }
    if r < 0.5 {
        sin_quarter(0.5 - r)
    } else {
        -sin_quarter(r - 0.5)
    }
}

/// Return `tan(pi*x)` or NaN at a pole.
pub fn tanpi(x: f64) -> f64 {
    let mut r = x % 1.0;
    if r > 0.5 {
        r -= 1.0;
    } else if r < -0.5 {
        r += 1.0;
    }
    let a = r.abs();
    if a == 0.0 {
        0.0
    } else if a == 0.5 {
        f64::NAN
    } else if a == 0.25 {
        r.signum()
    } else {
        (PI * r).tan()
    }
}

/// Return `ln(1+x) - x` without cancellation near zero.
pub fn log1pmx(x: f64) -> f64 {
    if x == f64::INFINITY {
        return f64::NEG_INFINITY;
    }
    if !(-0.5..=1.0).contains(&x) {
        return x.ln_1p() - x;
    }
    let r = x / (2.0 + x);
    let r2 = r * r;
    let mut term = r2;
    let mut sum = 0.0;
    for k in (3..=41).step_by(2) {
        sum += term / f64::from(k);
        term *= r2;
    }
    r * (2.0 * sum - x)
}

/// Return `ln(abs(Gamma(1+a)))`, with extra accuracy near `a = 0`.
pub fn lgamma1p(a: f64) -> f64 {
    if a.abs() > 0.5 || a.is_nan() {
        return super::gamma::lgamma(1.0 + a, &mut crate::Flags::default());
    }

    // DLMF 5.7.3. Separate the terms with zeta(k) replaced by 1.
    // The remaining coefficients decrease as 2^-k / k.
    let series = ZETA_MINUS_ONE_OVER_K
        .iter()
        .rev()
        .fold(0.0_f64, |sum, &coef| sum.mul_add(-a, coef));
    (a * a).mul_add(series, -log1pmx(a) - EULER * a)
}

// The standard library constant is unstable.
#[allow(clippy::approx_constant)]
pub(crate) const EULER: f64 = 0.577_215_664_901_532_9;

/// Return the error of Stirling's leading approximation to `ln(Gamma(n+1))`.
///
/// At zero, return the conventional table value of zero.
pub fn stirlerr(n: f64) -> f64 {
    if n >= 8.0 {
        return stirling_correction(n);
    }
    if n < 0.0 || n.is_nan() {
        return f64::NAN;
    }
    let half_steps = 2.0 * n;
    if half_steps.fract() == 0.0 {
        return STIRLING_HALF_STEPS[half_steps as usize];
    }
    lgamma_positive(n + 1.0) - (n + 0.5) * n.ln() + n - LN_SQRT_2PI
}

/// Return the binomial deviance term `x * ln(x/np) + np - x`.
pub fn bd0(x: f64, np: f64) -> f64 {
    let (high, low) = ebd0(x, np);
    high + low
}

/// Return the binomial deviance as a rounded high part and a residual.
pub fn ebd0(x: f64, np: f64) -> (f64, f64) {
    if !x.is_finite() || !np.is_finite() || x < 0.0 || np <= 0.0 {
        return (f64::NAN, 0.0);
    }
    if x == 0.0 {
        return (np, 0.0);
    }
    let diff = x - np;
    if diff == 0.0 {
        return (0.0, 0.0);
    }
    let mean = 0.5 * x + 0.5 * np;
    if diff.abs() < 0.2 * mean {
        // Loader's odd-power expansion in (x-np)/(x+np).
        let v = 0.5 * (diff / mean);
        let v2 = v * v;
        let head = diff * v;
        let mut sum = (head, diff.mul_add(v, -head));
        let mut term = (x * v) * (2.0 * v2);
        for j in (3..=39).step_by(2) {
            let add = term / f64::from(j);
            sum = add_split(sum.0, sum.1 + add);
            if add.abs() <= f64::EPSILON * sum.0.abs() {
                break;
            }
            term *= v2;
        }
        return sum;
    }

    let ratio = x / np;
    let log_ratio = if (0.5..=2.0).contains(&ratio) {
        (diff / np).ln_1p()
    } else if ratio.is_normal() {
        ratio.ln()
    } else {
        x.ln() - np.ln()
    };
    let product = x * log_ratio;
    if !product.is_finite() {
        return (product, 0.0);
    }
    let (sum, residual) = add_split(product, -diff);
    let (_, diff_residual) = add_split(x, -np);
    add_split(
        sum,
        residual + x.mul_add(log_ratio, -product) - diff_residual,
    )
}

fn add_split(a: f64, b: f64) -> (f64, f64) {
    let sum = a + b;
    let shift = sum - a;
    (sum, (a - (sum - shift)) + (b - shift))
}

/// Evaluate a Chebyshev series with a half-weight constant coefficient.
///
/// The valid argument range is `[-1, 1]`.
pub fn chebyshev_eval(x: f64, coeffs: &[f64]) -> f64 {
    if !(-1.0..=1.0).contains(&x) {
        return f64::NAN;
    }
    let Some((&constant, tail)) = coeffs.split_first() else {
        return 0.0;
    };
    let (mut next, mut after) = (0.0, 0.0);
    for &coef in tail.iter().rev() {
        (next, after) = ((2.0 * x).mul_add(next, coef - after), next);
    }
    x.mul_add(next, 0.5 * constant - after)
}

/// Return `ln(1 - exp(-a))` for positive `a`.
pub fn log1mexp(a: f64) -> f64 {
    if a <= std::f64::consts::LN_2 {
        (-(-a).exp_m1()).ln()
    } else {
        (-(-a).exp()).ln_1p()
    }
}

/// Return `ln(1 + exp(x))` without overflow.
pub fn log1pexp(x: f64) -> f64 {
    if x > 0.0 {
        x + (-x).exp().ln_1p()
    } else {
        x.exp().ln_1p()
    }
}

const ZETA_MINUS_ONE_OVER_K: [f64; 29] = [
    0.3224670334241132,
    0.0673523010531981,
    0.020580808427784546,
    0.007385551028673986,
    0.0028905103307415234,
    0.001192753911703261,
    0.0005096695247430425,
    0.00022315475845357939,
    9.945751278180853e-05,
    4.492623673813314e-05,
    2.050721277567069e-05,
    9.439488275268397e-06,
    4.374866789907488e-06,
    2.039215753801366e-06,
    9.55141213040742e-07,
    4.492469198764566e-07,
    2.1207184805554665e-07,
    1.0043224823968099e-07,
    4.7698101693639804e-08,
    2.2711094608943164e-08,
    1.0838659214896955e-08,
    5.183475041970047e-09,
    2.4836745438024785e-09,
    1.1921401405860912e-09,
    5.731367241678862e-10,
    2.7595228851242334e-10,
    1.330476437424449e-10,
    6.4229645638381e-11,
    3.1044247747322276e-11,
];

const STIRLING_HALF_STEPS: [f64; 16] = [
    0.0,
    0.15342640972002736,
    0.08106146679532726,
    0.05481412105191765,
    0.0413406959554093,
    0.03316287351993629,
    0.02767792568499834,
    0.023746163656297496,
    0.020790672103765093,
    0.018488450532673187,
    0.016644691189821193,
    0.015134973221917378,
    0.013876128823070748,
    0.012810465242920227,
    0.01189670994589177,
    0.011104559758206917,
];

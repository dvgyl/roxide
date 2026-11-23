//! Beta, binomial coefficient, and factorial functions.

use std::f64::consts::PI;

use crate::Flags;

use super::aux::sinpi;
use super::domain;
use super::gamma::{LN_SQRT_2PI, gamma, lgamma, lgamma_positive, stirling_correction};

/// Return the beta function for nonnegative `a` and `b`.
pub fn beta(a: f64, b: f64, flags: &mut Flags) -> f64 {
    if a == 1.0 && b >= 0.0 {
        return 1.0 / b;
    }
    if b == 1.0 && a >= 0.0 {
        return 1.0 / a;
    }
    lbeta(a, b, flags).exp()
}

/// Return the natural logarithm of the beta function.
pub fn lbeta(a: f64, b: f64, flags: &mut Flags) -> f64 {
    if a.is_nan() || b.is_nan() {
        return a + b;
    }
    let (a, b) = (a.min(b), a.max(b));
    if a < 0.0 {
        return domain(flags);
    }
    if a == 0.0 {
        return f64::INFINITY;
    }
    if b == f64::INFINITY {
        return f64::NEG_INFINITY;
    }
    if a == 1.0 {
        return 0.0 - b.ln();
    }
    if b < 8.0 {
        return lgamma_positive(a) + lgamma_positive(b) - lgamma_positive(a + b);
    }

    let ratio = a / b;
    let log_sum = ratio.ln_1p();
    let correction = stirling_correction(b) - stirling_correction(a + b);
    if a < 8.0 {
        // Form b*log1p(a/b) before adding the offset to b.
        // For a subnormal ratio its limiting value is a.
        let main = if ratio.is_normal() { b * log_sum } else { a };
        return lgamma_positive(a) - a * b.ln() + (a - main) - (a - 0.5) * log_sum + correction;
    }
    let total = a + b;
    if total.is_infinite() {
        return f64::NEG_INFINITY;
    }
    let share = a / total;
    let log_share = if share.is_normal() {
        share.ln()
    } else {
        a.ln() - total.ln()
    };
    (stirling_correction(a) + correction + LN_SQRT_2PI - 0.5 * b.ln())
        + (a - 0.5) * log_share
        + b * (-share).ln_1p()
}

/// Return the generalized binomial coefficient, with `k` rounded to an integer.
///
/// Set [`Flags::NONINT_X`] if rounding crosses the relative tolerance.
pub fn choose(n: f64, k: f64, flags: &mut Flags) -> f64 {
    binomial(n, k, false, flags)
}

/// Return the natural logarithm of the absolute generalized binomial coefficient.
pub fn lchoose(n: f64, k: f64, flags: &mut Flags) -> f64 {
    binomial(n, k, true, flags)
}

fn binomial(mut n: f64, k: f64, log: bool, flags: &mut Flags) -> f64 {
    if n.is_nan() || k.is_nan() {
        return n + k;
    }
    let rounded = k.round_ties_even();
    if (k - rounded).abs() > 1e-9 * rounded.abs().max(1.0) {
        flags.set(Flags::NONINT_X);
    }
    let mut k = rounded;
    let rounded_n = n.round_ties_even();
    if (n - rounded_n).abs() <= 1e-9 * rounded_n.abs().max(1.0) {
        n = rounded_n;
    }
    let zero = if log { f64::NEG_INFINITY } else { 0.0 };
    if k < 0.0 || n >= 0.0 && n.is_finite() && n.fract() == 0.0 && k > n {
        return zero;
    }
    if k == 0.0 {
        return if log { 0.0 } else { 1.0 };
    }
    if n.is_infinite() {
        if log && k != 1.0 || k.is_infinite() {
            return domain(flags);
        }
        return if log || n > 0.0 || k % 2.0 == 0.0 {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        };
    }
    if k.is_infinite() {
        return if n < 0.0 { domain(flags) } else { zero };
    }
    if n >= 0.0 && n.fract() == 0.0 {
        k = k.min(n - k);
    }
    if k <= 64.0 && n.abs() <= 1e4 {
        let mut value = 1.0;
        for j in 0..k as u32 {
            let j = f64::from(j);
            value *= (n - j) / (j + 1.0);
        }
        return if log {
            value.abs().ln()
        } else if n.fract() == 0.0 && value.abs() <= (1_u64 << 52) as f64 {
            value.round()
        } else {
            value
        };
    }
    if k == 0.0 {
        return if log { 0.0 } else { 1.0 };
    }
    if k == 1.0 {
        return if log { n.abs().ln() } else { n };
    }

    let mut sign = 1.0;
    let magnitude = if n < 0.0 {
        sign = if k % 2.0 == 0.0 { 1.0 } else { -1.0 };
        n = -n;
        -lbeta(k + 1.0, n, flags) - (k + n).ln()
    } else if k > n {
        let sin = sinpi(n);
        sign = if k % 2.0 == 0.0 { -sin } else { sin }.signum();
        lbeta(n + 1.0, k - n, flags) + sin.abs().ln() - PI.ln()
    } else {
        -lbeta(k, n - k + 1.0, flags) - k.ln()
    };
    if log {
        magnitude
    } else {
        sign * magnitude.exp()
    }
}

/// Return the generalized factorial `Gamma(x+1)`.
pub fn factorial(x: f64, flags: &mut Flags) -> f64 {
    if x.is_nan() { x } else { gamma(x + 1.0, flags) }
}

/// Return the natural logarithm of the absolute generalized factorial.
pub fn lfactorial(x: f64, flags: &mut Flags) -> f64 {
    if x.is_nan() {
        x
    } else {
        lgamma(x + 1.0, flags)
    }
}

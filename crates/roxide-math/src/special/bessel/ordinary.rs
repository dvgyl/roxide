use std::f64::consts::PI;

use crate::Flags;

use super::{MAX_RECURRENCE, asymptotic_terms, integrate, series, x_sinh};
use crate::special::aux::{cospi, sinpi};

pub(super) fn j(x: f64, nu: f64, flags: &mut Flags) -> f64 {
    let order = nu.abs();
    let (sin, cos) = (sinpi(order), cospi(order));
    if x.is_infinite() {
        if nu >= 0.0 || cos != 0.0 {
            flags.set(Flags::UNDERFLOW);
        }
        return if nu < 0.0 && sin != 0.0 {
            (-sin).mul_add(0.0, cos * 0.0)
        } else {
            0.0
        };
    }
    if x == 0.0 {
        return if order == 0.0 {
            1.0
        } else if nu >= 0.0 || sin == 0.0 {
            0.0
        } else {
            f64::INFINITY.copysign(sin)
        };
    }
    if nu >= 0.0 {
        return j_positive(x, order, flags);
    }
    // DLMF 10.4.1 and 10.4.4. Skip zero coefficients at special orders.
    let first = if cos == 0.0 {
        0.0
    } else {
        cos * j_positive(x, order, flags)
    };
    if sin == 0.0 {
        first
    } else {
        first - sin * y_positive(x, order, flags)
    }
}

pub(super) fn y(x: f64, nu: f64, flags: &mut Flags) -> f64 {
    let order = nu.abs();
    let (sin, cos) = (sinpi(order), cospi(order));
    if x > 1e8 {
        if nu < 0.0 && sin != 0.0 && x.is_infinite() {
            flags.set(Flags::UNDERFLOW);
        }
        if nu >= 0.0 {
            return 0.0;
        }
        if sin == 0.0 {
            return cos * 0.0;
        }
        let j = if x.is_finite() {
            j_positive(x, order, flags)
        } else {
            0.0
        };
        return sin.mul_add(j, cos * 0.0);
    }
    if x == 0.0 {
        return if cos == 0.0 && nu < 0.0 {
            0.0
        } else if nu < 0.0 {
            f64::NEG_INFINITY.copysign(-cos)
        } else {
            f64::NEG_INFINITY
        };
    }
    if nu >= 0.0 {
        return y_positive(x, order, flags);
    }
    let second = if cos == 0.0 {
        0.0
    } else {
        cos * y_positive(x, order, flags)
    };
    if sin == 0.0 {
        second
    } else {
        sin.mul_add(j_positive(x, order, flags), second)
    }
}

fn j_positive(x: f64, nu: f64, flags: &mut Flags) -> f64 {
    if x <= 4.0 || 0.25 * x * x <= nu + 1.0 {
        let result = series(x, nu, -1.0);
        if result == 0.0 && nu >= 2.0 && series(x, nu - 1.0, -1.0) == 0.0 {
            flags.set(Flags::PRECISION);
        }
        return result;
    }
    if x >= 24.0 + nu * nu {
        return hankel(x, nu).0;
    }
    let count = nu as usize;
    if count > MAX_RECURRENCE {
        return precision_limit(flags);
    }
    let frac = nu.fract();
    let base = ordinary_seed(x, frac, false);
    if count == 0 {
        return base;
    }
    let next = ordinary_seed(x, frac + 1.0, false);
    if nu > x {
        return miller(x, frac, count, base, next);
    }
    forward(x, frac, count, base, next)
}

fn y_positive(x: f64, nu: f64, flags: &mut Flags) -> f64 {
    if x >= 24.0 + nu * nu {
        return hankel(x, nu).1;
    }
    let count = nu as usize;
    let frac = nu.fract();
    let base = ordinary_seed(x, frac, true);
    if count == 0 {
        return base;
    }
    let next = ordinary_seed(x, frac + 1.0, true);
    let result = forward(x, frac, count.min(MAX_RECURRENCE), base, next);
    if result.is_infinite() {
        if count > 1 && x >= f64::MIN_POSITIVE {
            flags.set(Flags::PRECISION);
        }
        return result;
    }
    if count > MAX_RECURRENCE {
        return precision_limit(flags);
    }
    result
}

fn forward(x: f64, frac: f64, count: usize, mut base: f64, mut next: f64) -> f64 {
    // DLMF 10.6.1. Only use upward recurrence where it is stable.
    for n in 1..count {
        let scale = 2.0 * (frac + n as f64) / x;
        (base, next) = (next, scale.mul_add(next, -base));
        if next.is_infinite() {
            break;
        }
    }
    next
}

fn miller(x: f64, frac: f64, order: usize, base: f64, next: f64) -> f64 {
    let top = order + 40 + (8.0 * x.cbrt()) as usize;
    let (mut current, mut above, mut wanted) = (1.0_f64, 0.0, 0.0);
    for n in (1..=top).rev() {
        if n == order {
            wanted = current;
        }
        let scale = 2.0 * (frac + n as f64) / x;
        (above, current) = (current, scale.mul_add(current, -above));
        if current.abs() > 1e150 {
            current *= 1e-150;
            above *= 1e-150;
            wanted *= 1e-150;
        }
    }
    // Use the larger seed to avoid dividing by a value near a root.
    if base.abs() >= next.abs() {
        base * (wanted / current)
    } else {
        next * (wanted / above)
    }
}

fn ordinary_seed(x: f64, nu: f64, second: bool) -> f64 {
    if nu == 0.5 {
        let scale = (2.0 / PI).sqrt() / x.sqrt();
        return if second {
            -scale * x.cos()
        } else {
            scale * x.sin()
        };
    }
    if x >= 24.0 {
        let (j, y) = hankel(x, nu);
        return if second { y } else { j };
    }
    if !second && x <= 4.0 {
        return series(x, nu, -1.0);
    }

    // Schläfli's integrals, DLMF 10.9.6 and 10.9.7.
    let trig = integrate(PI, 8, |t| {
        let phase = x.mul_add(t.sin(), -nu * t);
        if second { phase.sin() } else { phase.cos() }
    });
    let limit = (100.0 / x).asinh();
    let end = if limit.is_finite() {
        limit
    } else {
        200.0_f64.ln() - x.ln()
    };
    let sin = sinpi(nu);
    if !second && sin == 0.0 {
        return trig / PI;
    }
    let cos = cospi(nu);
    let tail = integrate(end, (2.0 * end).ceil() as usize, |t| {
        let exponent = -x_sinh(x, t);
        let order = nu * t;
        let minus = (exponent - order).exp();
        if second {
            cos.mul_add(minus, (exponent + order).exp())
        } else {
            sin * minus
        }
    });
    (trig - tail) / PI
}

fn hankel(x: f64, nu: f64) -> (f64, f64) {
    // DLMF 10.17.3 and 10.17.4. Subtract the phase by angle addition.
    let (sin, cos) = x.sin_cos();
    let shift = 0.5_f64.mul_add(nu, 0.25);
    let sine_shift = sinpi(shift);
    let cosine_shift = cospi(shift);
    let c = cos.mul_add(cosine_shift, sin * sine_shift);
    let s = sin.mul_add(cosine_shift, -cos * sine_shift);
    let (mut p, mut q) = (1.0, 0.0);
    asymptotic_terms(x, nu, |k, term| match k % 4 {
        0 => p += term,
        1 => q += term,
        2 => p -= term,
        _ => q -= term,
    });
    let scale = (2.0 / PI).sqrt() / x.sqrt();
    (scale * c.mul_add(p, -s * q), scale * s.mul_add(p, c * q))
}

fn precision_limit(flags: &mut Flags) -> f64 {
    flags.set(Flags::PRECISION);
    super::super::domain(flags)
}

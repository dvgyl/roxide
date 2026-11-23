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
    if nu > MAX_RECURRENCE as f64 {
        return olver(x, nu, false);
    }
    let count = nu as usize;
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
    if nu > MAX_RECURRENCE as f64 {
        return olver(x, nu, true);
    }
    let count = nu as usize;
    let frac = nu.fract();
    let base = ordinary_seed(x, frac, true);
    if count == 0 {
        return base;
    }
    let next = ordinary_seed(x, frac + 1.0, true);
    let result = forward(x, frac, count, base, next);
    if result.is_infinite() {
        if count > 1 && x >= f64::MIN_POSITIVE {
            flags.set(Flags::PRECISION);
        }
        return result;
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

// DLMF 10.20.4, 10.20.5, 10.20.10 and 10.20.11.
fn olver(x: f64, nu: f64, second: bool) -> f64 {
    let z = x / nu;
    let d = ((nu - x) / nu) * (1.0 + z);
    let q = d.abs().sqrt();
    let near_turn = d.abs() <= 0.125;
    let phase = if near_turn {
        let series = (0..=20)
            .rev()
            .fold(0.0_f64, |acc, k| acc.mul_add(d, 1.0 / f64::from(2 * k + 3)));
        q * q * q * series
    } else if d > 0.0 {
        q.ln_1p() + (nu.ln() - x.ln()) - q
    } else {
        q - q.atan()
    };
    let inverse_series = if d == 0.0 { 3.0 } else { (q * q * q) / phase };
    let ratio_root = (1.5 / inverse_series).cbrt();
    let (a1, b0, b1) = olver_coefficients(d, inverse_series);
    let nu_root = nu.cbrt();
    let argument = d * (nu_root * ratio_root).powi(2);
    let (value, derivative, exponent) = airy(argument, nu * phase, second);
    let leading = 2.0_f64.sqrt() * ratio_root.sqrt() / nu_root;
    let corrected = (1.0 + (a1 / nu) / nu) * value
        + derivative * ((b0 + (b1 / nu) / nu) / (ratio_root * nu * nu_root));
    let result = leading * corrected;
    let result = if exponent.abs() < 700.0 {
        result * exponent.exp()
    } else {
        (result.abs().ln() + exponent).exp().copysign(result)
    };
    if second { -result } else { result }
}

fn olver_coefficients(d: f64, inverse_series: f64) -> (f64, f64, f64) {
    let (u, v) = airy_coefficients(3);
    if d.abs() > 0.125 {
        let p1 = d / 8.0 - 5.0 / 24.0;
        let p2 = ((81.0 * d - 462.0) * d + 385.0) / 1152.0;
        let p3 = (((30_375.0 * d - 369_603.0) * d + 765_765.0) * d - 425_425.0) / 414_720.0;
        let g = inverse_series;
        return (
            (p2 + g * (v[1] * p1 + g * v[2])) / d.powi(3),
            -(p1 + g * u[1]) / d.powi(2),
            -(p3 + g * (u[1] * p2 + g * (u[2] * p1 + g * u[3]))) / d.powi(5),
        );
    }

    // Expand the numerators in d with rational arithmetic and remove the
    // exact zeros of orders 3, 2 and 5 before rounding the coefficients.
    const A1: [f64; 12] = [
        -0.004_444_444_444_444_444_4,
        -0.000_922_077_922_077_922,
        -0.000_088_489_288_489_288_49,
        0.000_165_927_687_832_449_73,
        0.000_246_691_372_741_792_9,
        0.000_265_995_589_346_254_8,
        0.000_261_824_297_061_500_96,
        0.000_248_730_437_344_655_6,
        0.000_232_721_040_083_232_09,
        0.000_216_362_485_712_365_08,
        0.000_200_738_858_762_752_34,
        0.000_186_267_636_637_545_17,
    ];
    const B0: [f64; 12] = [
        -0.014_285_714_285_714_285,
        -0.007_301_587_301_587_302,
        -0.004_648_113_790_970_933_4,
        -0.003_305_989_248_846_392,
        -0.002_514_904_173_679_684,
        -0.002_001_494_291_222_982_7,
        -0.001_645_412_307_581_358_6,
        -0.001_386_129_673_946_653_3,
        -0.001_190_170_100_530_526_8,
        -0.001_037_647_432_118_638_4,
        -0.000_916_070_111_356_853_6,
        -0.000_817_231_172_809_041_5,
    ];
    const B1: [f64; 12] = [
        0.001_184_859_584_859_585,
        0.000_934_003_456_860_599_7,
        0.000_660_442_709_484_726_3,
        0.000_465_271_115_858_424_5,
        0.000_332_441_431_537_752_47,
        0.000_241_376_898_813_421_38,
        0.000_177_738_181_686_386_35,
        0.000_132_330_076_562_273_12,
        0.000_099_289_023_301_361_38,
        0.000_074_820_046_274_587_93,
        0.000_056_416_018_827_179_7,
        0.000_042_384_796_857_738_43,
    ];
    let polynomial = |coefficients: &[f64]| {
        coefficients
            .iter()
            .rev()
            .fold(0.0_f64, |sum, &c| sum.mul_add(d, c))
    };
    (polynomial(&A1), -polynomial(&B0), -polynomial(&B1))
}

fn airy_coefficients(order: usize) -> ([f64; 40], [f64; 40]) {
    let mut u = [1.0; 40];
    let mut v = u;
    for k in 1..=order {
        let n = k as f64;
        u[k] = u[k - 1] * ((6.0 * n - 5.0) * (6.0 * n - 3.0) * (6.0 * n - 1.0))
            / ((2.0 * n - 1.0) * 216.0 * n);
        v[k] = u[k] * (6.0 * n + 1.0) / (1.0 - 6.0 * n);
    }
    (u, v)
}

// DLMF 9.7.5–9.7.12. Return the Airy values with a common exponential factor.
fn airy_asymptotic(x: f64, phase: f64, second: bool) -> (f64, f64, f64) {
    let (u, v) = airy_coefficients(39);
    let series = |coefficients: &[f64; 40]| {
        let mut even = 1.0;
        let mut odd = 0.0;
        let mut factor = 1.0;
        let mut previous = 1.0;
        for (k, coefficient) in coefficients.iter().enumerate().skip(1) {
            factor /= phase;
            let term = coefficient * factor;
            if term.abs() > previous {
                break;
            }
            previous = term.abs();
            if x > 0.0 {
                even += if second || k % 2 == 0 { term } else { -term };
            } else if k % 2 == 0 {
                even += if k % 4 == 0 { term } else { -term };
            } else {
                odd += if k % 4 == 1 { term } else { -term };
            }
        }
        (even, odd)
    };
    let (a, b) = series(&u);
    let (c, d) = series(&v);
    let root = x.abs().sqrt().sqrt();
    let factor = PI.sqrt().recip();
    if x > 0.0 {
        if second {
            (factor * a / root, factor * root * c, phase)
        } else {
            (0.5 * factor * a / root, -0.5 * factor * root * c, -phase)
        }
    } else {
        let (sine, cosine) = phase.sin_cos();
        let s = (sine - cosine) * std::f64::consts::FRAC_1_SQRT_2;
        let cphase = (cosine + sine) * std::f64::consts::FRAC_1_SQRT_2;
        if second {
            (
                factor * (-s * a + cphase * b) / root,
                factor * root * (cphase * c + s * d),
                0.0,
            )
        } else {
            (
                factor * (cphase * a + s * b) / root,
                factor * root * (s * c - cphase * d),
                0.0,
            )
        }
    }
}

fn airy(x: f64, phase: f64, second: bool) -> (f64, f64, f64) {
    if !(-9.0..=12.0).contains(&x) {
        return airy_asymptotic(x, phase, second);
    }
    let mut start = 0.0;
    let (mut a, mut b) = if second {
        (0.614_926_627_446_000_7, 0.448_288_357_353_826_4)
    } else if x <= 0.0 {
        (0.355_028_053_887_817_2, -0.258_819_403_792_806_8)
    } else {
        // Use the decaying solution at the positive end and integrate backwards.
        start = 12.0;
        let (a, b, exponent) = airy_asymptotic(start, (2.0 / 3.0) * start * start.sqrt(), false);
        (a * exponent.exp(), b * exponent.exp())
    };
    let steps = (2.0 * (x - start).abs()).ceil().max(1.0) as u32;
    let h = (x - start) / f64::from(steps);
    for i in 0..steps {
        // Taylor recurrence for y'' = x*y.
        let center = h.mul_add(f64::from(i), start);
        let mut coefficients = [0.0; 24];
        coefficients[0] = a;
        coefficients[1] = b;
        for k in 0..22 {
            let previous = if k == 0 { 0.0 } else { coefficients[k - 1] };
            coefficients[k + 2] =
                center.mul_add(coefficients[k], previous) / ((k + 1) * (k + 2)) as f64;
        }
        (a, b) = coefficients
            .iter()
            .rev()
            .fold((0.0, 0.0), |(value, deriv), &c| {
                (h.mul_add(value, c), h.mul_add(deriv, value))
            });
    }
    (a, b, 0.0)
}

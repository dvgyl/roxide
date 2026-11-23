//! Scalar normal density, probability, quantile, and random variate functions.

#![allow(clippy::excessive_precision)]

use crate::core::{Flags, Tail};
use crate::rng::Rng;

const LOG_SQRT_2PI: f64 = 0.91893853320467274178;
const INV_SQRT_2PI: f64 = 0.39894228040143267794;

fn invalid(flags: &mut Flags) -> f64 {
    flags.set(Flags::NAN_PRODUCED);
    f64::NAN
}

/// Evaluate the normal density, optionally on the log scale.
pub fn dnorm(x: f64, mean: f64, sd: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || mean.is_nan() || sd.is_nan() {
        return x + mean + sd;
    }
    let zero = if log { f64::NEG_INFINITY } else { 0.0 };
    if sd < 0.0 {
        return invalid(flags);
    }
    if sd.is_infinite() {
        return zero;
    }
    if x.is_infinite() && x == mean {
        return invalid(flags);
    }
    if sd == 0.0 {
        return if x == mean { f64::INFINITY } else { zero };
    }
    let z = ((x - mean) / sd).abs();
    if !z.is_finite() {
        return zero;
    }
    if log {
        return (-0.5 * z).mul_add(z, -LOG_SQRT_2PI) - sd.ln();
    }
    if z < 5.0 {
        return INV_SQRT_2PI * (-0.5 * z * z).exp() / sd;
    }
    if z > 38.6 {
        return zero;
    }
    let (square_hi, square_lo) = half_square(z);
    (INV_SQRT_2PI / sd) * (-square_hi).exp() * (-square_lo).exp()
}

fn horner(x: f64, coefficients: &[f64]) -> f64 {
    coefficients
        .iter()
        .rev()
        .fold(0.0_f64, |value, &c| value.mul_add(x, c))
}

fn half_square(x: f64) -> (f64, f64) {
    let hi = (x * 16.0).floor() / 16.0;
    (0.5 * hi * hi, 0.5 * (x - hi) * (x + hi))
}

// Cody (1969), rational approximations to erf and the scaled complementary erf.
const ERF_NUM: &[f64] = &[
    3209.37758913846947,
    377.485237685302021,
    113.864154151050156,
    3.16112374387056560,
    0.185777706184603153,
];
const ERF_DEN: &[f64] = &[
    2844.23683343917062,
    1282.61652607737228,
    244.024637934444173,
    23.6012909523441209,
    1.0,
];
const ERFC_NUM: &[f64] = &[
    1230.33935479799725,
    2051.07837782607147,
    1712.04761263407058,
    881.952221241769090,
    298.635138197400131,
    66.1191906371416295,
    8.88314979438837594,
    0.564188496988670089,
    2.15311535474403846e-8,
];
const ERFC_DEN: &[f64] = &[
    1230.33935480374942,
    3439.36767414372164,
    4362.61909014324716,
    3290.79923573345963,
    1621.38957456669019,
    537.181101862009858,
    117.693950891312499,
    15.7449261107098347,
    1.0,
];
const FAR_NUM: &[f64] = &[
    0.000658749161529837803,
    0.0160837851487422766,
    0.125781726111229246,
    0.360344899949804439,
    0.305326634961232344,
    0.0163153871373020978,
];
const FAR_DEN: &[f64] = &[
    0.00233520497626869185,
    0.0605183413124413191,
    0.527905102951428412,
    1.87295284992346047,
    2.56852019228982242,
    1.0,
];

fn scaled_erfc(x: f64) -> f64 {
    if x <= 4.0 {
        horner(x, ERFC_NUM) / horner(x, ERFC_DEN)
    } else {
        let r = x.recip();
        let s = r * r;
        (-s).mul_add(
            horner(s, FAR_NUM) / horner(s, FAR_DEN),
            0.564189583547756287,
        ) * r
    }
}

fn small_log_tail(z: f64) -> f64 {
    if z > 1.0e150 {
        return (-0.5 * z) * z;
    }
    let x = z * std::f64::consts::FRAC_1_SQRT_2;
    let (hi, lo) = half_square(z);
    (0.5 * scaled_erfc(x)).ln() - lo - hi
}

fn log_one_minus_exp(x: f64) -> f64 {
    if x < -std::f64::consts::LN_2 {
        (-x.exp()).ln_1p()
    } else {
        (-x.exp_m1()).ln()
    }
}

/// Evaluate the requested normal tail probability or its logarithm.
pub fn pnorm(q: f64, mean: f64, sd: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if q.is_nan() || mean.is_nan() || sd.is_nan() {
        return q + mean + sd;
    }
    if sd < 0.0 || (q.is_infinite() && q == mean) {
        return invalid(flags);
    }
    if sd == 0.0 {
        let p = f64::from(u8::from((q >= mean) == tail.lower));
        return if tail.log { p.ln() } else { p };
    }
    let delta = q - mean;
    let z = if delta.is_infinite() {
        delta
    } else {
        delta / sd
    };
    let z = if tail.lower { z } else { -z };
    let x = z * std::f64::consts::FRAC_1_SQRT_2;
    if x.abs() <= 0.46875 {
        let s = x * x;
        let p = (0.5 * x).mul_add(horner(s, ERF_NUM) / horner(s, ERF_DEN), 0.5);
        return if tail.log { p.ln() } else { p };
    }
    let lp = small_log_tail(z.abs());
    match (z < 0.0, tail.log) {
        (true, true) => lp,
        (true, false) if z > -38.0 => {
            let (hi, lo) = half_square(z.abs());
            (-hi).exp() * ((-lo).exp() * (0.5 * scaled_erfc(x.abs())))
        }
        (true, false) => lp.exp(),
        (false, true) => log_one_minus_exp(lp),
        (false, false) => -lp.exp_m1(),
    }
}

// Wichura (1988), Algorithm AS 241, double-precision coefficient tables.
const A: &[f64] = &[
    3.3871328727963666080,
    133.14166789178437745,
    1971.5909503065514427,
    13731.693765509461125,
    45921.953931549871457,
    67265.770927008700853,
    33430.575583588128105,
    2509.0809287301226727,
];
const B: &[f64] = &[
    1.0,
    42.313330701600911252,
    687.18700749205790830,
    5394.1960214247511077,
    21213.794301586595867,
    39307.895800092710610,
    28729.085735721942674,
    5226.4952788528545610,
];
const C: &[f64] = &[
    1.42343711074968357734,
    4.63033784615654529590,
    5.76949722146069140550,
    3.64784832476320460504,
    1.27045825245236838258,
    0.241780725177450611770,
    0.0227238449892691845833,
    0.000774545014278341407640,
];
const D: &[f64] = &[
    1.0,
    2.05319162663775882187,
    1.67638483018380384940,
    0.689767334985100004550,
    0.148103976427480074590,
    0.0151986665636164571966,
    0.000547593808499534494600,
    1.05075007164441684324e-9,
];
const E: &[f64] = &[
    6.65790464350110377720,
    5.46378491116411436990,
    1.78482653991729133580,
    0.296560571828504891230,
    0.0265321895265761230930,
    0.00124266094738807843860,
    0.0000271155556874348757815,
    0.000000201033439929228813265,
];
const F: &[f64] = &[
    1.0,
    0.599832206555887937690,
    0.136929880922735805310,
    0.0148753612908506148525,
    0.000786869131145613259100,
    0.0000184631831751005468180,
    0.000000142151175831644588870,
    2.04426310338993978564e-15,
];

fn extreme_quantile(lp: f64, r: f64) -> f64 {
    if r >= 6.4e8 {
        return r * std::f64::consts::SQRT_2;
    }
    let twice_log = -2.0 * lp;
    let mut t = twice_log - (twice_log.ln() + 2.0 * LOG_SQRT_2PI);
    for _ in 0..4 {
        let s = t.recip();
        let mills = s * horner(s, &[-1.0, 3.0, -15.0, 105.0, -945.0, 10395.0]);
        t = twice_log - (t.ln() + 2.0 * LOG_SQRT_2PI) + 2.0 * mills.ln_1p();
    }
    let x = t.sqrt();
    let (hi, lo) = half_square(x);
    let scale = 0.5 * scaled_erfc(x * std::f64::consts::FRAC_1_SQRT_2);
    let error = ((-hi - lp) - lo) + scale.ln();
    let derivative = -INV_SQRT_2PI / scale;
    let step = error / derivative;
    x - step / (1.0 + 0.5 * step * (x + derivative))
}

pub(crate) fn standard_quantile(p: f64, tail: Tail) -> f64 {
    let lower = match (tail.lower, tail.log) {
        (true, true) => p.exp(),
        (false, true) => -p.exp_m1(),
        (true, false) => p,
        (false, false) => 0.5 - p + 0.5,
    };
    let q = lower - 0.5;
    if q.abs() <= 0.425 {
        let r = (-q).mul_add(q, 0.180625);
        return (q * horner(r, A)) / horner(r, B);
    }
    let direct = (q < 0.0) == tail.lower;
    let lp = match (direct, tail.log) {
        (true, true) => p,
        (true, false) => p.ln(),
        (false, true) => log_one_minus_exp(p),
        (false, false) => (0.5 - p + 0.5).ln(),
    };
    let r = (-lp).sqrt();
    let value = if r <= 5.0 {
        horner(r - 1.6, C) / horner(r - 1.6, D)
    } else if r <= 27.0 {
        horner(r - 5.0, E) / horner(r - 5.0, F)
    } else {
        extreme_quantile(lp, r)
    };
    if q < 0.0 { -value } else { value }
}

/// Evaluate a normal quantile for the specified probability and tail.
pub fn qnorm(p: f64, mean: f64, sd: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || mean.is_nan() || sd.is_nan() {
        return p + mean + sd;
    }
    if (tail.log && p > 0.0) || (!tail.log && !(0.0..=1.0).contains(&p)) {
        return invalid(flags);
    }
    let zero = if tail.log { f64::NEG_INFINITY } else { 0.0 };
    let one = if tail.log { 0.0 } else { 1.0 };
    if p == zero || p == one {
        return if (p == one) == tail.lower {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        };
    }
    if sd < 0.0 {
        return invalid(flags);
    }
    if sd == 0.0 {
        return mean;
    }
    let value = sd.mul_add(standard_quantile(p, tail), mean);
    if value.is_nan() {
        invalid(flags)
    } else {
        value
    }
}

/// Draw from the normal distribution with the specified mean and standard deviation.
pub fn rnorm(rng: &mut Rng, mean: f64, sd: f64, flags: &mut Flags) -> f64 {
    if mean.is_nan() || !sd.is_finite() || sd < 0.0 {
        return invalid(flags);
    }
    if sd == 0.0 || !mean.is_finite() {
        return mean;
    }
    sd.mul_add(rng.norm_rand(), mean)
}

/// Return the ratio of the normal density at `z` to its upper tail.
pub(crate) fn mills_ratio(z: f64, log_tail: f64) -> f64 {
    if z >= 8.0 {
        let fraction = (1..=64)
            .rev()
            .fold(0.0, |value, k| f64::from(k) / (z + value));
        z + fraction
    } else {
        (-0.5 * z * z - 0.918_938_533_204_672_8 - log_tail).exp()
    }
}

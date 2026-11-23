//! Log-scale Gaussian quadrature for normal and chi-square mixtures.

use super::LOG_SQRT_2PI;
use crate::special::aux::{lgamma1p, stirlerr};

const NODES: [f64; 8] = [
    0.095_012_509_837_637_44,
    0.281_603_550_779_258_9,
    0.458_016_777_657_227_4,
    0.617_876_244_402_643_8,
    0.755_404_408_355_003,
    0.865_631_202_387_831_8,
    0.944_575_023_073_232_6,
    0.989_400_934_991_649_9,
];
const WEIGHTS: [f64; 8] = [
    0.189_450_610_455_068_5,
    0.182_603_415_044_923_6,
    0.169_156_519_395_002_54,
    0.149_595_988_816_576_73,
    0.124_628_971_255_533_88,
    0.095_158_511_682_492_78,
    0.062_253_523_938_647_89,
    0.027_152_459_411_754_096,
];

fn panel(low: f64, high: f64, f: &impl Fn(f64) -> f64) -> f64 {
    let center = low.midpoint(high);
    let radius = 0.5 * (high - low);
    let sum: f64 = NODES
        .into_iter()
        .zip(WEIGHTS)
        .map(|(node, weight)| {
            weight * (f(node.mul_add(radius, center)) + f((-node).mul_add(radius, center)))
        })
        .sum();
    radius * sum
}

// Integrate a unimodal log integrand. The two guesses bracket its important region.
pub(super) fn log_integral(left: f64, right: f64, scale: f64, f: impl Fn(f64) -> f64) -> f64 {
    let mut low = left - scale;
    let mut high = right + scale;
    let mut stride = scale;
    for _ in 0..60 {
        if f(low - stride) > f(low) {
            low -= stride;
        } else if f(high + stride) > f(high) {
            high += stride;
        } else {
            break;
        }
        stride *= 2.0;
    }
    for _ in 0..80 {
        if high - low <= scale * 1e-6 {
            break;
        }
        let third = (high - low) / 3.0;
        let a = low + third;
        let b = high - third;
        if f(a) < f(b) {
            low = a;
        } else {
            high = b;
        }
    }
    let peak = low.midpoint(high);
    let height = f(peak);
    if !height.is_finite() {
        return height;
    }
    let limits = |direction: f64| {
        let mut distance = scale;
        let mut nearest = peak;
        for _ in 0..60 {
            let x = peak + direction * distance;
            if f(x) < height - 43.0 || !x.is_finite() {
                for _ in 0..24 {
                    let mid = nearest.midpoint(peak + direction * distance);
                    if f(mid) < height - 43.0 {
                        distance = (mid - peak).abs();
                    } else {
                        nearest = mid;
                    }
                }
                break;
            }
            nearest = x;
            distance *= 2.0;
        }
        peak + direction * distance
    };
    let f = |x| (f(x) - height).exp();
    let integrate = |a: f64, b: f64| {
        let width = (b - a) / 8.0;
        (0..8)
            .map(|k| {
                let offset = f64::from(k);
                let low = width.mul_add(offset, a);
                let high = width.mul_add(offset + 1.0, a);
                panel(low, high, &f)
            })
            .sum::<f64>()
    };
    let integral = integrate(limits(-1.0), peak) + integrate(peak, limits(1.0));
    height + integral.ln()
}

// Log density in w = sqrt(df) * log(s), where df * s^2 is chi-square.
pub(super) fn log_chisq_scale(w: f64, df: f64) -> f64 {
    let h = 2.0 * w / df.sqrt();
    let a = 0.5 * df;
    let deviance = if h.abs() < 0.25 {
        let series = (2..=19)
            .rev()
            .fold(0.0_f64, |sum, k| (h * sum + 1.0) / f64::from(k));
        2.0 * w * w * series
    } else {
        a * (h.exp_m1() - h)
    };
    let center = if a >= 8.0 {
        0.5 * std::f64::consts::LN_2 - LOG_SQRT_2PI - stirlerr(a)
    } else if a == 0.0 {
        0.5 * df.ln()
    } else {
        a.mul_add(a.ln(), -a) + 0.5 * df.ln() - lgamma1p(a)
    };
    center - deviance
}

// Integrate a log-concave function with log value zero at its mode.
pub(super) fn centered_integral(log_relative: impl Fn(f64) -> f64) -> f64 {
    let integrate = |direction: f64| {
        let mut edge = 1.0;
        for _ in 0..16 {
            let end = log_relative(direction * edge);
            let mid = log_relative(direction * (edge * 0.5));
            // Concavity bounds the remaining integral by the chord's exponential tail.
            let slope = (end - mid) / (edge * 0.5);
            if slope < 0.0 && end - (-slope).ln() < -40.0 {
                break;
            }
            edge *= 2.0;
        }
        let f = |x| log_relative(direction * x).exp();
        let mut sum = panel(0.0, 1.0, &f);
        let mut start = 1.0;
        while start < edge {
            sum += panel(start, 2.0 * start, &f);
            start *= 2.0;
        }
        sum
    };
    (integrate(-1.0) + integrate(1.0)).ln()
}

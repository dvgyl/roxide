use super::{NormalKind, Rng};
use crate::core::Tail;
use crate::dist::normal::standard_quantile;

impl Rng {
    /// Draw a standard normal variate with the selected normal algorithm.
    pub fn norm_rand(&mut self) -> f64 {
        match self.kinds.normal {
            NormalKind::Inversion => {
                standard_quantile(self.precise_uniform(134_217_728.0), Tail::default())
            }
            NormalKind::BoxMuller => {
                if let Some(value) = self.bm_saved.take() {
                    return value;
                }
                let theta = std::f64::consts::TAU * self.unif_rand();
                let radius = (-2.0 * self.unif_rand().ln()).sqrt();
                let (sin, cos) = theta.sin_cos();
                self.bm_saved = Some(radius * sin).filter(|&value| value != 0.0);
                radius * cos
            }
            NormalKind::KindermanRamage => kinderman_ramage(self, false),
            NormalKind::BuggyKindermanRamage => kinderman_ramage(self, true),
            NormalKind::AhrensDieter => ahrens_dieter(self),
            NormalKind::UserNorm => f64::NAN,
        }
    }

    pub(super) fn precise_uniform(&mut self, scale: f64) -> f64 {
        ((scale * self.unif_rand()).floor() + self.unif_rand()) / scale
    }

    /// Draw an exponential variate with rate one.
    pub fn exp_rand(&mut self) -> f64 {
        let ln2 = std::f64::consts::LN_2;
        let mut u = self.unif_rand();
        let mut offset = 0.0;
        while 2.0 * u <= 1.0 {
            offset += ln2;
            u *= 2.0;
        }
        let v = 2.0 * u - 1.0;
        if v <= ln2 {
            return offset + v;
        }
        let mut minimum = self.unif_rand();
        let mut term = ln2;
        let mut threshold = ln2;
        for k in 2..=16 {
            minimum = minimum.min(self.unif_rand());
            term *= ln2 / f64::from(k);
            threshold += term;
            if v <= threshold {
                break;
            }
        }
        minimum.mul_add(ln2, offset)
    }
}

fn kinderman_ramage(rng: &mut Rng, legacy: bool) -> f64 {
    const A: f64 = 2.216035867166471;
    let selector = rng.unif_rand();
    if selector.is_nan() {
        return selector;
    }
    if selector < 0.884070402298758 {
        let slope: f64 = if legacy {
            1.13113163544180
        } else {
            1.13113163544418
        };
        return A * (slope.mul_add(selector, rng.unif_rand()) - 1.0);
    }
    if selector >= 0.973310954173898 {
        loop {
            let b = rng.unif_rand();
            let c = rng.unif_rand();
            let square = A * A - 2.0 * c.ln();
            if b * b * square <= A * A {
                return if selector < 0.986655477086949 {
                    square.sqrt()
                } else {
                    -square.sqrt()
                };
            }
        }
    }

    let (slope, intercept, limit, area): (f64, f64, f64, f64) = if selector >= 0.958720824790463 {
        (-0.630834801921960, A, 0.755591531667601, 0.034240503750111)
    } else if selector >= 0.911312780288703 {
        (
            1.10547366102207,
            0.479727404222441,
            0.872834976671790,
            0.049264496373128,
        )
    } else {
        (
            -0.595507138015940,
            0.479727404222441,
            0.805577924423817,
            0.053377549506886,
        )
    };
    let innermost = selector < 0.911312780288703;
    loop {
        let b = rng.unif_rand();
        let c = rng.unif_rand();
        let t = slope.mul_add(b.min(c), intercept);
        if innermost && !legacy && t < 0.0 {
            continue;
        }
        if b.max(c) <= limit
            || (!(innermost && legacy)
                && area * (b - c).abs()
                    <= 0.398942280401433 * (-0.5 * t * t).exp() - 0.180025191068563 * (A - t.abs()))
        {
            return if b < c { t } else { -t };
        }
    }
}

// Algorithm FL (Ahrens and Dieter, 1973). These tables use seven significant digits.
const BOUNDS: [f64; 32] = [
    0.0, 0.03917609, 0.07841241, 0.1177699, 0.1573107, 0.1970991, 0.2372021, 0.2776904, 0.3186394,
    0.3601299, 0.4022501, 0.4450965, 0.4887764, 0.5334097, 0.5791322, 0.626099, 0.6744898,
    0.7245144, 0.7764218, 0.8305109, 0.8871466, 0.9467818, 1.00999, 1.077516, 1.150349, 1.229859,
    1.318011, 1.417797, 1.534121, 1.67594, 1.862732, 2.153875,
];
const THRESHOLDS: [f64; 31] = [
    0.0007673828,
    0.00230687,
    0.003860618,
    0.005438454,
    0.007050699,
    0.008708396,
    0.01042357,
    0.01220953,
    0.01408125,
    0.01605579,
    0.0181529,
    0.02039573,
    0.02281177,
    0.02543407,
    0.02830296,
    0.03146822,
    0.03499233,
    0.03895483,
    0.04345878,
    0.04864035,
    0.05468334,
    0.06184222,
    0.07047983,
    0.08113195,
    0.09462444,
    0.1123001,
    0.136498,
    0.1716886,
    0.2276241,
    0.330498,
    0.5847031,
];
const HEIGHTS: [f64; 31] = [
    0.03920617, 0.03932705, 0.03950999, 0.03975703, 0.04007093, 0.04045533, 0.04091481, 0.04145507,
    0.04208311, 0.04280748, 0.04363863, 0.04458932, 0.04567523, 0.04691571, 0.04833487, 0.04996298,
    0.05183859, 0.05401138, 0.05654656, 0.0595313, 0.06308489, 0.06737503, 0.07264544, 0.07926471,
    0.08781922, 0.09930398, 0.1155599, 0.1404344, 0.1836142, 0.2790016, 0.7010474,
];
// The last strip covers the fixed-up zero uniform. Its width matches the
// first strip threshold in the GNU R endpoint probe.
const WIDTHS: &[f64] = &[
    0.2636843,
    0.2425085,
    0.2255674,
    0.2116342,
    0.1999243,
    0.1899108,
    0.1812252,
    0.1736014,
    0.1668419,
    0.1607967,
    0.1553497,
    0.1504094,
    0.1459026,
    0.14177,
    0.1379632,
    0.1344418,
    0.1311722,
    0.128126,
    0.1252791,
    0.1226109,
    0.1201036,
    0.1177417,
    0.1155119,
    0.1134023,
    0.1114027,
    0.1095039,
    THRESHOLDS[0],
];

fn ahrens_dieter(rng: &mut Rng) -> f64 {
    let u = rng.unif_rand();
    if u.is_nan() {
        return u;
    }
    let negative = u > 0.5;
    let folded = if negative { 2.0 * u - 1.0 } else { 2.0 * u };
    let bin = (32.0 * folded) as usize;
    let bin = bin.min(31);
    let v = 32.0 * folded - bin as f64;
    let value = if bin == 0 {
        fl_tail(rng, v)
    } else {
        fl_strip(rng, bin - 1, v)
    };
    if negative { -value } else { value }
}

fn fl_strip(rng: &mut Rng, i: usize, mut v: f64) -> f64 {
    let base = BOUNDS[i];
    loop {
        if v > THRESHOLDS[i] {
            return base + (v - THRESHOLDS[i]) * HEIGHTS[i];
        }
        let w = rng.unif_rand() * (BOUNDS[i + 1] - base);
        let mut z = w * (base + 0.5 * w);
        loop {
            if v > z {
                return base + w;
            }
            let x = rng.unif_rand();
            let retry_strip = v < x;
            v = rng.unif_rand();
            if retry_strip {
                break;
            }
            z = x;
        }
    }
}

fn fl_tail(rng: &mut Rng, mut v: f64) -> f64 {
    let mut base = BOUNDS[31];
    let mut strip = 0;
    while 2.0 * v < 1.0 && strip + 1 < WIDTHS.len() {
        base += WIDTHS[strip];
        strip += 1;
        v *= 2.0;
    }
    let width = WIDTHS[strip];
    let mut w = (2.0 * v - 1.0) * width;
    let mut z = w * (base + 0.5 * w);
    loop {
        let v = rng.unif_rand();
        if v > z {
            return base + w;
        }
        let x = rng.unif_rand();
        if v >= x {
            z = x;
        } else {
            w = rng.unif_rand() * width;
            z = w * (base + 0.5 * w);
        }
    }
}

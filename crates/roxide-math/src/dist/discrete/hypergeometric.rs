//! Hypergeometric masses, tails, and random draws.

use super::binomial::binomial_log;
use super::*;
use crate::dist::normal::{mills_ratio, pnorm};
use crate::rng::Rng;
use crate::special::{aux::stirlerr, lbeta};

const RECURRENCE_SCALE: f64 = f64::from_bits((1023 + 512) << 52);

struct ScaledMass {
    log_base: f64,
    factor: f64,
    shifts: i32,
}

impl ScaledMass {
    fn log(&self) -> f64 {
        let scale = f64::from(-512 * self.shifts);
        scale.mul_add(std::f64::consts::LN_2, self.log_base + self.factor.ln())
    }

    fn raw(&self) -> f64 {
        if self.log_base < -700.0 {
            return self.log().exp();
        }
        let mut mass = self.log_base.exp() * self.factor;
        for _ in 0..self.shifts {
            mass /= RECURRENCE_SCALE;
        }
        mass
    }
}

#[derive(Clone, Copy)]
struct Urn {
    white: f64,
    black: f64,
    draws: f64,
}

impl Urn {
    fn new(white: f64, black: f64, draws: f64) -> Option<Self> {
        let white = white.round_ties_even();
        let black = black.round_ties_even();
        let draws = draws.round_ties_even();
        (white >= 0.0
            && black >= 0.0
            && (white + black).is_finite()
            && draws >= 0.0
            && draws <= white + black)
            .then_some(Self {
                white,
                black,
                draws,
            })
    }

    fn min(self) -> f64 {
        (self.draws - self.black).max(0.0)
    }

    fn max(self) -> f64 {
        self.draws.min(self.white)
    }

    fn mode(self) -> f64 {
        ((self.draws + 1.0) * ((self.white + 1.0) / (self.white + self.black + 2.0)))
            .floor()
            .clamp(self.min(), self.max())
    }

    fn mass(self, x: f64) -> f64 {
        if x < self.min() || x > self.max() {
            return -INF;
        }
        if self.min() == self.max() {
            return 0.0;
        }
        self.mass_at(x)
    }

    fn mass_at(self, x: f64) -> f64 {
        let total = self.white + self.black;
        let p = self.draws / total;
        let q = (total - self.draws) / total;
        binomial_log(x, self.white, p, q) + binomial_log(self.draws - x, self.black, p, q)
            - binomial_log(self.draws, total, p, q)
    }

    // Keep the recurrence factor separate to avoid rounding a small mass
    // through a large negative logarithm.
    fn mass_from_mode(self, x: f64, tail_factor: f64) -> ScaledMass {
        let mode = self.mode();
        let log_base = self.mass_at(mode);
        let mut product = tail_factor;
        let mut shifts = 0;
        let mut j = mode;
        while j != x {
            if j < x {
                product *= self.forward(j);
                j += 1.0;
            } else {
                product *= self.backward(j);
                j -= 1.0;
            }
            if product < 1e-150 {
                product *= RECURRENCE_SCALE;
                shifts += 1;
            }
        }
        if product == 0.0 {
            return ScaledMass {
                log_base: self.mass_at(x),
                factor: tail_factor,
                shifts: 0,
            };
        }
        ScaledMass {
            log_base,
            factor: product,
            shifts,
        }
    }

    fn scaled_mass(self, x: f64, factor: f64) -> ScaledMass {
        let mode = self.mode();
        if (mode - x).abs() <= 1024.0 && mode.max(x) < 9_007_199_254_740_992.0 {
            self.mass_from_mode(x, factor)
        } else {
            ScaledMass {
                log_base: self.mass(x),
                factor,
                shifts: 0,
            }
        }
    }

    fn forward(self, x: f64) -> f64 {
        ratio(
            self.white - x,
            self.draws - x,
            x + 1.0,
            self.black - (self.draws - x) + 1.0,
        )
    }

    fn backward(self, x: f64) -> f64 {
        ratio(
            x,
            self.black - (self.draws - x),
            self.white - x + 1.0,
            self.draws - x + 1.0,
        )
    }

    fn cumulative(self, x: f64, tail: Tail, flags: &mut Flags) -> f64 {
        if let Some(result) = self.narrow_tail(x, tail) {
            return result;
        }
        if x < self.min() || x >= self.max() {
            return step(x >= self.max(), tail);
        }

        let total = self.white + self.black;
        let p = self.white / total;
        let q = self.black / total;
        let fraction = self.draws / total;
        let sd = (self.draws * p * q * ((total - self.draws) / (total - 1.0))).sqrt();
        let offset = (-self.draws).mul_add(p, x) + 0.5;
        let z = offset / sd;
        if sd > 50_000.0 && z.abs() <= 8.0 {
            // First Edgeworth term, with lattice continuity correction.
            let lower = z <= 0.0;
            let log_tail = pnorm(
                -z.abs(),
                0.0,
                1.0,
                Tail {
                    lower: true,
                    log: true,
                },
                flags,
            );
            let skew = (q - p) * (1.0 - 2.0 * fraction) / sd;
            let adjustment = (skew / 6.0) * (1.0 - z * z) * mills_ratio(z.abs(), log_tail);
            let relative = if lower { adjustment } else { -adjustment };
            return from_log(log_tail + relative.ln_1p(), lower, tail);
        }
        let lower = x < self.mode();
        if x + 1.0 == x || x - 1.0 == x || sd > 50_000.0 {
            return self.block_tail(x, lower, tail, flags);
        }
        let start = if lower { x } else { x + 1.0 };
        let stop = if lower { self.min() } else { self.max() };
        let mut x = start;
        let mut term = 1.0;
        let mut total: f64 = 1.0;
        let mut error = 0.0;
        for _ in 0..2_000_000 {
            if x == stop {
                return self.tail_probability(start, total, lower, tail);
            }
            let ratio = if lower {
                self.backward(x)
            } else {
                self.forward(x)
            };
            term *= ratio;
            if term <= f64::EPSILON * total * (1.0 - ratio) {
                return self.tail_probability(start, total, lower, tail);
            }
            let add = term - error;
            let sum = total + add;
            error = (sum - total) - add;
            total = sum;
            let next = x + if lower { -1.0 } else { 1.0 };
            if next == x {
                break;
            }
            x = next;
        }
        self.block_tail(start - if lower { 0.0 } else { 1.0 }, lower, tail, flags)
    }

    fn narrow_tail(self, x: f64, tail: Tail) -> Option<f64> {
        if self.max() - self.min() > 1024.0
            || self.white + self.black < (1_u64 << 53) as f64
            || self.white.max(self.black).max(self.draws) >= u128::MAX as f64
        {
            return None;
        }
        let (white, black, draws) = (self.white as u128, self.black as u128, self.draws as u128);
        let lo = draws.saturating_sub(black);
        let hi = draws.min(white);
        if hi < lo || hi - lo > 1024 {
            return None;
        }
        if x < 0.0 || x >= u128::MAX as f64 {
            return Some(step(x >= 0.0, tail));
        }
        let x = x as u128;
        if x < lo || x >= hi {
            return Some(step(x >= hi, tail));
        }
        let mut mass = 0.0;
        let mut below = -INF;
        let mut above = -INF;
        for j in lo..=hi {
            if j <= x {
                below = log_add(below, mass);
            } else {
                above = log_add(above, mass);
            }
            if j < hi {
                let numerator = (white - j) as f64 * (draws - j) as f64;
                let denominator = (j + 1) as f64 * (black - (draws - j) + 1) as f64;
                mass += (numerator / denominator).ln();
            }
        }
        let lower = below <= above;
        Some(from_log(
            below.min(above) - log_add(below, above),
            lower,
            tail,
        ))
    }

    fn block_tail(self, x: f64, lower: bool, tail: Tail, flags: &mut Flags) -> f64 {
        // Exchange colors to reduce the upper tail to a lower tail.
        let (urn, start) = if lower {
            (self, x)
        } else {
            (
                Self {
                    white: self.black,
                    black: self.white,
                    ..self
                },
                (self.draws - x) - 1.0,
            )
        };
        let a = start;
        let b = urn.black - (urn.draws - start);
        let c = urn.white - start;
        let d = urn.draws - start;
        let remaining = a.min(b);
        let log_ratio = |offset: f64| {
            ratio(
                a - offset,
                b - offset,
                (c + offset) + 1.0,
                (d + offset) + 1.0,
            )
            .min(1.0)
            .ln()
        };
        let mut offset = 0.0;
        let mut mass = 0.0;
        let mut sum = 0.0;
        let mut width: f64 = 1.0;
        for _ in 0..16_384 {
            if offset >= remaining {
                return from_log(urn.mass_at(start) + sum, lower, tail);
            }
            let left = log_ratio(offset);
            if left == -INF || (left < 0.0 && mass + left - log1mexp(-left) < sum - 38.0) {
                return from_log(urn.mass_at(start) + sum, lower, tail);
            }
            let block = width.min(remaining - offset);
            let right = log_ratio(offset + (block - 1.0));
            let upper = mass + log_geometric(left, block);
            let bound = mass + log_geometric(right, block);
            if block <= 1.0 || upper - bound <= 2e-6 || upper < sum - 38.0 {
                sum = log_add(sum, upper.midpoint(bound));
                mass += block * left.midpoint(right);
                offset += block;
                width = (width * 2.0).min(remaining);
            } else {
                width = (block * 0.5).floor().max(1.0);
            }
        }
        flags.set(Flags::PRECISION);
        from_log((urn.mass_at(start) + sum).min(0.0), lower, tail)
    }

    fn tail_probability(self, start: f64, total: f64, lower: bool, tail: Tail) -> f64 {
        let probability = self.scaled_mass(start, total);
        match (lower == tail.lower, tail.log) {
            (true, false) => probability.raw(),
            (false, true) => {
                let p = probability.raw();
                if p < 0.25 {
                    (-p).ln_1p()
                } else {
                    from_log(probability.log(), lower, tail)
                }
            }
            _ => from_log(probability.log(), lower, tail),
        }
    }
}

fn ratio(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let numerator = a * b;
    let denominator = c * d;
    if numerator.is_finite() && denominator.is_finite() {
        numerator / denominator
    } else {
        (a / c) * (b / d)
    }
}

fn log_add(a: f64, b: f64) -> f64 {
    let max = a.max(b);
    if max == -INF {
        max
    } else {
        max + (a.min(b) - max).exp().ln_1p()
    }
}

fn log_geometric(log_ratio: f64, count: f64) -> f64 {
    if log_ratio == 0.0 {
        count.ln()
    } else {
        log_ratio + log1mexp(-count * log_ratio) - log1mexp(-log_ratio)
    }
}

/// Return the hypergeometric mass at `x`, the number of white balls drawn.
pub fn dhyper(x: f64, m: f64, n: f64, k: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || m.is_nan() || n.is_nan() || k.is_nan() {
        return x + m + n + k;
    }
    if !integral(m) || !integral(n) || !integral(k) {
        return invalid(flags);
    }
    let Some(urn) = Urn::new(m, n, k) else {
        return invalid(flags);
    };
    if x < urn.min() || x > urn.max() {
        return zero(log);
    }
    match observation(x, true, flags) {
        Some(x) if (urn.min()..=urn.max()).contains(&x) && urn.min() != urn.max() => {
            let mass = urn.scaled_mass(x, 1.0);
            if log { mass.log() } else { mass.raw() }
        }
        Some(x) => on_scale(urn.mass(x), log),
        None => zero(log),
    }
}

/// Return the requested hypergeometric tail probability at `q`.
pub fn phyper(q: f64, m: f64, n: f64, k: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if q.is_nan() || m.is_nan() || n.is_nan() || k.is_nan() {
        return q + m + n + k;
    }
    match Urn::new(m, n, k) {
        Some(urn) => urn.cumulative(count_floor(q), tail, flags),
        None => invalid(flags),
    }
}

/// Return the smallest hypergeometric count at the requested probability.
pub fn qhyper(p: f64, m: f64, n: f64, k: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || m.is_nan() || n.is_nan() || k.is_nan() {
        return p + m + n + k;
    }
    let Some(urn) = Urn::new(m, n, k) else {
        return invalid(flags);
    };
    if !p.is_finite() || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, urn.min(), urn.max()) {
        return x;
    }
    quantile(urn, p, tail, flags)
}

fn quantile(urn: Urn, p: f64, tail: Tail, flags: &mut Flags) -> f64 {
    let min = urn.min();
    let max = urn.max();
    let target = lower_probability(p, tail) * (1.0 - 1000.0 * f64::EPSILON);
    if target == 0.0 || min == max {
        return min;
    }
    if max - min > 100_000.0 {
        return first_count(min, max, urn.mode(), |x| {
            let value = urn.cumulative(x, Tail::default(), flags);
            (!value.is_nan()).then_some(value >= target)
        })
        .unwrap_or(f64::NAN);
    }

    // The lower-end beta identity preserves the oracle's rounding at CDF jumps.
    let log_choose = |total: f64, count: f64, flags: &mut Flags| {
        let a = count.min(total - count) + 1.0;
        let b = count.max(total - count) + 1.0;
        let sum = a + b;
        let log_beta = if a >= 8.0 {
            let share = a / sum;
            let correction = stirlerr(a) + stirlerr(b) - stirlerr(sum);
            (a - 0.5) * share.ln()
                + (b - 0.5) * (-share).ln_1p()
                + (LOG_SQRT_2PI - 0.5 * sum.ln() + correction)
        } else {
            lbeta(a, b, flags)
        };
        -(total + 1.0).ln() - log_beta
    };
    let mut lp = log_choose(urn.white, min, flags) + log_choose(urn.black, urn.draws - min, flags)
        - log_choose(urn.white + urn.black, urn.draws, flags);
    let mut x = min;
    let mut mass = lp.exp();
    let mut sum = mass;
    if mass < f64::MIN_POSITIVE {
        let log_target = target.ln();
        while sum < target && x < max && x + 1.0 > x {
            lp += urn.forward(x).ln();
            x += 1.0;
            // Before the mass is representable, even the entire partial sum
            // cannot reach an ordinary target.
            mass = lp.exp();
            sum += mass;
            if lp >= log_target {
                break;
            }
        }
    } else {
        while sum < target && x < max && x + 1.0 > x {
            mass *= urn.forward(x);
            x += 1.0;
            sum += mass;
        }
    }
    x
}

/// Draw a hypergeometric count from `rng`.
pub fn rhyper(rng: &mut Rng, m: f64, n: f64, k: f64, flags: &mut Flags) -> f64 {
    let Some(urn) = Urn::new(m, n, k) else {
        return invalid(flags);
    };
    let total = urn.white + urn.black;
    if total > INT_MAX {
        return if urn.draws == 1.0 {
            rbinom(rng, 1.0, urn.white / total, flags)
        } else {
            quantile(urn, rng.unif_rand(), UPPER, flags)
        };
    }

    // Exchange colors and sample complements to work on the smaller support.
    let swap_colors = urn.white > urn.black;
    let complement = urn.draws > total / 2.0;
    let simple = Urn {
        white: urn.white.min(urn.black),
        black: urn.white.max(urn.black),
        draws: urn.draws.min(total - urn.draws),
    };
    let mode = simple.mode();
    let mut x = if simple.min() == simple.max() {
        simple.min()
    } else if mode - simple.min() < 10.0 {
        invert(rng, simple)
    } else {
        h2pe(rng, simple, mode)
    };
    if x.is_nan() {
        flags.set(Flags::NO_CONVERGENCE);
    }
    if swap_colors {
        x = simple.draws - x;
    }
    if complement {
        x = urn.white - x;
    }
    x
}

fn invert(rng: &mut Rng, urn: Urn) -> f64 {
    let min = urn.min();
    let max = urn.max();
    let initial = urn.mass(min).exp();
    for _ in 0..100_000 {
        let mut u = rng.unif_rand();
        let mut x = min;
        let mut mass = initial;
        for _ in 0..1024 {
            if u <= mass || x >= max {
                break;
            }
            u -= mass;
            mass *= urn.forward(x);
            x += 1.0;
        }
        if u <= mass {
            return x;
        }
    }
    f64::NAN
}

fn h2pe(rng: &mut Rng, urn: Urn, mode: f64) -> f64 {
    // Kachitvichyanukul and Schmeiser (1985), central rectangle and two tails.
    let total = urn.white + urn.black;
    let sd = ((urn.white / total)
        * (urn.black / total)
        * urn.draws
        * ((total - urn.draws) / (total - 1.0)))
        .sqrt();
    let width = (1.5 * sd).floor() + 0.5;
    let left = (mode - width + 0.5).max(urn.min());
    let right = (mode + width + 0.5).min(urn.max() + 1.0);
    let peak = urn.mass(mode);
    let left_height = (urn.mass(left) - peak).exp();
    let right_height = (urn.mass(right - 1.0) - peak).exp();
    let left_rate = -urn.backward(left).ln();
    let right_rate = -urn.forward(right - 1.0).ln();
    let rectangle = right - left;
    let left_area = rectangle + left_height / left_rate;
    let area = left_area + right_height / right_rate;
    for _ in 0..100_000 {
        let u = area * rng.unif_rand();
        let v = rng.unif_rand();
        let (x, height) = if u < rectangle {
            ((left + u).floor(), v)
        } else if u <= left_area {
            (
                (left + v.ln() / left_rate).floor(),
                v * (u - rectangle) * left_rate,
            )
        } else {
            (
                (right - v.ln() / right_rate).floor(),
                v * (u - left_area) * right_rate,
            )
        };
        if (urn.min()..=urn.max()).contains(&x) && height.ln() <= urn.mass(x) - peak {
            return x;
        }
    }
    f64::NAN
}

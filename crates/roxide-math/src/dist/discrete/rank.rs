//! Rank distribution probabilities from exact counting recurrences.

use super::*;
use crate::rng::Rng;
use crate::special::lchoose;

const MAX_SUPPORT: f64 = 1_000_000.0;
const MAX_UPDATES: f64 = 50_000_000.0;
const MAX_SAMPLE: f64 = 1_000_000.0;

struct RankMasses {
    // Store probabilities symmetrically about the support midpoint.
    probabilities: Vec<f64>,
    max: usize,
}

impl RankMasses {
    fn wilcox(m: f64, n: f64) -> Option<Self> {
        let small = m.min(n);
        let large = m.max(n);
        let support = small * large;
        let updates = (small + 1.0) * small * (large + 1.0) * large / 4.0;
        if small <= 0.0 || support > MAX_SUPPORT || updates > MAX_UPDATES {
            return None;
        }
        let (m, n) = (large as usize, small as usize);
        let mut rows = vec![vec![1.0]; n + 1];
        for i in 1..=m {
            for j in 1..=n {
                let (preceding, current) = rows.split_at_mut(j);
                let row = &mut current[0];
                let old_len = row.len();
                row.resize(i * j + 1, 0.0);
                let denominator = (i + j) as f64;
                let exclude = i as f64 / denominator;
                let include = j as f64 / denominator;
                // Partition subsets by whether they contain the largest rank.
                for value in &mut row[..old_len] {
                    *value *= exclude;
                }
                for (value, previous) in row[i..].iter_mut().zip(&preceding[j - 1]) {
                    *value += include * previous;
                }
            }
        }
        let mut probabilities = rows.pop()?;
        probabilities.truncate(m * n / 2 + 1);
        Some(Self {
            probabilities,
            max: m * n,
        })
    }

    fn signrank(n: f64) -> Option<Self> {
        let support = n * (n + 1.0) / 2.0;
        if n <= 0.0 || support > MAX_SUPPORT || n * support / 6.0 > 100_000_000.0 {
            return None;
        }
        let n = n as usize;
        let max = n * (n + 1) / 2;
        let middle = max / 2;
        let mut probabilities = vec![0.0; middle + 1];
        probabilities[0] = 1.0;
        for rank in 1..=n {
            let high = (rank * (rank + 1) / 2).min(middle);
            for k in (rank..=high).rev() {
                probabilities[k] = 0.5 * (probabilities[k] + probabilities[k - rank]);
            }
            for probability in &mut probabilities[..rank.min(high + 1)] {
                *probability *= 0.5;
            }
        }
        Some(Self { probabilities, max })
    }

    fn mass(&self, x: usize) -> f64 {
        self.probabilities[x.min(self.max - x)]
    }

    fn cumulative(&self, x: usize, tail: Tail) -> f64 {
        let lower = x < self.max.div_ceil(2);
        let end = if lower { x } else { self.max - x - 1 };
        let probability = self.probabilities[..=end].iter().sum::<f64>();
        match (tail.lower == lower, tail.log) {
            (true, false) => probability,
            (true, true) => probability.ln(),
            (false, false) => 0.5 - probability + 0.5,
            (false, true) => (-probability).ln_1p(),
        }
    }

    fn quantile(&self, p: f64, tail: Tail) -> f64 {
        let probability = lower_probability(p, tail);
        let lower = probability <= 0.5;
        let target = if lower {
            probability - 10.0 * f64::EPSILON
        } else {
            0.5 - probability + 0.5 + 10.0 * f64::EPSILON
        };
        let mut total = 0.0;
        for x in 0..=self.max {
            total += self.mass(x);
            if (lower && total >= target) || (!lower && total > target) {
                return if lower { x } else { self.max - x } as f64;
            }
        }
        self.max as f64
    }
}

fn mass(x: f64, log: bool, table: Option<RankMasses>, flags: &mut Flags) -> f64 {
    let Some(table) = table else {
        return invalid(flags);
    };
    match observation(x.round_ties_even(), false, flags).filter(|_| integral(x)) {
        Some(x) if x <= table.max as f64 => {
            let p = table.mass(x as usize);
            if log { p.ln() } else { p }
        }
        _ => zero(log),
    }
}

fn cumulative(q: f64, tail: Tail, table: Option<RankMasses>, flags: &mut Flags) -> f64 {
    let Some(table) = table else {
        return invalid(flags);
    };
    let q = count_floor(q);
    if q < 0.0 || q >= table.max as f64 {
        return step(q >= 0.0, tail);
    }
    table.cumulative(q as usize, tail)
}

fn quantile(p: f64, tail: Tail, table: Option<RankMasses>, flags: &mut Flags) -> f64 {
    let Some(table) = table else {
        return invalid(flags);
    };
    table.quantile(p, tail)
}

/// Return the Wilcoxon rank sum mass at `x`.
pub fn dwilcox(x: f64, m: f64, n: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || m.is_nan() || n.is_nan() {
        return x + m + n;
    }
    let (m, n) = (m.round_ties_even(), n.round_ties_even());
    if !positive_ranks(m) || !positive_ranks(n) {
        return invalid(flags);
    }
    let max = m * n;
    if x < 0.0 || x > max || !integral(x) {
        return zero(log);
    }
    let x = x.round_ties_even();
    if x == 0.0 || x == max {
        return on_scale(-lchoose(m + n, m.min(n), flags), log);
    }
    mass(x, log, RankMasses::wilcox(m, n), flags)
}

/// Return the requested Wilcoxon rank sum tail probability at `q`.
pub fn pwilcox(q: f64, m: f64, n: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if q.is_nan() || m.is_nan() || n.is_nan() {
        return q + m + n;
    }
    let (m, n) = (m.round_ties_even(), n.round_ties_even());
    if !positive_ranks(m) || !positive_ranks(n) {
        return invalid(flags);
    }
    let max = m * n;
    let q = count_floor(q);
    if q < 0.0 || q >= max {
        return step(q >= 0.0, tail);
    }
    if q == 0.0 || q == max - 1.0 {
        return from_log(-lchoose(m + n, m.min(n), flags), q == 0.0, tail);
    }
    cumulative(q, tail, RankMasses::wilcox(m, n), flags)
}

/// Return the smallest Wilcoxon rank sum at the requested probability.
pub fn qwilcox(p: f64, m: f64, n: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || m.is_nan() || n.is_nan() {
        return p + m + n;
    }
    let (m, n) = (m.round_ties_even(), n.round_ties_even());
    if !positive_ranks(m) || !positive_ranks(n) || !p.is_finite() || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(boundary) = endpoints(p, tail, 0.0, m * n) {
        return boundary;
    }
    quantile(p, tail, RankMasses::wilcox(m, n), flags)
}

/// Draw a Wilcoxon rank sum using the sampling method in `rng`.
pub fn rwilcox(rng: &mut Rng, m: f64, n: f64, flags: &mut Flags) -> f64 {
    let m = m.round_ties_even();
    let n = n.round_ties_even();
    if m < 0.0 || n < 0.0 || !(m + n).is_finite() {
        return invalid(flags);
    }
    if m == 0.0 || n == 0.0 {
        return 0.0;
    }
    if m + n > MAX_SAMPLE {
        return invalid(flags);
    }
    let ranks = rng.sample_without_replacement((m + n) as u64, n as usize, false);
    ranks.iter().sum::<u64>() as f64 - n * (n + 1.0) / 2.0
}

fn positive_ranks(n: f64) -> bool {
    n > 0.0 && n.is_finite()
}

/// Return the Wilcoxon signed rank mass at `x`.
pub fn dsignrank(x: f64, n: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || n.is_nan() {
        return x + n;
    }
    let n = n.round_ties_even();
    if !positive_ranks(n) {
        return invalid(flags);
    }
    let max = n * (n + 1.0) / 2.0;
    if x < 0.0 || x > max {
        return zero(log);
    }
    if x.round_ties_even() == 0.0 || x.round_ties_even() == max {
        return if integral(x) {
            on_scale(-n * std::f64::consts::LN_2, log)
        } else {
            zero(log)
        };
    }
    mass(x, log, RankMasses::signrank(n), flags)
}

/// Return the requested Wilcoxon signed rank tail probability at `q`.
pub fn psignrank(q: f64, n: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if q.is_nan() || n.is_nan() {
        return q + n;
    }
    let n = n.round_ties_even();
    if !positive_ranks(n) {
        return invalid(flags);
    }
    let max = n * (n + 1.0) / 2.0;
    let q = count_floor(q);
    if q < 0.0 || q >= max {
        return step(q >= 0.0, tail);
    }
    if q == 0.0 || q == max - 1.0 {
        return from_log(-n * std::f64::consts::LN_2, q == 0.0, tail);
    }
    cumulative(q, tail, RankMasses::signrank(n), flags)
}

/// Return the smallest Wilcoxon signed rank at the requested probability.
pub fn qsignrank(p: f64, n: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || n.is_nan() {
        return p + n;
    }
    let n = n.round_ties_even();
    if !positive_ranks(n) || !p.is_finite() || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(boundary) = endpoints(p, tail, 0.0, n * (n + 1.0) / 2.0) {
        return boundary;
    }
    quantile(p, tail, RankMasses::signrank(n), flags)
}

/// Draw a Wilcoxon signed rank sum from `rng`.
pub fn rsignrank(rng: &mut Rng, n: f64, flags: &mut Flags) -> f64 {
    let n = n.round_ties_even();
    if !(0.0..=MAX_SAMPLE).contains(&n) {
        return invalid(flags);
    }
    (1..=n as u32)
        .filter(|_| rng.unif_rand() > 0.5)
        .map(f64::from)
        .sum()
}

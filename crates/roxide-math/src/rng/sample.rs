use std::collections::HashSet;

use super::{Rng, SampleKind, UniformKind};

impl Rng {
    /// Draw a zero-based index for a population of size `dn`.
    pub fn unif_index(&mut self, dn: f64) -> f64 {
        if !dn.is_finite() || dn <= 0.0 || dn > (1u64 << 53) as f64 {
            return 0.0;
        }
        match self.kinds.sample {
            SampleKind::Rounding => {
                let cutoff = match self.kinds.uniform {
                    UniformKind::KnuthTaocp | UniformKind::KnuthTaocp2002 => 33_554_431.0,
                    _ => 2_147_483_647.0,
                };
                let u = if dn > cutoff {
                    self.precise_uniform(33_554_432.0)
                } else {
                    self.unif_rand()
                };
                (dn * u).floor()
            }
            SampleKind::Rejection => {
                let bits = dn.log2().ceil() as u32;
                let mask = (1u64 << bits) - 1;
                loop {
                    let mut value = 0;
                    for _ in 0..=bits / 16 {
                        value = (value << 16) | (65536.0 * self.unif_rand()) as u64;
                    }
                    value &= mask;
                    if (value as f64) < dn {
                        return value as f64;
                    }
                }
            }
        }
    }

    /// Draw one-based indices with replacement from `n` elements.
    /// The caller must validate the population and output sizes.
    pub fn sample_with_replacement(&mut self, n: u64, size: usize) -> Vec<u64> {
        (0..size)
            .map(|_| self.unif_index(n as f64) as u64 + 1)
            .collect()
    }

    /// Draw one-based indices without replacement from `n` elements.
    /// Select hashing to use memory proportional to the sample size.
    /// The caller must validate the population and output sizes.
    pub fn sample_without_replacement(&mut self, n: u64, size: usize, use_hash: bool) -> Vec<u64> {
        if size == 0 || size as u64 > n {
            return Vec::new();
        }
        if size == 1 && !use_hash {
            return self.sample_with_replacement(n, size);
        }
        let mut out = Vec::with_capacity(size);
        if use_hash {
            let mut seen = HashSet::with_capacity(size);
            while out.len() < size {
                let index = self.unif_index(n as f64) as u64 + 1;
                if seen.insert(index) {
                    out.push(index);
                }
            }
        } else {
            let mut pool: Vec<_> = (1..=n).collect();
            for _ in 0..size {
                let index = self.unif_index(pool.len() as f64) as usize;
                out.push(pool.swap_remove(index));
            }
        }
        out
    }

    /// Draw one-based indices with replacement from normalized nonnegative weights.
    pub fn sample_prob_with_replacement(&mut self, probabilities: &[f64], size: usize) -> Vec<u64> {
        let n = probabilities.len();
        if n == 0 || size == 0 {
            return Vec::new();
        }
        let scaled = probabilities.iter().map(|p| p * n as f64);
        if scaled.clone().filter(|&p| p > 0.1).count() > 200 {
            return self.sample_alias(scaled.collect(), size);
        }
        let mut weights = ordered_weights(probabilities);
        let mut total = 0.0;
        for (p, _) in &mut weights {
            total += *p;
            *p = total;
        }
        (0..size)
            .map(|_| {
                let u = self.unif_rand();
                let i = weights.partition_point(|&(p, _)| p < u).min(n - 1);
                weights[i].1
            })
            .collect()
    }

    /// Draw one-based indices without replacement from normalized nonnegative weights.
    /// The caller must ensure at least `size` weights are positive.
    pub fn sample_prob_without_replacement(
        &mut self,
        probabilities: &[f64],
        size: usize,
    ) -> Vec<u64> {
        let mut weights = ordered_weights(probabilities);
        let mut total = 1.0;
        (0..size.min(weights.len()))
            .map(|_| {
                let target = self.unif_rand() * total;
                let mut sum = 0.0;
                let index = weights
                    .iter()
                    .take(weights.len() - 1)
                    .position(|&(p, _)| {
                        sum += p;
                        target <= sum
                    })
                    .unwrap_or(weights.len() - 1);
                let (weight, value) = weights.remove(index);
                total -= weight;
                value
            })
            .collect()
    }

    fn sample_alias(&mut self, mut scaled: Vec<f64>, size: usize) -> Vec<u64> {
        let n = scaled.len();
        let mut alias: Vec<_> = (0..n).collect();
        let mut queue = vec![0; n];
        let mut small_end = 0;
        let mut large_start = n;
        for (i, &p) in scaled.iter().enumerate() {
            if p < 1.0 {
                queue[small_end] = i;
                small_end += 1;
            } else {
                large_start -= 1;
                queue[large_start] = i;
            }
        }
        let mut head = 0;
        while head < small_end && large_start < n {
            let small = queue[head];
            let large = queue[large_start];
            alias[small] = large;
            scaled[large] += scaled[small] - 1.0;
            head += 1;
            if scaled[large] < 1.0 {
                queue[small_end] = large;
                small_end += 1;
                large_start += 1;
            }
        }
        (0..size)
            .map(|_| {
                let (i, value) = match self.kinds.sample {
                    SampleKind::Rounding => {
                        let value = n as f64 * self.unif_rand();
                        (value as usize, value)
                    }
                    SampleKind::Rejection => {
                        let i = self.unif_index(n as f64) as usize;
                        (i, i as f64 + self.unif_rand())
                    }
                };
                let index = if value < i as f64 + scaled[i] {
                    i
                } else {
                    alias[i]
                };
                index as u64 + 1
            })
            .collect()
    }
}

fn ordered_weights(probabilities: &[f64]) -> Vec<(f64, u64)> {
    let mut weights: Vec<_> = probabilities
        .iter()
        .enumerate()
        .map(|(i, &p)| (p, i as u64 + 1))
        .collect();
    // A min-heap gives descending probabilities and preserves the R tie order.
    for root in (0..weights.len() / 2).rev() {
        sift_min(&mut weights, root);
    }
    for end in (1..weights.len()).rev() {
        weights.swap(0, end);
        sift_min(&mut weights[..end], 0);
    }
    weights
}

fn sift_min(weights: &mut [(f64, u64)], mut root: usize) {
    while 2 * root + 1 < weights.len() {
        let mut child = 2 * root + 1;
        if child + 1 < weights.len() && weights[child + 1].0 < weights[child].0 {
            child += 1;
        }
        if weights[root].0 <= weights[child].0 {
            break;
        }
        weights.swap(root, child);
        root = child;
    }
}

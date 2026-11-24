use super::{fits, index_info};

#[derive(Clone, Copy)]
pub(super) struct Triangle<'a> {
    data: &'a [f64],
    ld: usize,
    n: usize,
    upper: bool,
    scale: f64,
}

impl<'a> Triangle<'a> {
    pub(super) fn new(data: &'a [f64], ld: usize, n: usize, upper: bool, scale: f64) -> Self {
        Self {
            data,
            ld,
            n,
            upper,
            scale,
        }
    }

    fn col(self, j: usize) -> &'a [f64] {
        &self.data[j * self.ld..j * self.ld + self.n]
    }

    fn off_diagonal(self, j: usize) -> std::ops::Range<usize> {
        if self.upper { 0..j } else { j + 1..self.n }
    }

    fn step(self, step: usize, transpose: bool) -> usize {
        if self.upper != transpose {
            self.n - 1 - step
        } else {
            step
        }
    }

    pub(super) fn scaled_solve(self, x: &mut [f64], transpose: bool) -> f64 {
        let mut scale = 1.0;
        for step in 0..self.n {
            let j = self.step(step, transpose);
            let col = self.col(j);
            let diag = col[j] / self.scale;
            if transpose {
                let dot = self
                    .off_diagonal(j)
                    .map(|i| x[i] * (col[i] / self.scale))
                    .sum::<f64>();
                x[j] -= dot;
            }
            if diag == 0.0 {
                x.fill(0.0);
                x[j] = 1.0;
                scale = 0.0;
            } else {
                let bound = diag.abs().min(1.0);
                if x[j].abs() > bound {
                    let factor = bound / x[j].abs();
                    rescale(x, factor);
                    scale *= factor;
                }
                x[j] /= diag;
            }
            if !transpose {
                for i in self.off_diagonal(j) {
                    x[i] -= x[j] * (col[i] / self.scale);
                }
            }
        }
        scale
    }
}

fn rescale(x: &mut [f64], factor: f64) {
    for value in x {
        *value *= factor;
    }
}

pub(super) fn unit_one(x: &mut [f64]) -> f64 {
    let norm = x.iter().map(|v| v.abs()).sum::<f64>();
    if norm == 0.0 {
        return 1.0;
    }
    let scale = norm.recip();
    rescale(x, scale);
    scale
}

pub(super) fn grow_transposed_solution(t: Triangle<'_>, z: &mut [f64]) {
    z.fill(0.0);
    let mut magnitude: f64 = 1.0;
    for step in 0..t.n {
        let j = t.step(step, true);
        let col = t.col(j);
        let diag = col[j] / t.scale;
        let bound = z[j].abs() + magnitude;
        if diag != 0.0 && bound > diag.abs() {
            let scale = diag.abs() / bound;
            rescale(z, scale);
            magnitude *= scale;
        }
        let rhs = magnitude.copysign(z[j]);
        let (plus, minus) = if diag == 0.0 {
            (1.0, 1.0)
        } else {
            ((z[j] + rhs) / diag, (z[j] - rhs) / diag)
        };
        let remaining = if t.upper { j + 1..t.n } else { 0..j };
        let row_entry = |i: usize| t.data[i * t.ld + j] / t.scale;
        let score = |x: f64| {
            (x * diag).abs()
                + remaining
                    .clone()
                    .map(|i| (z[i] - x * row_entry(i)).abs())
                    .sum::<f64>()
        };
        let best = if score(plus) >= score(minus) {
            plus
        } else {
            minus
        };
        z[j] = best;
        for i in remaining {
            z[i] -= best * row_entry(i);
        }
    }
    unit_one(z);
}

/// Solve a triangular system in place. A nonzero units digit of `job` selects
/// the upper triangle. A nonzero tens digit selects the transpose.
/// A zero diagonal returns its one-based index and leaves `b` unchanged.
pub fn dtrsl(t: &[f64], ldt: usize, n: usize, b: &mut [f64], job: i32) -> i32 {
    if !fits(t.len(), ldt, n, n) || b.len() < n {
        return -1;
    }
    if let Some(j) = (0..n).find(|&j| t[j * ldt + j] == 0.0) {
        return index_info(j);
    }
    solve(
        Triangle::new(t, ldt, n, job % 10 != 0, 1.0),
        &mut b[..n],
        (job / 10) % 10 != 0,
    );
    0
}

pub(super) fn solve(t: Triangle<'_>, b: &mut [f64], transpose: bool) {
    for step in 0..t.n {
        let j = t.step(step, transpose);
        let col = t.col(j);
        if transpose {
            let dot = t.off_diagonal(j).map(|i| col[i] * b[i]).sum::<f64>();
            b[j] = (b[j] - dot) / col[j];
        } else if b[j] != 0.0 {
            b[j] /= col[j];
            for i in t.off_diagonal(j) {
                b[i] -= col[i] * b[j];
            }
        }
    }
}

/// Estimate the reciprocal one-norm condition number of a triangular matrix.
/// Zero `job` selects the lower triangle. Other values select the upper triangle.
/// The first `n` entries of `z` contain the unit one-norm condition vector.
pub fn dtrco(t: &[f64], ldt: usize, n: usize, z: &mut [f64], job: i32) -> f64 {
    if n == 0 || !fits(t.len(), ldt, n, n) || z.len() < n {
        return 0.0;
    }
    let z = &mut z[..n];
    let upper = job != 0;
    let active = |j| {
        let start = j * ldt;
        if upper {
            &t[start..=start + j]
        } else {
            &t[start + j..start + n]
        }
    };
    let scale = (0..n).flat_map(active).map(|x| x.abs()).fold(0.0, f64::max);
    if !scale.is_finite() || (0..n).flat_map(active).any(|x| x.is_nan()) {
        return 0.0;
    }
    if scale == 0.0 {
        z.fill(0.0);
        z[0] = 1.0;
        return 0.0;
    }
    let norm = (0..n)
        .map(|j| active(j).iter().map(|x| x.abs() / scale).sum::<f64>())
        .fold(0.0, f64::max);
    let t = Triangle::new(t, ldt, n, upper, scale);
    grow_transposed_solution(t, z);
    let factor = t.scaled_solve(z, false);
    (factor * unit_one(z) / norm).clamp(0.0, 1.0)
}

use super::tri::{Triangle, grow_transposed_solution, unit_one};
use super::{fits, index_info};

#[derive(Clone, Copy)]
struct UpperBand {
    ld: usize,
    width: usize,
    band_row: Option<usize>,
}

impl UpperBand {
    fn index(self, i: usize, j: usize) -> usize {
        j * self.ld + self.band_row.map_or(i, |row| row - (j - i))
    }

    fn factor(self, a: &mut [f64], n: usize) -> i32 {
        for j in 0..n {
            let first = j.saturating_sub(self.width);
            let diagonal = self.index(j, j);
            let mut squares = 0.0;
            for i in first..j {
                let dot = (first..i)
                    .map(|k| a[self.index(k, i)] * a[self.index(k, j)])
                    .sum::<f64>();
                let ij = self.index(i, j);
                let entry = (a[ij] - dot) / a[self.index(i, i)];
                a[ij] = entry;
                squares += entry * entry;
            }
            let pivot = a[diagonal] - squares;
            if pivot <= 0.0 || !pivot.is_finite() {
                return index_info(j);
            }
            a[diagonal] = pivot.sqrt();
        }
        0
    }

    fn solve(self, a: &[f64], b: &mut [f64]) {
        for j in 0..b.len() {
            let dot = (j.saturating_sub(self.width)..j)
                .map(|i| a[self.index(i, j)] * b[i])
                .sum::<f64>();
            b[j] = (b[j] - dot) / a[self.index(j, j)];
        }
        for j in (0..b.len()).rev() {
            b[j] /= a[self.index(j, j)];
            for i in j.saturating_sub(self.width)..j {
                b[i] -= a[self.index(i, j)] * b[j];
            }
        }
    }

    fn dense(ld: usize, n: usize) -> Self {
        Self {
            ld,
            width: n,
            band_row: None,
        }
    }
}

/// Factor the upper triangle of a symmetric positive definite matrix as `RᵀR`.
/// Return the one-based index of the first leading minor that fails, or zero.
pub fn dpofa(a: &mut [f64], lda: usize, n: usize) -> i32 {
    if !fits(a.len(), lda, n, n) {
        return -1;
    }
    UpperBand::dense(lda, n).factor(a, n)
}

/// Solve a symmetric positive definite system using the factor from [dpofa].
pub fn dposl(a: &[f64], lda: usize, n: usize, b: &mut [f64]) {
    if fits(a.len(), lda, n, n) && b.len() >= n {
        UpperBand::dense(lda, n).solve(a, &mut b[..n]);
    }
}

/// Factor an upper band with `m` superdiagonals. Row `m` holds the diagonal.
/// Return the one-based index of the first leading minor that fails, or zero.
pub fn dpbfa(abd: &mut [f64], lda: usize, n: usize, m: usize) -> i32 {
    if m >= lda || !fits(abd.len(), lda, m + 1, n) {
        return -1;
    }
    UpperBand {
        ld: lda,
        width: m,
        band_row: Some(m),
    }
    .factor(abd, n)
}

/// Solve a symmetric positive definite band system using the factor from [dpbfa].
pub fn dpbsl(abd: &[f64], lda: usize, n: usize, m: usize, b: &mut [f64]) {
    if m < lda && fits(abd.len(), lda, m + 1, n) && b.len() >= n {
        UpperBand {
            ld: lda,
            width: m,
            band_row: Some(m),
        }
        .solve(abd, &mut b[..n]);
    }
}

/// Factor a symmetric positive definite matrix and estimate its reciprocal
/// one-norm condition number. On success, `z` holds a unit one-norm vector
/// that estimates a direction of small gain. Return `(rcond, info)`.
pub fn dpoco(a: &mut [f64], lda: usize, n: usize, z: &mut [f64]) -> (f64, i32) {
    if !fits(a.len(), lda, n, n) || z.len() < n {
        return (0.0, -1);
    }
    if n == 0 {
        return (0.0, 0);
    }
    let max = (0..n)
        .flat_map(|j| a[j * lda..=j * lda + j].iter().copied())
        .map(f64::abs)
        .fold(0.0, f64::max);
    let z = &mut z[..n];
    if max > 0.0 && max.is_finite() {
        z.fill(0.0);
        for j in 0..n {
            z[j] += a[j * lda + j].abs() / max;
            for i in 0..j {
                let value = a[j * lda + i].abs() / max;
                z[i] += value;
                z[j] += value;
            }
        }
    }
    let norm = z.iter().copied().fold(0.0, f64::max);
    let info = dpofa(a, lda, n);
    if info != 0 {
        return (0.0, info);
    }
    let r = Triangle::new(a, lda, n, true, max.sqrt());
    grow_transposed_solution(r, z);
    r.scaled_solve(z, false);
    unit_one(z);
    let first = r.scaled_solve(z, true) * unit_one(z);
    let second = r.scaled_solve(z, false) * unit_one(z);
    ((first / norm * second).clamp(0.0, 1.0), 0)
}

/// Use the upper factor to compute a determinant, an inverse, or both.
/// A nonzero tens part of `job` requests the decimal-scaled determinant.
/// A nonzero units digit requests the inverse in the upper triangle.
pub fn dpodi(a: &mut [f64], lda: usize, n: usize, det: &mut [f64; 2], job: i32) {
    if !fits(a.len(), lda, n, n) {
        return;
    }
    if job / 10 != 0 {
        *det = [1.0, 0.0];
        for j in 0..n {
            let mut factor = [a[j * lda + j].abs(), 0.0];
            if factor[0] == 0.0 || !factor[0].is_finite() {
                *det = factor;
                break;
            }
            decimal_normalize(&mut factor);
            det[0] *= factor[0] * factor[0];
            det[1] += 2.0 * factor[1];
            decimal_normalize(det);
        }
    }
    if job % 10 == 0 || (0..n).any(|j| a[j * lda + j] == 0.0) {
        return;
    }

    // Compute R⁻¹ before the product overwrites the factor.
    for j in 0..n {
        let start = j * lda;
        let reciprocal = 1.0 / a[start + j];
        for i in 0..j {
            let dot = (i..j).map(|k| a[k * lda + i] * a[start + k]).sum::<f64>();
            a[start + i] = -dot * reciprocal;
        }
        a[start + j] = reciprocal;
    }
    for j in 0..n {
        let start = j * lda;
        for k in 0..j {
            let scale = a[start + k];
            for i in 0..=k {
                a[k * lda + i] += a[start + i] * scale;
            }
        }
        let diagonal = a[start + j];
        for entry in &mut a[start..=start + j] {
            *entry *= diagonal;
        }
    }
}

fn decimal_normalize(value: &mut [f64; 2]) {
    while value[0] < 1.0 {
        value[0] *= 10.0;
        value[1] -= 1.0;
    }
    while value[0] >= 10.0 {
        value[0] /= 10.0;
        value[1] += 1.0;
    }
}

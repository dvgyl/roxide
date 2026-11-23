//! LU, Cholesky, triangular solves, and column-pivoted QR.

#![deny(missing_docs)]

mod complex;
mod lu;
mod qr;

pub use complex::{ComplexLu, zgetrf};
pub use lu::{Lu, dgetrf, matrix_norm_inf, matrix_norm_one};
pub use qr::{QrLapack, dgeqp3, dlarfg};

use super::linpack::{diagonal, zero_matrix};
use crate::{MatMut, MatRef, MathError};

fn square(a: MatRef<'_>) -> Result<usize, MathError> {
    if a.nrow != a.ncol {
        return Err(MathError::Dim);
    }
    MatRef::new(a.data, a.nrow, a.ncol)?;
    Ok(a.nrow)
}

fn first_zero(a: MatRef<'_>, k: usize) -> Option<usize> {
    (0..k).find(|&i| a.get(i, i) == 0.0).map(|i| i + 1)
}

/// Compute an upper Cholesky factor in place. Read only the upper triangle.
pub fn dpotrf_upper(a: &mut MatMut<'_>) -> Result<(), MathError> {
    let n = square(a.as_ref())?;
    for j in 0..n {
        let mut d = a.get(j, j);
        for i in 0..j {
            d = (-a.get(i, j)).mul_add(a.get(i, j), d);
        }
        if d <= 0.0 || d.is_nan() {
            return Err(MathError::NotPositiveDefinite { minor: j + 1 });
        }
        let root = d.sqrt();
        a.set(j, j, root);
        for k in j + 1..n {
            let mut v = a.get(j, k);
            for i in 0..j {
                v = (-a.get(i, j)).mul_add(a.get(i, k), v);
            }
            a.set(j, k, v / root);
        }
        a.col_mut(j)[j + 1..].fill(0.0);
    }
    Ok(())
}

fn max_index(range: std::ops::Range<usize>, score: impl Fn(usize) -> f64) -> Option<usize> {
    range.reduce(|i, j| if score(j) > score(i) { j } else { i })
}

/// Compute a pivoted upper Cholesky factor. Return zero-based pivots and rank.
pub fn dpstrf_upper(a: &mut MatMut<'_>, tol: f64) -> Result<(Vec<usize>, usize), MathError> {
    let n = square(a.as_ref())?;
    let mut pivots: Vec<_> = (0..n).collect();
    let mut sums = vec![0.0; n];
    let mut diagonal = vec![0.0; n];
    let mut cutoff = tol;
    let mut rank = n;
    for j in 0..n {
        for i in j..n {
            if j > 0 {
                let previous = a.get(j - 1, i);
                sums[i] = previous.mul_add(previous, sums[i]);
            }
            diagonal[i] = a.get(i, i) - sums[i];
        }
        let p = max_index(j..n, |i| diagonal[i]).ok_or(MathError::Dim)?;
        let d = diagonal[p];
        if j == 0 && tol < 0.0 {
            cutoff = (n as f64 * (f64::EPSILON / 2.0)) * d;
        }
        if d.is_nan() || (j == 0 && d <= 0.0) || (j > 0 && d <= cutoff) {
            rank = j;
            break;
        }
        if j != p {
            let dj = a.get(j, j);
            a.set(p, p, dj);
            for i in 0..j {
                a.data.swap(j * n + i, p * n + i);
            }
            for i in j + 1..p {
                a.data.swap(i * n + j, p * n + i);
            }
            for i in p + 1..n {
                a.data.swap(i * n + j, i * n + p);
            }
            pivots.swap(j, p);
            sums.swap(j, p);
        }
        let root = d.sqrt();
        a.set(j, j, root);
        for k in j + 1..n {
            let mut v = a.get(j, k);
            for i in 0..j {
                v = (-a.get(i, j)).mul_add(a.get(i, k), v);
            }
            a.set(j, k, v * (1.0 / root));
        }
    }
    for j in 0..n {
        a.col_mut(j)[rank.min(j + 1)..].fill(0.0);
    }
    Ok((pivots, rank))
}

/// Invert a positive definite matrix from its upper Cholesky factor.
pub fn dpotri_upper(r: MatRef<'_>) -> Result<Vec<f64>, MathError> {
    let n = square(r)?;
    let mut inverse = diagonal(n, n, None)?;
    dtrsm(true, false, r, &mut MatMut::new(&mut inverse, n, n)?, n)?;
    let mut out = zero_matrix(n, n)?;
    let inverse = MatRef::new(&inverse, n, n)?;
    super::blas::syrk_tcrossprod(inverse, &mut out)?;
    Ok(out)
}

/// Solve a triangular system using the first `k` rows of each response.
pub fn dtrsm(
    upper: bool,
    trans: bool,
    r: MatRef<'_>,
    b: &mut MatMut<'_>,
    k: usize,
) -> Result<(), MathError> {
    if k > r.nrow.min(r.ncol).min(b.nrow) {
        return Err(MathError::Dim);
    }
    MatRef::new(r.data, r.nrow, r.ncol)?;
    MatRef::new(b.data, b.nrow, b.ncol)?;
    if let Some(info) = first_zero(r, k) {
        return Err(MathError::Singular { info });
    }
    for j in 0..b.ncol {
        triangular(upper, trans, false, r, &mut b.col_mut(j)[..k]);
    }
    Ok(())
}

fn triangular(upper: bool, trans: bool, unit: bool, a: MatRef<'_>, x: &mut [f64]) {
    let n = x.len();
    let reverse = upper != trans;
    for step in 0..n {
        let i = if reverse { n - 1 - step } else { step };
        if trans {
            let range = if upper { 0..i } else { i + 1..n };
            let mut value = x[i];
            for row in range {
                value = (-a.get(row, i)).mul_add(x[row], value);
            }
            x[i] = if unit { value } else { value / a.get(i, i) };
        } else if x[i] != 0.0 {
            if !unit {
                x[i] /= a.get(i, i);
            }
            let range = if upper { 0..i } else { i + 1..n };
            for row in range {
                x[row] = (-a.get(row, i)).mul_add(x[i], x[row]);
            }
        }
    }
}

//! Limited-pivoting QR factorization and least squares.

#![deny(missing_docs)]

use crate::{MatRef, MathError};

/// Packed LINPACK factors of a column-major matrix.
#[derive(Debug)]
pub struct QrLinpack {
    /// Upper trapezoid and Householder reflector tails.
    pub qr: Vec<f64>,
    /// Number of input rows.
    pub nrow: usize,
    /// Number of input columns.
    pub ncol: usize,
    /// Number of accepted columns, capped at the number of rows.
    pub rank: usize,
    /// First entry of each Householder reflector.
    pub qraux: Vec<f64>,
    /// Original zero-based index of each packed column.
    pub pivot: Vec<usize>,
    /// Relative cutoff for limited pivoting.
    pub tol: f64,
}

pub(super) fn norm(x: &[f64]) -> f64 {
    let sum = x.iter().fold(0.0, |sum, &v| v.mul_add(v, sum));
    if sum.is_normal() || x.iter().all(|&v| v == 0.0) {
        return sum.sqrt();
    }
    let scale = x.iter().map(|v| v.abs()).fold(0.0, f64::max);
    if !scale.is_finite() {
        return sum.sqrt();
    }
    scale
        * x.iter()
            .fold(0.0, |sum, &v| {
                let v = v / scale;
                v.mul_add(v, sum)
            })
            .sqrt()
}

/// Factor a matrix with limited pivoting and the given tolerance.
pub fn dqrdc2(mut x: Vec<f64>, nrow: usize, ncol: usize, tol: f64) -> Result<QrLinpack, MathError> {
    let view = MatRef::new(&x, nrow, ncol)?;
    let mut qraux = zero_matrix(ncol, 1)?;
    for (j, value) in qraux.iter_mut().enumerate() {
        *value = norm(view.col(j));
    }
    let mut original: Vec<_> = qraux
        .iter()
        .map(|&v| if v == 0.0 { 1.0 } else { v })
        .collect();
    let mut pivot: Vec<_> = (0..ncol).collect();
    let mut end = ncol;
    for l in 0..nrow.min(ncol) {
        while l < end && qraux[l] < tol * original[l] {
            x[l * nrow..].rotate_left(nrow);
            qraux[l..].rotate_left(1);
            original[l..].rotate_left(1);
            pivot[l..].rotate_left(1);
            end -= 1;
        }
        if l + 1 == nrow {
            break;
        }
        let (left, right) = x.split_at_mut((l + 1) * nrow);
        let v = &mut left[l * nrow + l..];
        qraux[l] = 0.0;
        let length = norm(v);
        if length == 0.0 {
            continue;
        }
        let s = if v[0] < 0.0 { -length } else { length };
        let inverse = 1.0 / s;
        for value in &mut *v {
            *value *= inverse;
        }
        v[0] += 1.0;
        for (col, current) in right.chunks_exact_mut(nrow).zip(&mut qraux[l + 1..]) {
            let y = &mut col[l..];
            let dot = v
                .iter()
                .zip(&*y)
                .fold(0.0, |sum, (&a, &b)| a.mul_add(b, sum));
            let t = -dot / v[0];
            for (b, &a) in y.iter_mut().zip(&*v) {
                *b = t.mul_add(a, *b);
            }
            if *current != 0.0 {
                let u = y[0].abs() / *current;
                let w = (-u).mul_add(u, 1.0).max(0.0);
                *current = if w >= 1e-6 {
                    *current * w.sqrt()
                } else {
                    norm(&y[1..])
                };
            }
        }
        qraux[l] = v[0];
        v[0] = -s;
    }
    Ok(QrLinpack {
        qr: x,
        nrow,
        ncol,
        rank: nrow.min(end),
        qraux,
        pivot,
        tol,
    })
}

pub(super) fn zero_matrix(rows: usize, cols: usize) -> Result<Vec<f64>, MathError> {
    let len = rows.checked_mul(cols).ok_or(MathError::TooLarge)?;
    let mut out = Vec::new();
    out.try_reserve_exact(len)
        .map_err(|_| MathError::TooLarge)?;
    out.resize(len, 0.0);
    Ok(out)
}

pub(super) fn diagonal(
    rows: usize,
    cols: usize,
    scales: Option<&[f64]>,
) -> Result<Vec<f64>, MathError> {
    let mut out = zero_matrix(rows, cols)?;
    for i in 0..rows.min(cols) {
        out[i * (rows + 1)] = scales.map_or(1.0, |v| v.get(i).copied().unwrap_or(f64::NAN));
    }
    Ok(out)
}

pub(super) fn upper(a: MatRef<'_>, rows: usize) -> Result<Vec<f64>, MathError> {
    let mut out = zero_matrix(rows, a.ncol)?;
    for j in 0..a.ncol {
        let height = rows.min(j + 1);
        out[j * rows..j * rows + height].copy_from_slice(&a.col(j)[..height]);
    }
    Ok(out)
}

impl QrLinpack {
    fn factors(&self) -> Result<MatRef<'_>, MathError> {
        if self.qraux.len() != self.ncol
            || self.pivot.len() != self.ncol
            || self.rank > self.nrow.min(self.ncol)
            || self.pivot.iter().any(|&p| p >= self.ncol)
        {
            return Err(MathError::Dim);
        }
        MatRef::new(&self.qr, self.nrow, self.ncol)
    }

    fn apply(&self, y: &mut [f64], trans: bool, count: usize) -> Result<(), MathError> {
        let qr = self.factors()?;
        if qr.nrow == 0 {
            return Ok(());
        }
        let count = count.min(qr.nrow - 1).min(qr.ncol);
        for col in y.chunks_exact_mut(qr.nrow) {
            for i in 0..count {
                let j = if trans { i } else { count - 1 - i };
                let lead = self.qraux[j];
                if lead == 0.0 {
                    continue;
                }
                let tail = &qr.col(j)[j + 1..];
                let dot = tail
                    .iter()
                    .zip(&col[j + 1..])
                    .fold(lead * col[j], |sum, (&a, &b)| a.mul_add(b, sum));
                let t = -dot / lead;
                col[j] = t.mul_add(lead, col[j]);
                for (b, &a) in col[j + 1..].iter_mut().zip(tail) {
                    *b = t.mul_add(a, *b);
                }
            }
        }
        Ok(())
    }

    fn response(&self, y: MatRef<'_>) -> Result<Vec<f64>, MathError> {
        if self.nrow != y.nrow {
            return Err(MathError::Dim);
        }
        MatRef::new(y.data, y.nrow, y.ncol)?;
        Ok(y.data.to_vec())
    }

    /// Multiply by Q, or by its transpose when `trans` is true.
    pub fn qy(&self, y: MatRef<'_>, trans: bool) -> Result<Vec<f64>, MathError> {
        let mut out = self.response(y)?;
        self.apply(&mut out, trans, self.rank)?;
        Ok(out)
    }

    /// Form the full or reduced Q, with optional column scales.
    pub fn q_mat(&self, complete: bool, dvec: Option<&[f64]>) -> Result<Vec<f64>, MathError> {
        let cols = if complete {
            self.nrow
        } else {
            self.nrow.min(self.ncol)
        };
        let mut q = diagonal(self.nrow, cols, dvec)?;
        self.apply(&mut q, false, self.rank)?;
        Ok(q)
    }

    /// Extract the upper trapezoid, with optional zero rows.
    pub fn r_mat(&self, complete: bool) -> Result<Vec<f64>, MathError> {
        let qr = self.factors()?;
        upper(
            qr,
            if complete {
                self.nrow
            } else {
                self.nrow.min(self.ncol)
            },
        )
    }

    fn coefficients(
        &self,
        effects: &[f64],
        cols: usize,
        original: bool,
    ) -> Result<Vec<f64>, MathError> {
        let qr = self.factors()?;
        let mut out = zero_matrix(self.ncol, cols)?;
        for j in 0..cols {
            let mut active = effects[j * self.nrow..j * self.nrow + self.rank].to_vec();
            for i in (0..self.rank).rev() {
                let d = qr.get(i, i);
                if d == 0.0 {
                    return Err(MathError::Singular { info: i + 1 });
                }
                active[i] /= d;
                for row in 0..i {
                    active[row] = (-active[i]).mul_add(qr.get(row, i), active[row]);
                }
            }
            if original {
                let col = &mut out[j * self.ncol..(j + 1) * self.ncol];
                col.fill(f64::NAN);
                for (i, value) in active.into_iter().enumerate() {
                    col[self.pivot[i]] = value;
                }
            } else {
                out[j * self.ncol..j * self.ncol + self.rank].copy_from_slice(&active);
            }
        }
        Ok(out)
    }

    /// Fit coefficients in original column order. Aliased entries are NaN.
    pub fn coef(&self, y: MatRef<'_>) -> Result<Vec<f64>, MathError> {
        let mut effects = self.response(y)?;
        self.apply(&mut effects, true, self.rank)?;
        self.coefficients(&effects, y.ncol, true)
    }

    fn project(&self, mut effects: Vec<f64>, fitted: bool) -> Result<Vec<f64>, MathError> {
        if self.nrow != 0 {
            for col in effects.chunks_exact_mut(self.nrow) {
                if fitted {
                    col[self.rank..].fill(0.0);
                } else {
                    col[..self.rank].fill(0.0);
                }
            }
        }
        self.apply(&mut effects, false, self.rank)?;
        Ok(effects)
    }

    /// Return the part of each response outside the fitted subspace.
    pub fn resid(&self, y: MatRef<'_>) -> Result<Vec<f64>, MathError> {
        let mut effects = self.response(y)?;
        self.apply(&mut effects, true, self.rank)?;
        self.project(effects, false)
    }

    /// Return the projection of each response onto the fitted subspace.
    pub fn fitted(&self, y: MatRef<'_>) -> Result<Vec<f64>, MathError> {
        let mut effects = self.response(y)?;
        self.apply(&mut effects, true, self.rank)?;
        self.project(effects, true)
    }
}

/// Factorization and results from a least-squares fit.
#[derive(Debug)]
pub struct LsFit {
    /// Packed factorization of the design matrix.
    pub qr: QrLinpack,
    /// Coefficients in pivot order, with zeros for aliased columns.
    pub coefficients: Vec<f64>,
    /// Residuals for each response.
    pub residuals: Vec<f64>,
    /// Transformed responses in the Householder basis.
    pub effects: Vec<f64>,
    /// Responses minus residuals.
    pub fitted: Vec<f64>,
}

/// Factor the design matrix and fit one or more response columns.
pub fn dqrls(
    x: Vec<f64>,
    nrow: usize,
    ncol: usize,
    y: MatRef<'_>,
    tol: f64,
) -> Result<LsFit, MathError> {
    let qr = dqrdc2(x, nrow, ncol, tol)?;
    let mut effects = qr.response(y)?;
    qr.apply(&mut effects, true, qr.rank)?;
    let coefficients = qr.coefficients(&effects, y.ncol, false)?;
    let residuals = qr.project(effects.clone(), false)?;
    let fitted = y.data.iter().zip(&residuals).map(|(y, r)| y - r).collect();
    Ok(LsFit {
        qr,
        coefficients,
        residuals,
        effects,
        fitted,
    })
}

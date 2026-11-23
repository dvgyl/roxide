use crate::{MatMut, MatRef, MathError};

use super::{dtrsm, max_index};
use crate::linalg::linpack::{diagonal, norm, upper, zero_matrix};

/// Construct a Householder reflector in place and return its multiplier.
/// The first entry becomes the transformed scalar and the tail stores the reflector.
pub fn dlarfg(alpha: &mut f64, tail: &mut [f64]) -> f64 {
    let length = norm(tail);
    if length == 0.0 {
        return 0.0;
    }
    let mut beta = -hypot(*alpha, length).copysign(*alpha);
    let safe = f64::MIN_POSITIVE / (f64::EPSILON / 2.0);
    let mut count = 0;
    while beta.abs() < safe && count < 20 {
        count += 1;
        for v in &mut *tail {
            *v *= 1.0 / safe;
        }
        *alpha *= 1.0 / safe;
        beta *= 1.0 / safe;
    }
    if count != 0 {
        beta = -hypot(*alpha, norm(tail)).copysign(*alpha);
    }
    let tau = (beta - *alpha) / beta;
    let inverse = 1.0 / (*alpha - beta);
    for v in tail {
        *v *= inverse;
    }
    *alpha = beta;
    for _ in 0..count {
        *alpha *= safe;
    }
    tau
}

fn hypot(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return a + b;
    }
    let max = a.abs().max(b.abs());
    let min = a.abs().min(b.abs());
    if min == 0.0 || max.is_infinite() {
        return max;
    }
    let ratio = min / max;
    max * ratio.mul_add(ratio, 1.0).sqrt()
}

/// Packed factors of a LAPACK column-pivoted QR decomposition.
#[derive(Debug)]
pub struct QrLapack {
    /// Upper trapezoid and Householder reflector tails.
    pub qr: Vec<f64>,
    /// Number of rows.
    pub nrow: usize,
    /// Number of columns.
    pub ncol: usize,
    /// Number of factorization steps, as reported by R.
    pub rank: usize,
    /// Householder multipliers, one for each factorization step.
    pub qraux: Vec<f64>,
    /// Original zero-based index of each packed column.
    pub pivot: Vec<usize>,
}

/// Factor a matrix with full column pivoting.
pub fn dgeqp3(mut x: Vec<f64>, nrow: usize, ncol: usize) -> Result<QrLapack, MathError> {
    let a = MatRef::new(&x, nrow, ncol)?;
    let rank = nrow.min(ncol);
    let mut qraux = vec![0.0; rank];
    let mut norms = zero_matrix(ncol, 1)?;
    for (j, value) in norms.iter_mut().enumerate() {
        *value = norm(a.col(j));
    }
    let mut pivot: Vec<_> = (0..ncol).collect();
    let mut reference = norms.clone();
    for (j, tau) in qraux.iter_mut().enumerate() {
        let p = max_index(j..ncol, |i| norms[i]).ok_or(MathError::Dim)?;
        if j != p {
            for i in 0..nrow {
                x.swap(j * nrow + i, p * nrow + i);
            }
            pivot.swap(j, p);
            norms.swap(j, p);
            reference.swap(j, p);
        }
        let (done, next) = x.split_at_mut((j + 1) * nrow);
        let (alpha, tail) = done[j * nrow + j..]
            .split_first_mut()
            .ok_or(MathError::Dim)?;
        *tau = dlarfg(alpha, tail);
        for (offset, col) in next.chunks_exact_mut(nrow).enumerate() {
            apply_reflector(*tau, tail, &mut col[j..]);
            let k = j + 1 + offset;
            let old = norms[k];
            if old != 0.0 {
                let ratio = col[j].abs() / old;
                let left = (-ratio).mul_add(ratio, 1.0).max(0.0);
                if left * (old / reference[k]).powi(2) <= f64::EPSILON.sqrt() {
                    norms[k] = norm(&col[j + 1..]);
                    reference[k] = norms[k];
                } else {
                    norms[k] *= left.sqrt();
                }
            }
        }
    }
    Ok(QrLapack {
        qr: x,
        nrow,
        ncol,
        rank,
        qraux,
        pivot,
    })
}

fn apply_reflector(tau: f64, tail: &[f64], y: &mut [f64]) {
    if tau == 0.0 {
        return;
    }
    let len = tail.iter().rposition(|&v| v != 0.0).map_or(0, |i| i + 1);
    if len == 0 {
        y[0] *= 1.0 - tau;
        return;
    }
    let tail = &tail[..len];
    let dot = tail
        .iter()
        .zip(&y[1..])
        .fold(0.0, |sum, (&a, &b)| a.mul_add(b, sum))
        + y[0];
    let t = -tau * dot;
    y[0] = (-tau).mul_add(dot, y[0]);
    for (&v, entry) in tail.iter().zip(&mut y[1..]) {
        *entry = t.mul_add(v, *entry);
    }
}

impl QrLapack {
    fn factors(&self) -> Result<MatRef<'_>, MathError> {
        if self.rank != self.nrow.min(self.ncol)
            || self.qraux.len() != self.rank
            || self.pivot.len() != self.ncol
            || self.pivot.iter().any(|&p| p >= self.ncol)
        {
            return Err(MathError::Dim);
        }
        MatRef::new(&self.qr, self.nrow, self.ncol)
    }

    fn apply(&self, out: &mut [f64], trans: bool) -> Result<(), MathError> {
        let qr = self.factors()?;
        if qr.nrow == 0 {
            return Ok(());
        }
        for col in out.chunks_exact_mut(qr.nrow) {
            for i in 0..self.rank {
                let j = if trans { i } else { self.rank - 1 - i };
                apply_reflector(self.qraux[j], &qr.col(j)[j + 1..], &mut col[j..]);
            }
        }
        Ok(())
    }

    /// Multiply by Q, or by its transpose when `trans` is true.
    pub fn qy(&self, y: MatRef<'_>, trans: bool) -> Result<Vec<f64>, MathError> {
        if self.nrow != y.nrow {
            return Err(MathError::Dim);
        }
        MatRef::new(y.data, y.nrow, y.ncol)?;
        let mut out = y.data.to_vec();
        self.apply(&mut out, trans)?;
        Ok(out)
    }

    /// Form the full or reduced Q, with optional column scales.
    pub fn q_mat(&self, complete: bool, dvec: Option<&[f64]>) -> Result<Vec<f64>, MathError> {
        let mut q = diagonal(
            self.nrow,
            if complete { self.nrow } else { self.rank },
            dvec,
        )?;
        self.apply(&mut q, false)?;
        Ok(q)
    }

    /// Extract the upper trapezoid, with optional zero rows.
    pub fn r_mat(&self, complete: bool) -> Result<Vec<f64>, MathError> {
        upper(
            self.factors()?,
            if complete { self.nrow } else { self.rank },
        )
    }

    /// Solve for the coefficients in original column order.
    pub fn coef(&self, y: MatRef<'_>) -> Result<Vec<f64>, MathError> {
        let qr = self.factors()?;
        let mut effects = self.qy(y, true)?;
        dtrsm(
            true,
            false,
            qr,
            &mut MatMut::new(&mut effects, y.nrow, y.ncol)?,
            self.rank,
        )?;
        let mut out = zero_matrix(self.ncol, y.ncol)?;
        for j in 0..y.ncol {
            for i in 0..self.rank {
                out[j * self.ncol + self.pivot[i]] = effects[j * self.nrow + i];
            }
        }
        Ok(out)
    }

    fn project(&self, y: MatRef<'_>, fitted: bool) -> Result<Vec<f64>, MathError> {
        let mut effects = self.qy(y, true)?;
        if self.nrow != 0 {
            for col in effects.chunks_exact_mut(self.nrow) {
                if fitted {
                    col[self.rank..].fill(0.0);
                } else {
                    col[..self.rank].fill(0.0);
                }
            }
        }
        self.apply(&mut effects, false)?;
        Ok(effects)
    }

    /// Return the part of each response outside the fitted subspace.
    pub fn resid(&self, y: MatRef<'_>) -> Result<Vec<f64>, MathError> {
        self.project(y, false)
    }

    /// Return the projection of each response onto the fitted subspace.
    pub fn fitted(&self, y: MatRef<'_>) -> Result<Vec<f64>, MathError> {
        self.project(y, true)
    }
}

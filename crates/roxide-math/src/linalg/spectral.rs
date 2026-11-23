//! Real spectral decompositions and matrix norms.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod condition;
mod general;
mod singular;
mod symmetric;

use crate::{MatRef, MathError};

pub use condition::triangular_condition;
pub use general::{GeneralEigen, general_eigen};
pub use singular::{Svd, svd};
pub use symmetric::{SymmetricEigen, symmetric_eigen};

/// Matrix norm to compute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixNorm {
    /// Largest absolute column sum.
    One,
    /// Largest absolute row sum.
    Infinity,
    /// Euclidean norm of all entries.
    Frobenius,
    /// Largest absolute entry.
    Maximum,
    /// Largest singular value.
    Spectral,
}

/// Compute a matrix norm. An empty matrix has norm zero.
///
/// The first NaN is returned unchanged. This lets callers preserve missing values.
/// A nonfinite input to the spectral decomposition returns an error.
pub fn matrix_norm(a: MatRef<'_>, kind: MatrixNorm) -> Result<f64, MathError> {
    MatRef::new(a.data, a.nrow, a.ncol)?;
    if let Some(&nan) = a.data.iter().find(|x| x.is_nan()) {
        return Ok(nan);
    }
    if a.data.is_empty() {
        return Ok(0.0);
    }
    Ok(match kind {
        MatrixNorm::One => super::factor::matrix_norm_one(a),
        MatrixNorm::Infinity => super::factor::matrix_norm_inf(a),
        MatrixNorm::Frobenius => length(a.data.iter().copied()),
        MatrixNorm::Maximum => max_abs(a.data.iter().copied()),
        MatrixNorm::Spectral => svd(a, 0, 0)?.d[0],
    })
}

fn area<T>(rows: usize, cols: usize) -> Result<usize, MathError> {
    rows.checked_mul(cols)
        .filter(|&n| n <= isize::MAX as usize / size_of::<T>())
        .ok_or(MathError::TooLarge)
}

fn eye(rows: usize, cols: usize) -> Result<Vec<f64>, MathError> {
    let mut a = vec![0.0; area::<f64>(rows, cols)?];
    for i in 0..rows.min(cols) {
        a[i * (rows + 1)] = 1.0;
    }
    Ok(a)
}

fn max_abs(values: impl Iterator<Item = f64>) -> f64 {
    values.map(f64::abs).fold(0.0, f64::max)
}

fn length(values: impl Iterator<Item = f64>) -> f64 {
    values.fold(0.0, f64::hypot)
}

// Use power-of-two scaling to keep rounding out of the initial normalization.
fn scale(values: impl Iterator<Item = f64>) -> Result<f64, MathError> {
    let mut max: f64 = 0.0;
    for x in values {
        if !x.is_finite() {
            return Err(MathError::NoConvergence { info: 1 });
        }
        max = max.max(x.abs());
    }
    let exponent = ((max.to_bits() >> 52) & 0x7ff) as i32 - 1023;
    // Leave room for products and workspace growth. For large inputs,
    // keep the factor small enough to preserve much smaller matrix entries.
    let shift = match exponent {
        _ if max == 0.0 => 0,
        i if i > 450 => i - 450,
        i if i < -450 => i.max(-1022),
        _ => 0,
    };
    Ok(2.0_f64.powi(shift))
}

fn rotate(a: &mut [f64], rows: usize, x: usize, y: usize, c: f64, s: f64) {
    if a.is_empty() {
        return;
    }
    for i in 0..rows {
        let ix = x * rows + i;
        let iy = y * rows + i;
        let (u, v) = (a[ix], a[iy]);
        a[ix] = c * u + s * v;
        a[iy] = c * v - s * u;
    }
}

fn givens(x: f64, y: f64) -> (f64, f64, f64) {
    let r = x.hypot(y);
    if r == 0.0 {
        (1.0, 0.0, 0.0)
    } else {
        (x / r, y / r, r)
    }
}

fn reflect(tau: f64, tail: &[f64], x: &mut [f64]) {
    if tau != 0.0 {
        let dot = x[0] + tail.iter().zip(&x[1..]).map(|(u, v)| u * v).sum::<f64>();
        let w = tau * dot;
        x[0] -= w;
        for (v, u) in x[1..].iter_mut().zip(tail) {
            *v -= w * u;
        }
    }
}

// Apply a Householder reflector to consecutive columns.
fn reflect_right(a: &mut [f64], rows: usize, row_end: usize, start: usize, tau: f64, tail: &[f64]) {
    if tau == 0.0 || a.is_empty() {
        return;
    }
    for i in 0..row_end {
        let first = start * rows + i;
        let dot = a[first]
            + tail
                .iter()
                .enumerate()
                .map(|(j, v)| v * a[first + (j + 1) * rows])
                .sum::<f64>();
        let w = tau * dot;
        a[first] -= w;
        for (j, v) in tail.iter().enumerate() {
            a[first + (j + 1) * rows] -= w * v;
        }
    }
}

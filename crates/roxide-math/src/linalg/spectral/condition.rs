//! Estimate the one-norm condition number of a triangular matrix.

use crate::{MatRef, MathError};

/// Estimate the condition number by solving two triangular systems.
/// If `transpose` is true, select the triangle after transposing the input.
pub fn triangular_condition(a: MatRef<'_>, upper: bool, transpose: bool) -> Result<f64, MathError> {
    let n = a.nrow;
    MatRef::new(a.data, n, a.ncol)?;
    if n != a.ncol {
        return Err(MathError::Dim);
    }
    if n == 0 {
        return Ok(0.0);
    }
    let t = |i, j| if transpose { a.get(j, i) } else { a.get(i, j) };
    let indices = |k| if upper { k } else { n - 1 - k };
    let t = |i, j| {
        let (i, j) = (indices(i), indices(j));
        t(i, j)
    };
    if (0..n).any(|i| t(i, i) == 0.0) {
        return Ok(f64::INFINITY);
    }
    let mut max: f64 = 0.0;
    for j in 0..n {
        for i in 0..=j {
            let v: f64 = t(i, j);
            if !v.is_finite() {
                return Err(MathError::NoConvergence { info: 1 });
            }
            max = max.max(v.abs());
        }
    }
    let t = |i, j| t(i, j) / max;
    if (0..n).any(|i| t(i, i) == 0.0) {
        return Ok(f64::INFINITY);
    }
    let norm = (0..n)
        .map(|j| (0..=j).map(|i| t(i, j).abs()).sum::<f64>())
        .fold(0.0, f64::max);

    // Select signs in T^T y = d to increase the one-norm of the solution.
    let mut y: Vec<f64> = vec![0.0; n];
    let mut weight: f64 = 1.0;
    for j in 0..n {
        let diag = t(j, j);
        if y[j] != 0.0 {
            weight = weight.copysign(-y[j]);
        }
        let delta = weight - y[j];
        if delta.abs() > diag.abs() {
            let factor = diag.abs() / delta.abs();
            for v in &mut y {
                *v *= factor;
            }
            weight *= factor;
        }
        let rhs = weight - y[j];
        let rhs_alt = -weight - y[j];
        let w = rhs / diag;
        let alt = rhs_alt / diag;
        let growth = |w: f64| (j + 1..n).map(|i| (y[i] + w * t(j, i)).abs()).sum::<f64>();
        let w = if rhs_alt.abs() + growth(alt) > rhs.abs() + growth(w) {
            alt
        } else {
            w
        };
        y[j] = w;
        for (i, v) in y.iter_mut().enumerate().skip(j + 1) {
            *v += w * t(j, i);
        }
    }

    // Solve T x = y with scaling when a diagonal entry is small.
    let sum = |x: &[f64]| x.iter().map(|v| v.abs()).sum::<f64>();
    let mut rhs = sum(&y);
    for j in (0..n).rev() {
        let diag = t(j, j);
        if y[j].abs() > diag.abs() {
            let factor = diag.abs() / y[j].abs();
            for v in &mut y {
                *v *= factor;
            }
            rhs *= factor;
        }
        y[j] /= diag;
        let w = y[j];
        for (i, v) in y.iter_mut().enumerate().take(j) {
            *v -= w * t(i, j);
        }
    }
    Ok(norm * (sum(&y) / rhs))
}

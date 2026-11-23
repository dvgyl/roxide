use crate::{MatMut, MatRef, MathError};

use super::{first_zero, max_index, triangular};

/// Packed LU factors and row interchanges.
#[derive(Debug)]
pub struct Lu {
    /// Upper factor and strict lower triangle of the unit lower factor.
    pub lu: Vec<f64>,
    /// Order of the original square matrix.
    pub n: usize,
    /// Zero-based row interchanges in order of application.
    pub ipiv: Vec<usize>,
    /// Sign of the product of the row interchanges.
    pub sign: i8,
}

/// Factor a square matrix using partial row pivoting, including singular matrices.
pub fn dgetrf(mut a: Vec<f64>, n: usize) -> Result<Lu, MathError> {
    MatRef::new(&a, n, n)?;
    let mut ipiv: Vec<_> = (0..n).collect();
    let block = if n > 64 { 64 } else { n.max(1) };
    for start in (0..n).step_by(block) {
        let end = n.min(start + block);
        factor(&mut a, &mut ipiv, n, start, end);
        swap_rows(&mut a, &ipiv, n, start..end, 0..start);
        update(&mut a, &ipiv, n, start, end, n);
    }
    let sign = if ipiv.iter().enumerate().filter(|&(i, &p)| i != p).count() % 2 == 0 {
        1
    } else {
        -1
    };
    Ok(Lu {
        lu: a,
        n,
        ipiv,
        sign,
    })
}

fn factor(a: &mut [f64], ipiv: &mut [usize], n: usize, start: usize, end: usize) {
    if end - start == 1 {
        let i = start;
        let col = &mut a[i * n..(i + 1) * n];
        let p = max_index(i..n, |j| col[j].abs()).unwrap_or(i);
        ipiv[i] = p;
        if col[p] == 0.0 {
            return;
        }
        col.swap(i, p);
        let d = col[i];
        if d.abs() >= f64::MIN_POSITIVE {
            let inverse = 1.0 / d;
            for x in &mut col[i + 1..] {
                *x *= inverse;
            }
        } else {
            for x in &mut col[i + 1..] {
                *x /= d;
            }
        }
        return;
    }
    let mid = start + (end - start) / 2;
    factor(a, ipiv, n, start, mid);
    update(a, ipiv, n, start, mid, end);
    factor(a, ipiv, n, mid, end);
    swap_rows(a, ipiv, n, mid..end, start..mid);
}

fn swap_rows(
    a: &mut [f64],
    ipiv: &[usize],
    n: usize,
    rows: std::ops::Range<usize>,
    cols: std::ops::Range<usize>,
) {
    for j in cols {
        let col = &mut a[j * n..(j + 1) * n];
        for i in rows.clone() {
            col.swap(i, ipiv[i]);
        }
    }
}

fn update(a: &mut [f64], ipiv: &[usize], n: usize, start: usize, mid: usize, end: usize) {
    swap_rows(a, ipiv, n, start..mid, mid..end);
    let (lower, right) = a.split_at_mut(mid * n);
    for col in right[..(end - mid) * n].chunks_exact_mut(n) {
        for i in start..mid {
            let value = col[i];
            if value != 0.0 {
                for row in i + 1..mid {
                    col[row] = (-value).mul_add(lower[i * n + row], col[row]);
                }
            }
        }
        for i in start..mid {
            let value = -col[i];
            for row in mid..n {
                col[row] = value.mul_add(lower[i * n + row], col[row]);
            }
        }
    }
}

impl Lu {
    fn view(&self) -> Result<MatRef<'_>, MathError> {
        MatRef::new(&self.lu, self.n, self.n)
    }

    /// Return the first zero diagonal position, using one-based indices.
    pub fn zero_diag_index(&self) -> Option<usize> {
        first_zero(self.view().ok()?, self.n)
    }

    /// Apply the row interchanges and solve both triangular systems in place.
    pub fn solve(&self, b: &mut MatMut<'_>) -> Result<(), MathError> {
        let a = self.view()?;
        if b.nrow != self.n || self.ipiv.len() != self.n || self.ipiv.iter().any(|&p| p >= self.n) {
            return Err(MathError::Dim);
        }
        MatRef::new(b.data, b.nrow, b.ncol)?;
        if let Some(info) = first_zero(a, self.n) {
            return Err(MathError::Singular { info });
        }
        for j in 0..b.ncol {
            let x = b.col_mut(j);
            for (i, &p) in self.ipiv.iter().enumerate() {
                x.swap(i, p);
            }
            triangular(false, false, true, a, x);
            triangular(true, false, false, a, x);
        }
        Ok(())
    }

    /// Estimate the reciprocal one-norm or infinity-norm condition number.
    pub fn rcond(&self, anorm: f64, one_norm: bool) -> f64 {
        let Ok(a) = self.view() else {
            return f64::NAN;
        };
        if self.n == 0 {
            return 1.0;
        }
        if anorm == 0.0 || anorm.is_infinite() || first_zero(a, self.n).is_some() {
            return 0.0;
        }
        let ainv = inverse_norm(a, one_norm);
        if ainv == 0.0 {
            0.0
        } else {
            (1.0 / ainv) / anorm
        }
    }

    /// Return the log absolute determinant and its sign.
    pub fn log_modulus(&self) -> (f64, i8) {
        let Ok(a) = self.view() else {
            return (f64::NAN, 1);
        };
        if first_zero(a, self.n).is_some() {
            return (f64::NEG_INFINITY, 1);
        }
        (0..self.n).fold((0.0, self.sign), |(log, sign), i| {
            let v = a.get(i, i);
            (log + v.abs().ln(), if v < 0.0 { -sign } else { sign })
        })
    }
}

fn inverse_norm(a: MatRef<'_>, one: bool) -> f64 {
    let n = a.nrow;
    let mut x = vec![1.0 / n as f64; n];
    let apply = |x: &mut [f64], transpose: bool| {
        if one != transpose {
            triangular(false, false, true, a, x);
            triangular(true, false, false, a, x);
        } else {
            triangular(true, true, false, a, x);
            triangular(false, true, true, a, x);
        }
        x.iter().all(|v| v.abs() <= 1.0 / f64::MIN_POSITIVE)
    };
    let sum = |x: &[f64]| x.iter().map(|v| v.abs()).sum::<f64>();
    let sign = |x: f64| if x >= 0.0 { 1.0 } else { -1.0 };
    let largest = |x: &[f64]| max_index(0..n, |i| x[i].abs()).unwrap_or(0);
    if !apply(&mut x, false) {
        return f64::INFINITY;
    }
    let mut estimate = sum(&x);
    if n == 1 {
        return estimate;
    }
    let mut signs: Vec<_> = x.iter().map(|&v| sign(v)).collect();
    x.copy_from_slice(&signs);
    if !apply(&mut x, true) {
        return f64::INFINITY;
    }
    let mut j = largest(&x);
    for _ in 2..=5 {
        x.fill(0.0);
        x[j] = 1.0;
        if !apply(&mut x, false) {
            return f64::INFINITY;
        }
        let previous = estimate;
        estimate = sum(&x);
        if x.iter().zip(&signs).all(|(&v, &s)| sign(v) == s) || estimate <= previous {
            break;
        }
        for (s, v) in signs.iter_mut().zip(&mut x) {
            *s = sign(*v);
            *v = *s;
        }
        if !apply(&mut x, true) {
            return f64::INFINITY;
        }
        let p = largest(&x);
        if x[j] == x[p].abs() {
            break;
        }
        j = p;
    }
    for (i, v) in x.iter_mut().enumerate() {
        *v = (if i % 2 == 0 { 1.0 } else { -1.0 }) * (1.0 + i as f64 / (n - 1) as f64);
    }
    if !apply(&mut x, false) {
        return f64::INFINITY;
    }
    let alternate = 2.0 * (sum(&x) / (3.0 * n as f64));
    if estimate.is_nan() || alternate.is_nan() {
        f64::INFINITY
    } else {
        estimate.max(alternate)
    }
}

fn max_sum(sums: impl Iterator<Item = f64>) -> f64 {
    sums.fold(0.0, |max, v| {
        if v.is_nan() || max.is_nan() {
            f64::NAN
        } else {
            max.max(v)
        }
    })
}

/// Return the largest absolute column sum.
pub fn matrix_norm_one(a: MatRef<'_>) -> f64 {
    if MatRef::new(a.data, a.nrow, a.ncol).is_err() {
        return f64::NAN;
    }
    if a.nrow == 0 {
        return 0.0;
    }
    max_sum(
        a.data
            .chunks_exact(a.nrow)
            .map(|c| c.iter().map(|v| v.abs()).sum()),
    )
}

/// Return the largest absolute row sum.
pub fn matrix_norm_inf(a: MatRef<'_>) -> f64 {
    if MatRef::new(a.data, a.nrow, a.ncol).is_err() {
        return f64::NAN;
    }
    if a.ncol == 0 {
        return 0.0;
    }
    max_sum((0..a.nrow).map(|i| (0..a.ncol).map(|j| a.get(i, j).abs()).sum()))
}

//! Column-major matrix products.

#![deny(missing_docs)]

use crate::{Cplx, MatMut, MatRef, MathError};

fn shape(rows: usize, cols: usize, trans: bool) -> (usize, usize) {
    if trans { (cols, rows) } else { (rows, cols) }
}

fn size(rows: usize, cols: usize, len: usize) -> Result<(), MathError> {
    if rows.checked_mul(cols).ok_or(MathError::TooLarge)? != len {
        return Err(MathError::Dim);
    }
    Ok(())
}

/// Compute `C = alpha * op(A) * op(B) + beta * C`.
pub fn gemm(
    ta: bool,
    tb: bool,
    alpha: f64,
    a: MatRef<'_>,
    b: MatRef<'_>,
    beta: f64,
    c: &mut MatMut<'_>,
) -> Result<(), MathError> {
    let (m, k) = shape(a.nrow, a.ncol, ta);
    let (l, n) = shape(b.nrow, b.ncol, tb);
    if k != l || (c.nrow, c.ncol) != (m, n) {
        return Err(MathError::Dim);
    }
    size(a.nrow, a.ncol, a.data.len())?;
    size(b.nrow, b.ncol, b.data.len())?;
    size(m, n, c.data.len())?;
    if alpha == 0.0 || k == 0 {
        if beta == 0.0 {
            c.data.fill(0.0);
        } else if beta != 1.0 {
            for value in &mut *c.data {
                *value *= beta;
            }
        }
        return Ok(());
    }
    if m == 0 || n == 0 {
        return Ok(());
    }
    for j in 0..n {
        for i in 0..m {
            let sum = dot((0..k).map(|p| {
                let x = if ta { a.get(p, i) } else { a.get(i, p) };
                let y = if tb { b.get(j, p) } else { b.get(p, j) };
                (x, y)
            }));
            let scaled = if beta == 0.0 { 0.0 } else { beta * c.get(i, j) };
            c.set(i, j, na(sum).unwrap_or_else(|| alpha.mul_add(sum, scaled)));
        }
    }
    Ok(())
}

fn dot(terms: impl Iterator<Item = (f64, f64)>) -> f64 {
    let mut sum = 0.0;
    for (a, b) in terms {
        if let Some(missing) = na(a).or_else(|| na(b)) {
            return missing;
        }
        sum = a.mul_add(b, sum);
    }
    sum
}

fn na(x: f64) -> Option<f64> {
    (x.is_nan() && x.to_bits() as u32 == 1954).then_some(x)
}

fn product(
    ta: bool,
    tb: bool,
    a: MatRef<'_>,
    b: MatRef<'_>,
    out: &mut [f64],
) -> Result<(), MathError> {
    let (m, _) = shape(a.nrow, a.ncol, ta);
    let (_, n) = shape(b.nrow, b.ncol, tb);
    gemm(ta, tb, 1.0, a, b, 0.0, &mut MatMut::new(out, m, n)?)
}

/// Compute `A * B`.
pub fn matprod(a: MatRef<'_>, b: MatRef<'_>, out: &mut [f64]) -> Result<(), MathError> {
    product(false, false, a, b, out)
}

/// Compute `transpose(A) * B`.
pub fn crossprod(a: MatRef<'_>, b: MatRef<'_>, out: &mut [f64]) -> Result<(), MathError> {
    product(true, false, a, b, out)
}

/// Compute `transpose(A) * A`.
pub fn syrk_crossprod(a: MatRef<'_>, out: &mut [f64]) -> Result<(), MathError> {
    symmetric(a, true, out)
}

/// Compute `A * transpose(B)`.
pub fn tcrossprod(a: MatRef<'_>, b: MatRef<'_>, out: &mut [f64]) -> Result<(), MathError> {
    product(false, true, a, b, out)
}

/// Compute `A * transpose(A)`.
pub fn syrk_tcrossprod(a: MatRef<'_>, out: &mut [f64]) -> Result<(), MathError> {
    symmetric(a, false, out)
}

fn symmetric(a: MatRef<'_>, trans: bool, out: &mut [f64]) -> Result<(), MathError> {
    size(a.nrow, a.ncol, a.data.len())?;
    let (n, k) = if trans {
        (a.ncol, a.nrow)
    } else {
        (a.nrow, a.ncol)
    };
    size(n, n, out.len())?;
    for j in 0..n {
        for i in 0..=j {
            let value = if trans {
                dot(a.col(i).iter().copied().zip(a.col(j).iter().copied()))
            } else {
                dot((0..k).map(|p| (a.get(i, p), a.get(j, p))))
            };
            out[j * n + i] = value;
            out[i * n + j] = value;
        }
    }
    Ok(())
}

/// Immutable column-major complex matrix view.
#[derive(Clone, Copy)]
pub struct ComplexMatRef<'a> {
    data: &'a [Cplx],
    rows: usize,
    cols: usize,
}

impl<'a> ComplexMatRef<'a> {
    /// Make a view after checking the data length.
    pub fn new(data: &'a [Cplx], rows: usize, cols: usize) -> Result<Self, MathError> {
        size(rows, cols, data.len())?;
        Ok(Self { data, rows, cols })
    }

    fn get(self, i: usize, j: usize, trans: bool) -> Cplx {
        let (row, col) = if trans { (j, i) } else { (i, j) };
        self.data[col * self.rows + row]
    }
}

fn complex_product(
    ta: bool,
    tb: bool,
    a: ComplexMatRef<'_>,
    b: ComplexMatRef<'_>,
    out: &mut [Cplx],
) -> Result<(), MathError> {
    let (m, k) = shape(a.rows, a.cols, ta);
    let (l, n) = shape(b.rows, b.cols, tb);
    if k != l {
        return Err(MathError::Dim);
    }
    size(m, n, out.len())?;
    if m == 0 {
        return Ok(());
    }
    for (j, col) in out.chunks_exact_mut(m).enumerate() {
        for (i, value) in col.iter_mut().enumerate() {
            let mut sum = Cplx::default();
            for p in 0..k {
                let x = a.get(i, p, ta);
                let y = b.get(p, j, tb);
                if let Some(missing) = [x.re, x.im, y.re, y.im].into_iter().find_map(na) {
                    sum = Cplx::new(missing, missing);
                    break;
                }
                sum.re += x.re.mul_add(y.re, -(x.im * y.im));
                sum.im += x.re.mul_add(y.im, x.im * y.re);
            }
            *value = sum;
        }
    }
    Ok(())
}

/// Compute the product of two complex matrices.
pub fn matprod_cplx(
    a: ComplexMatRef<'_>,
    b: ComplexMatRef<'_>,
    out: &mut [Cplx],
) -> Result<(), MathError> {
    complex_product(false, false, a, b, out)
}

/// Compute the complex cross product without conjugation.
pub fn crossprod_cplx(
    a: ComplexMatRef<'_>,
    b: ComplexMatRef<'_>,
    out: &mut [Cplx],
) -> Result<(), MathError> {
    complex_product(true, false, a, b, out)
}

/// Compute the complex transpose cross product without conjugation.
pub fn tcrossprod_cplx(
    a: ComplexMatRef<'_>,
    b: ComplexMatRef<'_>,
    out: &mut [Cplx],
) -> Result<(), MathError> {
    complex_product(false, true, a, b, out)
}

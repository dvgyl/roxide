use crate::{Cplx, MathError};

use super::max_index;

const ZERO: Cplx = Cplx::new(0.0, 0.0);
const ONE: Cplx = Cplx::new(1.0, 0.0);

fn abs(z: Cplx) -> f64 {
    z.re.hypot(z.im)
}

fn sub_product(x: Cplx, a: Cplx, b: Cplx) -> Cplx {
    Cplx::new(
        x.re - a.re.mul_add(b.re, -(a.im * b.im)),
        x.im - a.re.mul_add(b.im, a.im * b.re),
    )
}

fn divide(x: Cplx, y: Cplx) -> Cplx {
    let scale = y.re.abs().max(y.im.abs());
    let (a, b) = (y.re / scale, y.im / scale);
    let (re, im) = (x.re / scale, x.im / scale);
    let d = a.mul_add(a, b * b);
    Cplx::new(re.mul_add(a, im * b) / d, im.mul_add(a, -(re * b)) / d)
}

fn conj(z: Cplx) -> Cplx {
    Cplx::new(z.re, -z.im)
}

/// Packed LU factors of a complex square matrix.
pub struct ComplexLu {
    data: Vec<Cplx>,
    pivots: Vec<usize>,
    n: usize,
    zero: Option<usize>,
}

/// Factor a complex square matrix using partial row pivoting.
pub fn zgetrf(mut data: Vec<Cplx>, n: usize) -> Result<ComplexLu, MathError> {
    if data.len() != n.checked_mul(n).ok_or(MathError::TooLarge)? {
        return Err(MathError::Dim);
    }
    let mut pivots = Vec::with_capacity(n);
    let mut zero = None;
    for j in 0..n {
        let p = max_index(j..n, |i| {
            let z = data[j * n + i];
            z.re.abs() + z.im.abs()
        })
        .ok_or(MathError::Dim)?;
        pivots.push(p);
        for col in data.chunks_exact_mut(n) {
            col.swap(j, p);
        }
        let (done, right) = data.split_at_mut((j + 1) * n);
        let col = &mut done[j * n..];
        let diag = col[j];
        if diag == ZERO {
            zero.get_or_insert(j + 1);
            continue;
        }
        for z in &mut col[j + 1..] {
            *z = divide(*z, diag);
        }
        for trailing in right.chunks_exact_mut(n) {
            let v = trailing[j];
            if v != ZERO {
                for i in j + 1..n {
                    trailing[i] = sub_product(trailing[i], col[i], v);
                }
            }
        }
    }
    Ok(ComplexLu {
        data,
        pivots,
        n,
        zero,
    })
}

impl ComplexLu {
    /// Solve for all column-major right-hand sides in place.
    pub fn solve(&self, rhs: &mut [Cplx]) -> Result<(), MathError> {
        if let Some(info) = self.zero {
            return Err(MathError::Singular { info });
        }
        if self.n == 0 {
            return if rhs.is_empty() {
                Ok(())
            } else {
                Err(MathError::Dim)
            };
        }
        if !rhs.len().is_multiple_of(self.n) {
            return Err(MathError::Dim);
        }
        for col in rhs.chunks_exact_mut(self.n) {
            for (i, &p) in self.pivots.iter().enumerate() {
                col.swap(i, p);
            }
            self.apply(col, false);
        }
        Ok(())
    }

    fn apply(&self, x: &mut [Cplx], trans: bool) {
        let n = self.n;
        if trans {
            for j in 0..n {
                let col = &self.data[j * n..(j + 1) * n];
                let mut z = x[j];
                for i in 0..j {
                    z = sub_product(z, conj(col[i]), x[i]);
                }
                x[j] = divide(z, conj(col[j]));
            }
            for j in (0..n).rev() {
                let col = &self.data[j * n..(j + 1) * n];
                for i in j + 1..n {
                    x[j] = sub_product(x[j], conj(col[i]), x[i]);
                }
            }
        } else {
            for j in 0..n {
                if x[j] != ZERO {
                    for i in j + 1..n {
                        x[i] = sub_product(x[i], self.data[j * n + i], x[j]);
                    }
                }
            }
            for j in (0..n).rev() {
                if x[j] != ZERO {
                    x[j] = divide(x[j], self.data[j * n + j]);
                    for i in 0..j {
                        x[i] = sub_product(x[i], self.data[j * n + i], x[j]);
                    }
                }
            }
        }
    }

    /// Estimate the reciprocal one-norm or infinity-norm condition number.
    pub fn rcond(&self, norm: f64, one: bool) -> f64 {
        let n = self.n;
        if n == 0 {
            return 1.0;
        }
        if self.zero.is_some() || norm == 0.0 {
            return 0.0;
        }
        let mut x = vec![Cplx::new(1.0 / n as f64, 0.0); n];
        let sum = |x: &[Cplx]| x.iter().copied().map(abs).sum::<f64>();
        let largest = |x: &[Cplx]| max_index(0..n, |i| abs(x[i])).unwrap_or(0);
        self.apply(&mut x, !one);
        let mut estimate = sum(&x);
        if n > 1 {
            let mut j = 0;
            for step in 1..=5 {
                for z in &mut x {
                    let mag = abs(*z);
                    *z = if mag > f64::MIN_POSITIVE {
                        Cplx::new(z.re / mag, z.im / mag)
                    } else {
                        ONE
                    };
                }
                self.apply(&mut x, one);
                let p = largest(&x);
                if step > 1 && abs(x[j]) == abs(x[p]) {
                    break;
                }
                j = p;
                x.fill(ZERO);
                x[j] = ONE;
                self.apply(&mut x, !one);
                let old = estimate;
                estimate = sum(&x);
                if estimate <= old {
                    break;
                }
            }
            for (i, z) in x.iter_mut().enumerate() {
                let sign = if i.is_multiple_of(2) { 1.0 } else { -1.0 };
                *z = Cplx::new(sign * (1.0 + i as f64 / (n - 1) as f64), 0.0);
            }
            self.apply(&mut x, !one);
            let alt = 2.0 * (sum(&x) / (3.0 * n as f64));
            estimate = estimate.max(alt);
        }
        if !estimate.is_finite() || estimate == 0.0 {
            0.0
        } else {
            (1.0 / estimate) / norm
        }
    }
}

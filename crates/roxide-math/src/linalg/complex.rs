//! Complex matrix decompositions for column-major matrices.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod general;
mod reduced;

use crate::{Cplx, MathError};

pub use general::{ComplexEigen, complex_general_eigen};
pub use reduced::{ComplexSvd, HermitianEigen, complex_svd, hermitian_eigen};

const ONE: Cplx = Cplx::new(1.0, 0.0);
const ZERO: Cplx = Cplx::new(0.0, 0.0);

impl Cplx {
    fn conjugate(self) -> Self {
        Self::new(self.re, -self.im)
    }

    fn modulus(self) -> f64 {
        self.re.hypot(self.im)
    }

    fn scaled(self, f: f64) -> Self {
        Self::new(self.re * f, self.im * f)
    }

    fn divided_by(self, f: f64) -> Self {
        Self::new(self.re / f, self.im / f)
    }

    fn phase(self) -> Self {
        let norm = self.modulus();
        if norm == 0.0 {
            ONE
        } else {
            self.divided_by(norm)
        }
    }

    fn plus(self, b: Self) -> Self {
        Self::new(self.re + b.re, self.im + b.im)
    }

    fn minus(self, b: Self) -> Self {
        Self::new(self.re - b.re, self.im - b.im)
    }

    fn times(self, b: Self) -> Self {
        Self::new(
            self.re * b.re - self.im * b.im,
            self.re * b.im + self.im * b.re,
        )
    }

    fn quotient(self, b: Self) -> Self {
        self.divided_by(b.modulus()).times(b.phase().conjugate())
    }
}

fn area(rows: usize, cols: usize) -> Result<usize, MathError> {
    rows.checked_mul(cols)
        .filter(|&n| n <= isize::MAX as usize / size_of::<Cplx>())
        .ok_or(MathError::TooLarge)
}

fn check_matrix(data: &[Cplx], rows: usize, cols: usize) -> Result<(), MathError> {
    if rows == 0 || cols == 0 || data.len() != area(rows, cols)? {
        return Err(MathError::Dim);
    }
    if data.iter().any(|z| !z.re.is_finite() || !z.im.is_finite()) {
        return Err(MathError::NoConvergence { info: 1 });
    }
    Ok(())
}

fn diagonal(data: &[Cplx], rows: usize) -> bool {
    data.chunks_exact(rows)
        .enumerate()
        .all(|(j, col)| col.iter().enumerate().all(|(i, &z)| i == j || z == ZERO))
}

fn basis(rows: usize, count: usize, order: &[usize]) -> Result<Vec<Cplx>, MathError> {
    let mut out = vec![ZERO; area(rows, count)?];
    for (j, col) in out.chunks_exact_mut(rows).enumerate() {
        col[order.get(j).copied().unwrap_or(j)] = ONE;
    }
    Ok(out)
}

fn scaled_matrix(data: &[Cplx], rows: usize, cols: usize) -> Result<(Vec<Cplx>, f64), MathError> {
    check_matrix(data, rows, cols)?;
    let max = data
        .iter()
        .fold(0.0_f64, |max, z| max.max(z.re.abs()).max(z.im.abs()));
    let exponent = ((max.to_bits() >> 52) & 0x7ff) as i32 - 1023;
    let factor = if max == 0.0 {
        1.0
    } else {
        2.0_f64.powi(exponent.max(-1022))
    };
    Ok((data.iter().map(|z| z.divided_by(factor)).collect(), factor))
}

fn adjoint(a: &[Cplx], rows: usize, cols: usize) -> Vec<Cplx> {
    (0..rows)
        .flat_map(|i| (0..cols).map(move |j| a[j * rows + i].conjugate()))
        .collect()
}

fn euclidean(v: &[Cplx]) -> f64 {
    v.iter().fold(0.0, |norm, z| norm.hypot(z.modulus()))
}

/// A Hermitian reflector `I - 2vv^H` with a unit direction.
struct Reflector {
    start: usize,
    direction: Vec<Cplx>,
}

impl Reflector {
    fn new(start: usize, mut direction: Vec<Cplx>) -> Option<Self> {
        if direction[1..].iter().all(|&z| z == ZERO) {
            return None;
        }
        let norm = euclidean(&direction);
        let phase = direction[0].phase();
        for z in &mut direction {
            *z = z.divided_by(norm);
        }
        direction[0] = direction[0].plus(phase);
        let norm = euclidean(&direction);
        for z in &mut direction {
            *z = z.divided_by(norm);
        }
        Some(Self { start, direction })
    }

    fn left(&self, a: &mut [Cplx], rows: usize) {
        for col in a.chunks_exact_mut(rows) {
            let x = &mut col[self.start..self.start + self.direction.len()];
            let dot = self
                .direction
                .iter()
                .zip(x.iter())
                .fold(ZERO, |s, (&v, &x)| s.plus(v.conjugate().times(x)))
                .scaled(2.0);
            for (x, &v) in x.iter_mut().zip(&self.direction) {
                *x = x.minus(v.times(dot));
            }
        }
    }

    fn right(&self, a: &mut [Cplx], rows: usize) {
        if a.is_empty() {
            return;
        }
        let a = &mut a[self.start * rows..];
        for i in 0..rows {
            let dot = self
                .direction
                .iter()
                .enumerate()
                .fold(ZERO, |s, (j, &v)| s.plus(a[j * rows + i].times(v)))
                .scaled(2.0);
            for (j, &v) in self.direction.iter().enumerate() {
                let x = &mut a[j * rows + i];
                *x = x.minus(dot.times(v.conjugate()));
            }
        }
    }
}

fn lift(data: Vec<f64>, phases: &[Cplx], reflectors: &[Reflector]) -> Vec<Cplx> {
    let n = phases.len();
    let mut vectors: Vec<_> = data
        .into_iter()
        .enumerate()
        .map(|(i, x)| phases[i % n].scaled(x))
        .collect();
    for h in reflectors.iter().rev() {
        h.left(&mut vectors, n);
    }
    vectors
}

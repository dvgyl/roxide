//! Shared pure-Rust numerical core types for `roxide-math` (ADR-0009).

use std::fmt;

/// Distribution tail and log-probability flags (`lower.tail`, `log.p`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tail {
    /// When true, compute `P(X <= x)` (`lower.tail = TRUE`); otherwise `P(X > x)`.
    pub lower: bool,
    /// When true, return or accept log-probabilities (`log.p = TRUE`).
    pub log: bool,
}

impl Default for Tail {
    fn default() -> Self {
        Self {
            lower: true,
            log: false,
        }
    }
}

/// Immutable column-major dense `f64` matrix view.
#[derive(Clone, Copy, Debug)]
pub struct MatRef<'a> {
    /// Column-major matrix entries (`len == nrow * ncol`).
    pub data: &'a [f64],
    /// Number of rows.
    pub nrow: usize,
    /// Number of columns.
    pub ncol: usize,
}

impl<'a> MatRef<'a> {
    /// Construct a column-major matrix view, returning [`MathError::Dim`] if `data.len() != nrow * ncol`.
    pub fn new(data: &'a [f64], nrow: usize, ncol: usize) -> Result<Self, MathError> {
        let expected = nrow.checked_mul(ncol).ok_or(MathError::TooLarge)?;
        if data.len() != expected {
            return Err(MathError::Dim);
        }
        Ok(Self { data, nrow, ncol })
    }

    /// Borrow column `j`.
    #[inline]
    #[must_use]
    pub fn col(&self, j: usize) -> &'a [f64] {
        let start = j * self.nrow;
        &self.data[start..start + self.nrow]
    }

    /// Read element `(i, j)`.
    #[inline]
    #[must_use]
    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.data[j * self.nrow + i]
    }
}

/// Mutable column-major dense `f64` matrix view.
#[derive(Debug)]
pub struct MatMut<'a> {
    /// Column-major matrix entries (`len == nrow * ncol`).
    pub data: &'a mut [f64],
    /// Number of rows.
    pub nrow: usize,
    /// Number of columns.
    pub ncol: usize,
}

impl<'a> MatMut<'a> {
    /// Construct a mutable column-major matrix view, checking dimensions.
    pub fn new(data: &'a mut [f64], nrow: usize, ncol: usize) -> Result<Self, MathError> {
        let expected = nrow.checked_mul(ncol).ok_or(MathError::TooLarge)?;
        if data.len() != expected {
            return Err(MathError::Dim);
        }
        Ok(Self { data, nrow, ncol })
    }

    /// Reborrow as an immutable [`MatRef`].
    #[inline]
    #[must_use]
    pub fn as_ref(&self) -> MatRef<'_> {
        MatRef {
            data: self.data,
            nrow: self.nrow,
            ncol: self.ncol,
        }
    }

    /// Borrow mutable column `j`.
    #[inline]
    pub fn col_mut(&mut self, j: usize) -> &mut [f64] {
        let start = j * self.nrow;
        &mut self.data[start..start + self.nrow]
    }

    /// Read element `(i, j)`.
    #[inline]
    #[must_use]
    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.data[j * self.nrow + i]
    }

    /// Write element `(i, j)`.
    #[inline]
    pub fn set(&mut self, i: usize, j: usize, value: f64) {
        self.data[j * self.nrow + i] = value;
    }
}

/// Double-precision complex scalar for FFT and spectral routines.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Cplx {
    /// Real part.
    pub re: f64,
    /// Imaginary part.
    pub im: f64,
}

impl Cplx {
    /// Construct a complex scalar `re + im*i`.
    #[inline]
    #[must_use]
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
}

/// Bitflag status set reported by pure numerical kernels to R binding wrappers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Flags(u8);

impl Flags {
    /// A `NaN` was produced by domain or parameter error (`"NaNs produced"`).
    pub const NAN_PRODUCED: u8 = 1 << 0;
    /// Loss of numerical precision occurred (`"full precision may not have been achieved"`).
    pub const PRECISION: u8 = 1 << 1;
    /// Underflow occurred.
    pub const UNDERFLOW: u8 = 1 << 2;
    /// A non-integer argument was passed to a discrete density (`"non-integer x = ..."`).
    pub const NONINT_X: u8 = 1 << 3;
    /// An iterative kernel failed to converge.
    pub const NO_CONVERGENCE: u8 = 1 << 4;

    /// Set a status bit.
    #[inline]
    pub fn set(&mut self, bit: u8) {
        self.0 |= bit;
    }

    /// Check whether a status bit is set.
    #[inline]
    #[must_use]
    pub const fn has(self, bit: u8) -> bool {
        (self.0 & bit) != 0
    }

    /// Raw bitmask.
    #[inline]
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

/// Pure numerical error returned by matrix, spectral, and solver kernels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MathError {
    /// Incompatible or invalid matrix/vector dimensions.
    Dim,
    /// Matrix is computationally or exactly singular (`U[info, info] == 0`, 1-based).
    Singular {
        /// 1-based pivot index where singularity occurred.
        info: usize,
    },
    /// Matrix is not positive definite (1-based leading minor index).
    NotPositiveDefinite {
        /// 1-based leading minor order that failed factorization.
        minor: usize,
    },
    /// Iterative decomposition or solver failed to converge.
    NoConvergence {
        /// Algorithm-specific diagnostic code.
        info: i32,
    },
    /// Allocation or dimension exceeds safe limits.
    TooLarge,
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dim => f.write_str("non-conformable or invalid dimensions"),
            Self::Singular { info } => {
                write!(f, "system is exactly singular: U[{info},{info}] = 0")
            }
            Self::NotPositiveDefinite { minor } => {
                write!(
                    f,
                    "the leading minor of order {minor} is not positive definite"
                )
            }
            Self::NoConvergence { info } => {
                write!(f, "algorithm failed to converge (code {info})")
            }
            Self::TooLarge => f.write_str("matrix or workspace size exceeds safe limit"),
        }
    }
}

impl std::error::Error for MathError {}

/// Scalar objective with optional gradient for multidimensional optimizers (`optim`).
///
/// Generic over `Err` so R evaluator `Unwind` values propagate untouched across callbacks.
pub trait Objective {
    /// Error / unwind type propagated from callbacks.
    type Err;

    /// Evaluate the scalar objective at `x`.
    fn value(&mut self, x: &[f64]) -> Result<f64, Self::Err>;

    /// Evaluate the gradient at `x` into `g`.
    fn gradient(&mut self, x: &[f64], g: &mut [f64]) -> Result<(), Self::Err>;
}

/// Vectorized integrand callback (`integrate` evaluates 21 points per call).
pub trait VecFn {
    /// Error / unwind type propagated from callbacks.
    type Err;

    /// Evaluate integrand at every point in `x`, storing results in `out`.
    fn eval(&mut self, x: &[f64], out: &mut [f64]) -> Result<(), Self::Err>;
}

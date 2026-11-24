//! Exact GNU R 4.6.2 `NA` / `NaN` bit semantics, Kleene logic, and checked integer math.

use crate::object::{CharId, SexpType};

/// Integer `NA` sentinel (`i32::MIN`).
pub const NA_INTEGER: i32 = i32::MIN;

/// Fresh `NA_real_` IEEE 754 bit pattern on arm64/x86_64 (payload `1954` = `0x07A2`,
/// verified byte-for-byte against official GNU R 4.6.2).
pub const NA_REAL_BITS: u64 = 0x7FF0_0000_0000_07A2;

/// Canonical quieted `NA_real_` IEEE 754 bit pattern (`0x7FF8_0000_0000_07A2`).
pub const NA_REAL_QUIET_BITS: u64 = 0x7FF8_0000_0000_07A2;

/// Canonical IEEE 754 quiet `NaN` with zero payload (`0x7FF8_0000_0000_0000`).
pub const R_NAN_BITS: u64 = 0x7FF8_0000_0000_0000;

/// Fresh `NA_real_` floating-point constant.
pub const NA_REAL: f64 = f64::from_bits(NA_REAL_BITS);

/// Canonical non-NA `NaN` constant.
pub const R_NAN: f64 = f64::from_bits(R_NAN_BITS);

/// Return true iff `x` is an R `NA_real_` (IEEE NaN whose low 32-bit payload equals `1954`).
#[inline]
#[must_use]
pub fn is_na_real(x: f64) -> bool {
    x.is_nan() && (x.to_bits() as u32) == 1954
}

/// Return true iff `x` is an IEEE NaN that is NOT an R `NA_real_` (matches R's `is.nan()`).
#[inline]
#[must_use]
pub fn is_nan_not_na(x: f64) -> bool {
    x.is_nan() && (x.to_bits() as u32) != 1954
}

/// Canonical hash/matching key for doubles: `-0.0 == 0.0`, all `NA_real_` share one key,
/// and all non-NA `NaN` share a distinct key.
#[inline]
#[must_use]
pub fn real_hash_key(x: f64) -> u64 {
    if is_na_real(x) {
        NA_REAL_BITS
    } else if x.is_nan() {
        R_NAN_BITS
    } else if x == 0.0 {
        0
    } else {
        x.to_bits()
    }
}

/// Deterministic NaN propagation for binary floating-point ops matching AArch64 `FPProcessNaNs`
/// and GNU R 4.6.2 on Apple Silicon (verified empirically against live GNU R 4.6.2).
#[inline]
#[must_use]
pub fn nan_propagate(a: f64, b: f64) -> f64 {
    const QUIET_BIT: u64 = 1u64 << 51;
    let ab = a.to_bits();
    let bb = b.to_bits();
    let a_nan = a.is_nan();
    let b_nan = b.is_nan();
    // Signaling NaNs take priority over quiet NaNs (and are quietened).
    if a_nan && (ab & QUIET_BIT) == 0 {
        return f64::from_bits(ab | QUIET_BIT);
    }
    if b_nan && (bb & QUIET_BIT) == 0 {
        return f64::from_bits(bb | QUIET_BIT);
    }
    if a_nan {
        return a;
    }
    if b_nan {
        return b;
    }
    R_NAN
}

/// A real arithmetic operation shared by the AST and register evaluators.
#[derive(Clone, Copy)]
pub enum RealArithOp {
    /// Add.
    Add,
    /// Subtract.
    Sub,
    /// Multiply.
    Mul,
    /// Divide.
    Div,
    /// Raise to a power.
    Pow,
}

impl RealArithOp {
    /// Apply this operation with R's NaN propagation rules.
    #[inline]
    #[must_use]
    pub fn apply(self, a: f64, b: f64) -> f64 {
        let result = match self {
            Self::Add => a + b,
            Self::Sub => a - b,
            Self::Mul => a * b,
            Self::Div => a / b,
            Self::Pow => return real_pow(a, b),
        };
        if result.is_nan() {
            nan_propagate(a, b)
        } else {
            result
        }
    }
}

/// Raise a real value to a power with R's rules for zero, infinities, and NaNs.
#[inline]
#[must_use]
pub fn real_pow(base: f64, exp: f64) -> f64 {
    if base == 1.0 || exp == 0.0 {
        return 1.0;
    }
    if base.is_nan() || exp.is_nan() {
        return nan_propagate(base, exp);
    }
    if base == 0.0 {
        return if exp > 0.0 { 0.0 } else { f64::INFINITY };
    }
    if base < 0.0 && (exp.is_infinite() || exp.fract() != 0.0) {
        return R_NAN;
    }
    if base == f64::NEG_INFINITY {
        return if exp < 0.0 {
            0.0
        } else if exp % 2.0 != 0.0 {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    base.powf(exp)
}

/// Checked R integer addition returning `(result, overflowed)`.
#[inline]
#[must_use]
pub fn int_add(a: i32, b: i32) -> (i32, bool) {
    if a == NA_INTEGER || b == NA_INTEGER {
        return (NA_INTEGER, false);
    }
    let sum = i64::from(a) + i64::from(b);
    if sum < i64::from(-i32::MAX) || sum > i64::from(i32::MAX) {
        (NA_INTEGER, true)
    } else {
        (sum as i32, false)
    }
}

/// Checked R integer subtraction returning `(result, overflowed)`.
#[inline]
#[must_use]
pub fn int_sub(a: i32, b: i32) -> (i32, bool) {
    if a == NA_INTEGER || b == NA_INTEGER {
        return (NA_INTEGER, false);
    }
    let diff = i64::from(a) - i64::from(b);
    if diff < i64::from(-i32::MAX) || diff > i64::from(i32::MAX) {
        (NA_INTEGER, true)
    } else {
        (diff as i32, false)
    }
}

/// Checked R integer multiplication returning `(result, overflowed)`.
#[inline]
#[must_use]
pub fn int_mul(a: i32, b: i32) -> (i32, bool) {
    if a == NA_INTEGER || b == NA_INTEGER {
        return (NA_INTEGER, false);
    }
    let prod = i64::from(a) * i64::from(b);
    if prod < i64::from(-i32::MAX) || prod > i64::from(i32::MAX) {
        (NA_INTEGER, true)
    } else {
        (prod as i32, false)
    }
}

/// Kleene three-valued R logical element (backed by `i32` for C-API compatibility).
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RLogical(i32);

impl RLogical {
    /// `FALSE` (`0`)
    pub const FALSE: Self = Self(0);
    /// `TRUE` (`1`)
    pub const TRUE: Self = Self(1);
    /// `NA` (`i32::MIN`)
    pub const NA: Self = Self(NA_INTEGER);

    /// Construct from a raw `i32` (`0` = false, `i32::MIN` = NA, any other nonzero = true).
    #[inline]
    #[must_use]
    pub const fn from_raw(v: i32) -> Self {
        Self(v)
    }

    /// Construct from `Option<bool>`.
    #[inline]
    #[must_use]
    pub const fn from_option(v: Option<bool>) -> Self {
        match v {
            Some(true) => Self::TRUE,
            Some(false) => Self::FALSE,
            None => Self::NA,
        }
    }

    /// Return the raw `i32` backing representation.
    #[inline]
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Interpret as `Option<bool>` (`None` for `NA`).
    #[inline]
    #[must_use]
    pub const fn get(self) -> Option<bool> {
        if self.0 == NA_INTEGER {
            None
        } else {
            Some(self.0 != 0)
        }
    }

    /// Kleene logical AND (`FALSE & NA == FALSE`).
    #[inline]
    #[must_use]
    pub const fn and(self, other: Self) -> Self {
        match (self.get(), other.get()) {
            (Some(false), _) | (_, Some(false)) => Self::FALSE,
            (Some(true), Some(true)) => Self::TRUE,
            _ => Self::NA,
        }
    }

    /// Kleene logical OR (`TRUE | NA == TRUE`).
    #[inline]
    #[must_use]
    pub const fn or(self, other: Self) -> Self {
        match (self.get(), other.get()) {
            (Some(true), _) | (_, Some(true)) => Self::TRUE,
            (Some(false), Some(false)) => Self::FALSE,
            _ => Self::NA,
        }
    }

    /// Kleene logical NOT (`!NA == NA`).
    #[inline]
    #[must_use]
    pub const fn not(self) -> Self {
        match self.get() {
            Some(true) => Self::FALSE,
            Some(false) => Self::TRUE,
            None => Self::NA,
        }
    }
}

/// R double-precision complex element (`Rcomplex`, C-layout compatible).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RComplex {
    /// Real part.
    pub re: f64,
    /// Imaginary part.
    pub im: f64,
}

impl RComplex {
    /// `NA_complex_`
    pub const NA: Self = Self {
        re: NA_REAL,
        im: NA_REAL,
    };

    /// Return true if either component is `NA_real_` or `NaN`.
    #[inline]
    #[must_use]
    pub fn is_na(self) -> bool {
        self.re.is_nan() || self.im.is_nan()
    }
}

/// Trait unifying atomic vector scalar element types.
pub trait Elem: Copy + 'static {
    /// Corresponding [`SexpType`].
    const TYPE: SexpType;
    /// Canonical missing value (`None` for raw bytes).
    const NA: Option<Self>;
    /// Return true if this element is missing (`NA` or `NaN` for real/complex).
    fn is_na(self) -> bool;
}

impl Elem for RLogical {
    const TYPE: SexpType = SexpType::Lgl;
    const NA: Option<Self> = Some(Self::NA);
    #[inline]
    fn is_na(self) -> bool {
        self.get().is_none()
    }
}

impl Elem for i32 {
    const TYPE: SexpType = SexpType::Int;
    const NA: Option<Self> = Some(NA_INTEGER);
    #[inline]
    fn is_na(self) -> bool {
        self == NA_INTEGER
    }
}

impl Elem for f64 {
    const TYPE: SexpType = SexpType::Real;
    const NA: Option<Self> = Some(NA_REAL);
    #[inline]
    fn is_na(self) -> bool {
        self.is_nan()
    }
}

impl Elem for RComplex {
    const TYPE: SexpType = SexpType::Cplx;
    const NA: Option<Self> = Some(Self::NA);
    #[inline]
    fn is_na(self) -> bool {
        Self::is_na(self)
    }
}

impl Elem for CharId {
    const TYPE: SexpType = SexpType::Str;
    const NA: Option<Self> = Some(CharId::NA);
    #[inline]
    fn is_na(self) -> bool {
        self == CharId::NA
    }
}

impl Elem for u8 {
    const TYPE: SexpType = SexpType::Raw;
    const NA: Option<Self> = None;
    #[inline]
    fn is_na(self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn na_real_and_nan_propagation_match_arm64_oracle() {
        let s = NA_REAL;
        let q = f64::from_bits(NA_REAL_QUIET_BITS);
        let n = R_NAN;

        assert!(is_na_real(s));
        assert!(!is_nan_not_na(s));
        assert!(is_na_real(q));
        assert!(!is_nan_not_na(q));
        assert!(!is_na_real(n));
        assert!(is_nan_not_na(n));

        assert_eq!(nan_propagate(s, n).to_bits(), NA_REAL_QUIET_BITS);
        assert_eq!(nan_propagate(n, s).to_bits(), NA_REAL_QUIET_BITS);
        assert_eq!(nan_propagate(q, n).to_bits(), NA_REAL_QUIET_BITS);
        assert_eq!(nan_propagate(n, q).to_bits(), R_NAN_BITS);
    }

    #[test]
    fn kleene_logic_and_int_overflow() {
        assert_eq!(RLogical::FALSE.and(RLogical::NA), RLogical::FALSE);
        assert_eq!(RLogical::TRUE.or(RLogical::NA), RLogical::TRUE);
        assert_eq!(RLogical::NA.and(RLogical::TRUE), RLogical::NA);
        assert_eq!(int_add(i32::MAX, 1), (NA_INTEGER, true));
        assert_eq!(int_sub(-i32::MAX, 1), (NA_INTEGER, true));
        assert_eq!(int_add(NA_INTEGER, 1), (NA_INTEGER, false));
    }
}

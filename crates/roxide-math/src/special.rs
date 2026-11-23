//! Real special functions and numerical helpers.

#![deny(missing_docs)]

pub mod aux;
pub mod bessel;
pub mod beta;
pub mod gamma;
pub mod psi;

pub use bessel::{bessel_i, bessel_j, bessel_k, bessel_y};
pub use beta::{beta, choose, factorial, lbeta, lchoose, lfactorial};
pub use gamma::{gamma, lgamma, lgamma_sign};
pub use psi::{digamma, psigamma, trigamma};

fn domain(flags: &mut crate::Flags) -> f64 {
    flags.set(crate::Flags::NAN_PRODUCED);
    f64::NAN
}

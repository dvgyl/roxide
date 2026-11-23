#![forbid(unsafe_code)]

//! Pure-Rust, zero-dependency clean-room numerical core for Roxide (ADR-0009).
//!
//! Operates strictly on primitive numerical types (`f64`, `i32`, `u32`, slices) with zero
//! `roxide-eval` or GC dependencies. Reused by `roxide-base`, `roxide-stats`, and (in
//! Milestone 4) `roxide-capi` (`Rmath.h`).

pub mod core;
pub mod dist;
pub mod fft;
pub mod fmin;
pub mod linalg;
pub mod optim;
pub mod quadpack;
pub mod rng;
pub mod roots;
pub mod special;

pub use crate::core::{Cplx, Flags, MatMut, MatRef, MathError, Objective, Tail, VecFn};

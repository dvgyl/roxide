#![deny(unsafe_op_in_unsafe_fn)]

//! Clean-room GNU R 4.6.2 public C API boundary (ADRs 0017–0020).
//!
//! This crate is one of the two explicitly audited `unsafe` boundary crates in Roxide
//! (alongside `roxide-libr`). All 15 pure-Rust engine crates retain `#![forbid(unsafe_code)]`.
//!
pub mod abi;
pub mod api;
pub mod cx;
pub mod globals;
pub mod jump;

/// ABI version tag in `Meta/Roxide.rds` for Roxide-compiled packages.
pub const CAPI_ABI_VERSION: &str = "roxide-capi-4.6.2-v1";

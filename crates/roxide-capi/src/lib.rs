#![deny(unsafe_op_in_unsafe_fn)]

//! Clean-room GNU R 4.6.2 public C API boundary (ADRs 0017–0020).
//!
//! This crate is one of the two explicitly audited `unsafe` boundary crates in Roxide
//! (alongside `roxide-libr`). All 15 pure-Rust engine crates retain `#![forbid(unsafe_code)]`.
//!
//! Populated across Milestone 4 Waves 0b–3:
//! - W0b: `abi.rs`, `cx.rs`, `jump.rs`, `globals.rs`, `csrc/{shim.cc,shim_va.c}`, `build.rs`
//! - W1: public C API families (`api/*.rs`) and clean-room `include/*.h` headers
//! - W2: dynamic library loading (`dll.rs`, `register.rs`, `dotcall.rs`, `hooks.rs`)
//! - W3: foreign ALTREP single-authority bridge (`api/altrep.rs`)

/// ABI version tag stamped into `Meta/Roxide.rds` for Roxide-compiled packages (ADR-0020).
pub const CAPI_ABI_VERSION: &str = "roxide-capi-4.6.2-v1";

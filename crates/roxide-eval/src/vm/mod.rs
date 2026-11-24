//! Clean-room Roxide register bytecode IR and VM scaffold (ADR-0020).
//!
//! Roxide never executes GNU R `BCODESXP` instruction streams; closures are lowered
//! from AST expressions (`consts[0]`) to Roxide's own register IR with `ENVSXP`
//! `shape_version` guards and fallback to the AST semantic reference.
//!
//! Populated in Milestone 4 Waves 1–2 (`ir.rs`, `compile.rs`, `exec.rs`, `guard.rs`, `cache.rs`).

/// Placeholder handle for compiled Roxide bytecode attached to a closure (W1/W2).
#[derive(Clone, Debug, Default)]
pub struct CompiledCode {
    /// Human-readable IR version marker.
    pub version: u32,
}

/// Return true if automatic JIT compilation is active at `level` (`level >= 1`).
#[inline]
#[must_use]
pub const fn is_jit_enabled(level: u8) -> bool {
    level > 0
}

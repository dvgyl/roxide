//! Roxide register bytecode for R syntax trees (ADR-0020).
//!
//! The compiler uses R language objects as input. The instruction set belongs to
//! Roxide. The AST evaluator defines the behavior of fallback paths.

pub mod cache;
pub mod compile;
pub mod exec;
pub mod guard;
pub mod ir;

pub use compile::compile_expr;
pub use exec::run_body;
pub use ir::{CompiledCode, Op, Reg};

/// Return true if automatic JIT compilation is active at `level` (`level >= 1`).
#[inline]
#[must_use]
pub const fn is_jit_enabled(level: u8) -> bool {
    level > 0
}

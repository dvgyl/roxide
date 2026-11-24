//! Roxide register bytecode for R syntax trees (ADR-0020).
//!
//! The compiler uses R language objects as input. The instruction set belongs to
//! Roxide. The AST evaluator defines the behavior of fallback paths.

pub mod cache;
pub mod compile;
pub mod exec;
pub mod guard;
pub mod ir;
mod iterator;
mod jit;
mod scalar;
mod stack;
mod subscript;

pub use compile::compile_expr;
pub use exec::{run_body, run_compiled};
pub use ir::{CompiledCode, Op, Reg};
pub use jit::is_compiled;

pub(crate) use exec::try_loop;

/// Return true if automatic JIT compilation is active at `level` (`level >= 1`).
#[inline]
#[must_use]
pub const fn is_jit_enabled(level: u8) -> bool {
    level > 0
}

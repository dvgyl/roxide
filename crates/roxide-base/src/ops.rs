//! Arithmetic, comparisons, Kleene logic, reductions, math, coercion, `identical`, and attributes.

use roxide_eval::Module;

/// Primitives and prelude exported by `roxide-base::ops`.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

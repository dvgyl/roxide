//! Base RNG bindings (`set.seed`, `RNGkind`, `sample`, `sample.int`, `RngExt`) (W1A).

use roxide_eval::Module;

/// Submodule primitive and prelude registration table.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

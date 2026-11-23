//! Reference classes (`setRefClass`, active-binding fields, `$copy`, `callSuper`).

use roxide_eval::Module;

/// Reference classes submodule (owned by M3 Builder A).
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

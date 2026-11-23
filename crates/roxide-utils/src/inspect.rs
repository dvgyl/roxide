//! Session inspection and head/tail utilities (`head`, `tail`, `sessionInfo`, `object.size`, `capture.output`) (W6B).

use roxide_eval::Module;

/// Submodule primitive and prelude registration table.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

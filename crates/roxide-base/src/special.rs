//! Base special math bindings (`gamma`, `lgamma`, `digamma`, `trigamma`, `psigamma`, `beta`, `choose`, `bessel*`) (W1B).

use roxide_eval::Module;

/// Submodule primitive and prelude registration table.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

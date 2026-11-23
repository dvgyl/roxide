//! Base matrix and linear algebra bindings (`%*%`, `crossprod`, `qr`, `chol`, `solve`, `det`, `svd`, `eigen`) (W3B/W5B/W7B).

use roxide_eval::Module;

/// Submodule primitive and prelude registration table.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

//! Base UTF-8 `read.dcf` and `write.dcf` with `#` comment support (W6B).

use roxide_eval::Module;

/// Submodule primitive and prelude registration table.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

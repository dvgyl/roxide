//! `tools` Rd parser and renderer (`parse_Rd`, `Rd2txt` with R 4.6.2 fix) (W8B).

use roxide_eval::Module;

/// Submodule primitive and prelude registration table.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

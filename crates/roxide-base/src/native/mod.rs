//! Native foreign-call and dynamic-library base primitives (`.Call`, `.C`, `dyn.load`, W2-F).

use roxide_eval::Module;

/// Native call and DLL management base module.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

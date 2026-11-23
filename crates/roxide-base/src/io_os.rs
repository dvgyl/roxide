//! Filesystem, OS helpers, and I/O connections (`readLines`, `readBin`, `readChar`, `gzfile`, ...).

use roxide_eval::Module;

/// Primitives and prelude exported by `roxide-base::io_os`.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

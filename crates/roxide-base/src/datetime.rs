//! `Date`, `POSIXct`, `POSIXlt`, `strptime`, `strftime`, and `difftime`.

use roxide_eval::Module;

/// Primitives and prelude exported by `roxide-base::datetime`.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

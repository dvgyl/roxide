//! String manipulation, formatting (`sprintf`), encodings, and regex (`fixed`/TRE/PCRE2).

use roxide_eval::Module;

/// Primitives and prelude exported by `roxide-base::strings`.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

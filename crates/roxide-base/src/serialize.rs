//! XDR v2/v3 `.rds` and `.RData` serialization (`serialize`, `unserialize`, `readRDS`, `saveRDS`, `save`, `load`).

use roxide_eval::Module;

/// Primitives and prelude exported by `roxide-base::serialize`.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

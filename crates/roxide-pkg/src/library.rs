//! `library`, `require`, `use`, `.libPaths`, `R.home`, `find.package`, `system.file`, `packageVersion`.

use roxide_eval::Module;

/// Package library search & attach primitives (owned by M3 Builder C).
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

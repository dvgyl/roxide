//! Lazy-load `.rdb`/`.rdx` database reader/writer and `PERSISTSXP` environment hook (M3 W2-C).

use roxide_eval::Module;

/// Lazy-load DB base primitives (`lazyLoadDBfetch`, etc.; owned by M3 Builder C).
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

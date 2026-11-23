//! S4 namespace metadata caching (`.rx_cacheMetaData`, exportClasses/exportMethods integration).

use roxide_eval::Module;

/// S4 metadata submodule (owned by M3 Builder B).
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

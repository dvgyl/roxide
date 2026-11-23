//! Classical hypothesis tests (`t.test`, `wilcox.test`, `chisq.test`, `fisher.test`, `ks.test`, `shapiro.test`, `prop.test`, `binom.test`, `cor.test`) (W8A).

use roxide_eval::Module;

/// Submodule primitive and prelude registration table.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

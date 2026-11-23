//! Generalized linear models and their family, link, and inference methods.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod families;
mod links;

use roxide_eval::Module;

use crate::distn::support::internal;

/// Registration table for generalized linear models.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_glm_gamma_shapes", 2, families::gamma_shapes),
        internal("C_glm_initializer", 1, links::initializer),
        internal("C_glm_link_math", 2, links::link_math),
    ],
    prelude: concat!(
        include_str!("glm/links.R"),
        include_str!("glm/families.R"),
        include_str!("glm/fit.R"),
        include_str!("glm/methods.R"),
        include_str!("glm/anova.R"),
        include_str!("glm/print.R"),
    ),
};

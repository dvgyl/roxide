//! Linear models and methods for fitted model objects.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod fit;

use roxide_eval::Module;

use crate::distn::support::internal;

/// Registration table for linear models and their methods.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_lm_dqrls", 4, fit::least_squares),
        internal("C_lm_call", 2, fit::language),
        internal("C_lm_size", 1, fit::size),
    ],
    prelude: concat!(
        include_str!("linmod/fit.R"),
        include_str!("linmod/methods.R"),
        include_str!("linmod/predict.R"),
        include_str!("linmod/anova.R"),
        include_str!("linmod/dummy.R"),
        include_str!("linmod/print.R"),
    ),
};

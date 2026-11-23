//! Distance, clustering, principal components, and multidimensional scaling.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod cluster;
mod distance;
mod kmeans;
mod output;
mod scaling;
mod support;

use roxide_eval::Module;

use crate::distn::support::internal;

/// Registration table for multivariate statistics and their S3 methods.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_multivar_explicit_rows", 1, support::explicit_rows),
        internal("C_multivar_call", 2, support::replace_call),
        internal("C_multivar_match", 3, support::match_choice),
        internal("C_multivar_dist", 6, distance::dist),
        internal("C_multivar_dist_matrix", 1, distance::as_matrix),
        internal("C_multivar_print_dist", 5, distance::print),
        internal("C_multivar_print_named", 2, output::print_named),
        internal("C_multivar_print_fields", 1, output::print_fields),
        internal("C_multivar_print_matrix", 2, output::print_matrix),
        internal("C_multivar_hclust", 4, cluster::hclust),
        internal("C_multivar_cutree", 3, cluster::cutree),
        internal("C_multivar_unique_rows", 1, kmeans::unique_rows),
        internal("C_multivar_kmeans", 5, kmeans::fit),
        internal("C_multivar_cmdscale", 6, scaling::cmdscale),
    ],
    prelude: concat!(
        include_str!("multivar/distance.R"),
        include_str!("multivar/kmeans.R"),
        include_str!("multivar/components.R"),
    ),
};

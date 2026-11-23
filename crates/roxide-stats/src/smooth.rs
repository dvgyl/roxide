//! Density estimates, interpolation, smoothing, and nonlinear minimization.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod density;
mod interpolate;
mod loess;
mod lowess;
mod nonlinear;

use crate::distn::support::internal;
use roxide_eval::{Ctx, EvalResult, Module};
use roxide_gc::SexpId;
use roxide_value::ValueApi;

/// Registration table for smoothing and nonlinear minimization.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_smooth_density", 8, density::estimate),
        internal("C_smooth_pair_counts", 2, density::pair_counts),
        internal("C_smooth_bw_score", 5, density::bandwidth_score),
        internal("C_smooth_approx", 5, interpolate::approx),
        internal("C_smooth_cubic", 3, interpolate::cubic),
        internal("C_smooth_spline", 6, interpolate::spline),
        internal("C_smooth_lowess", 5, lowess::fit),
        internal("C_smooth_loess", 5, loess::fit),
        internal("C_smooth_loess_predict", 6, loess::predict),
        internal("C_smooth_two_auto_rows", 1, loess::two_auto_rows),
        internal("C_smooth_nonlinear", 9, nonlinear::fit),
    ],
    prelude: concat!(
        include_str!("smooth/interpolate.R"),
        include_str!("smooth/density.R"),
        include_str!("smooth/loess.R"),
        include_str!("smooth/nonlinear.R"),
    ),
};

fn reals(ctx: &mut Ctx, values: &[f64]) -> EvalResult {
    let id = ctx.session_mut().alloc_real_from(values)?;
    ctx.protect(id);
    Ok(id)
}

fn list(ctx: &mut Ctx, values: &[SexpId]) -> EvalResult {
    let id = ctx.session_mut().alloc_list(values.len())?;
    ctx.protect(id);
    for (i, &value) in values.iter().enumerate() {
        ctx.session_mut().list_set(id, i, value)?;
    }
    Ok(id)
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_unstable_by(f64::total_cmp);
    let middle = values.len() / 2;
    (values[(values.len() - 1) / 2] + values[middle]) * 0.5
}

//! Descriptive statistics in the stats namespace.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod covariance;
mod order;

use roxide_eval::{Args, Call, Ctx, EvalResult, Module};
use roxide_value::{NA_REAL, SEXP_NIL, ValueApi, is_na_real};

use crate::distn::support::{internal, numbers};

/// Registration table for the descriptive statistics functions.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_var", 4, covariance::variance),
        internal("C_sd", 2, covariance::sd),
        internal("C_cov", 4, covariance::cov),
        internal("C_cor", 4, covariance::cor),
        internal("C_cov2cor", 1, covariance::cov2cor),
        internal("C_median", 2, order::median),
        internal("C_quantile", 7, order::quantile),
        internal("C_mad", 3, order::mad),
        internal("C_fivenum", 2, order::fivenum),
        internal("C_weighted_mean", 4, weighted_mean),
    ],
    prelude: include_str!("descriptive/prelude.R"),
};

fn boolean(ctx: &mut Ctx, call: &Call, value: roxide_gc::SexpId) -> EvalResult<bool> {
    match ctx.session().xlength(value)? {
        0 => return Err(ctx.stop(call.err_call(), "argument is of length zero")),
        1 => {}
        _ => return Err(ctx.stop(call.err_call(), "the condition has length > 1")),
    }
    crate::distn::support::logical(ctx, call, value)?
        .ok_or_else(|| ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"))
}

fn average(values: &[f64]) -> f64 {
    let n = values.len() as f64;
    let mut mean = values.iter().sum::<f64>() / n;
    if mean.is_infinite() && values.iter().all(|x| x.is_finite()) {
        mean = values.iter().map(|x| x / n).sum();
    }
    if mean.is_finite() {
        mean += values.iter().map(|x| (x - mean) / n).sum::<f64>();
    }
    mean
}

fn weighted_mean(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let na_rm = boolean(ctx, call, args.get(2))?;
    let unweighted = boolean(ctx, call, args.get(3))?;
    if !unweighted && ctx.session().xlength(args.get(0))? != ctx.session().xlength(args.get(1))? {
        return Err(ctx.stop(call.err_call(), "'x' and 'w' must have the same length"));
    }
    if !super::distn::support::numeric(ctx, args.get(0))? && args.get(0) != SEXP_NIL {
        ctx.warn_now(
            call.err_call(),
            "argument is not numeric or logical: returning NA",
        )?;
        return Ok(ctx.session_mut().scalar_real(NA_REAL)?);
    }
    let mut x = numbers(ctx, call, args.get(0))?;
    let result = if unweighted {
        if na_rm {
            x.retain(|v| !v.is_nan());
        }
        if x.iter().copied().any(is_na_real) {
            NA_REAL
        } else {
            average(&x)
        }
    } else {
        let weights = numbers(ctx, call, args.get(1))?;
        let (mut numerator, mut denominator) = (0.0, 0.0);
        let mut missing = false;
        for (x, w) in x.into_iter().zip(weights) {
            if na_rm && x.is_nan() {
                continue;
            }
            missing |= is_na_real(w) || (w != 0.0 && is_na_real(x));
            denominator += w;
            if w != 0.0 {
                numerator += x * w;
            }
        }
        if missing {
            NA_REAL
        } else {
            numerator / denominator
        }
    };
    Ok(ctx.session_mut().scalar_real(result)?)
}

//! Fourier transforms and time-series methods in the stats namespace.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod correlation;
mod fourier;
mod series;

use roxide_eval::{Args, Call, Ctx, EvalResult, Module};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, ValueApi};

use crate::distn::support::{attr, internal, limit, number};

/// Registration table for Fourier and time-series functions.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_ts_fft", 3, fourier::transform),
        internal("C_ts_nextn", 2, fourier::next_length),
        internal("C_ts_convolve", 4, fourier::convolve),
        internal("C_ts_extent", 3, extent),
        internal("C_ts_set_tsp", 2, series::set_tsp),
        internal("C_ts_filter", 6, series::filter),
        internal("C_ts_format", 2, series::format),
        internal("C_ts_acf", 5, correlation::acf),
        internal("C_ts_pacf", 3, correlation::pacf),
    ],
    prelude: include_str!("ts/prelude.R"),
};

fn dimensions(ctx: &mut Ctx, x: SexpId) -> EvalResult<Vec<usize>> {
    let dim = attr(ctx, x, "dim")?;
    if dim == SEXP_NIL {
        return Ok(Vec::new());
    }
    Ok(ctx.session().int(dim)?.iter().map(|v| v as usize).collect())
}

fn shape(ctx: &mut Ctx, x: SexpId) -> EvalResult<(usize, usize)> {
    match dimensions(ctx, x)?.as_slice() {
        [rows, cols] => Ok((*rows, *cols)),
        _ => Ok((ctx.session().xlength(x)?, 1)),
    }
}

fn extent(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let rows = number(ctx, call, args.get(0))?;
    let cols = number(ctx, call, args.get(1))?;
    let width = number(ctx, call, args.get(2))?;
    let bytes = rows * cols * width;
    if !rows.is_finite()
        || rows < 0.0
        || rows > f64::from(i32::MAX)
        || bytes > ctx.host().limits().max_vector_bytes as f64
    {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    Ok(ctx.session_mut().scalar_int(rows as i32)?)
}

fn text_arg(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<String> {
    let Some(&id) = ctx.session().str_slice(x)?.first() else {
        return Err(ctx.stop(call.err_call(), "invalid character argument"));
    };
    Ok(String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned())
}

fn array(ctx: &mut Ctx, call: &Call, data: &[f64], dims: &[usize]) -> EvalResult {
    use crate::distn::support::set_attr;

    limit::<f64>(ctx, call, data.len())?;
    let dims = dims
        .iter()
        .map(|&d| i32::try_from(d))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ctx.stop(call.err_call(), "vector size limit exceeded"))?;
    let x = ctx.session_mut().alloc_real_from(data)?;
    ctx.protect(x);
    let dim = ctx.session_mut().alloc_int_from(&dims)?;
    ctx.protect(dim);
    set_attr(ctx, x, "dim", dim)?;
    Ok(x)
}

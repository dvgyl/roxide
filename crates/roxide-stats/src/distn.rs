//! Vectorized bindings for the stats distribution functions.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod bindings;
mod multinomial;
/// Shared binding operations for the stats modules.
pub(super) mod support;

use roxide_base::random::with_rng;
use roxide_eval::{Args, Call, Ctx, EvalResult, Module};
use roxide_gc::SexpId;
use roxide_math::core::{Flags, Tail};
use roxide_math::rng::Rng;
use roxide_value::{NA_INTEGER, NA_REAL, ValueApi, is_na_real};

use support::{copy_attrs, has_class, limit, logical, number, numbers, numeric};

/// Registration table for the distribution functions in the stats namespace.
pub const MODULE: Module = Module {
    prims: bindings::TABLE,
    prelude: include_str!("distn/prelude.R"),
};

fn check_numeric_args(
    ctx: &mut Ctx,
    call: &Call,
    inputs: &[SexpId],
    error: &str,
) -> EvalResult<()> {
    for &value in inputs {
        if !numeric(ctx, value)? || has_class(ctx, value, "factor")? {
            return Err(ctx.stop(call.err_call(), error));
        }
    }
    Ok(())
}

fn columns<const N: usize>(
    ctx: &mut Ctx,
    call: &Call,
    args: &Args,
    offset: usize,
) -> EvalResult<[Vec<f64>; N]> {
    let mut columns = [const { Vec::new() }; N];
    for (i, column) in columns.iter_mut().enumerate() {
        *column = numbers(ctx, call, args.get(i + offset))?;
    }
    Ok(columns)
}

#[derive(Clone, Copy)]
enum KernelWarnings {
    Default,
    BinomialSize,
    TukeyQuantile,
}

fn density<const N: usize>(
    ctx: &mut Ctx,
    call: &Call,
    args: &Args,
    kernel: impl Fn([f64; N], bool, &mut Flags) -> f64,
) -> EvalResult {
    let log = logical(ctx, call, args.get(N))? != Some(false);
    vectorize(ctx, call, args, KernelWarnings::Default, |row, flags| {
        kernel(row, log, flags)
    })
}

fn tail<const N: usize>(
    ctx: &mut Ctx,
    call: &Call,
    args: &Args,
    warnings: KernelWarnings,
    kernel: impl Fn([f64; N], Tail, &mut Flags) -> f64,
) -> EvalResult {
    let tail = Tail {
        lower: logical(ctx, call, args.get(N))? != Some(false),
        log: logical(ctx, call, args.get(N + 1))? != Some(false),
    };
    vectorize(ctx, call, args, warnings, |row, flags| {
        kernel(row, tail, flags)
    })
}

fn vectorize<const N: usize>(
    ctx: &mut Ctx,
    call: &Call,
    args: &Args,
    warnings: KernelWarnings,
    kernel: impl Fn([f64; N], &mut Flags) -> f64,
) -> EvalResult {
    let inputs = &args.values()[..N];
    check_numeric_args(
        ctx,
        call,
        inputs,
        "Non-numeric argument to mathematical function",
    )?;
    let mut longest = (0, 0);
    for (index, &value) in inputs.iter().enumerate() {
        let len = ctx.session().xlength(value)?;
        if len == 0 {
            return Ok(ctx.session_mut().alloc_real_from(&[])?);
        }
        if len > longest.1 {
            longest = (index, len);
        }
    }
    let (source, len) = longest;
    let columns = columns::<N>(ctx, call, args, 0)?;
    limit::<f64>(ctx, call, len)?;
    let mut out = Vec::with_capacity(len);
    let mut invalid = false;
    for i in 0..len {
        let row = recycled_row(&columns, i);
        let mut flags = Flags::default();
        let value = if row.iter().copied().any(is_na_real) {
            NA_REAL
        } else if row.iter().any(|x| x.is_nan()) {
            f64::NAN
        } else {
            kernel(row, &mut flags)
        };
        if flags.has(Flags::NONINT_X) {
            let (parameter, value) = if matches!(warnings, KernelWarnings::BinomialSize) {
                ("n", row[1])
            } else {
                ("x", row[0])
            };
            ctx.warn_now(
                call.err_call(),
                format!("non-integer {parameter} = {value:.6}"),
            )?;
        }
        if flags.has(Flags::NO_CONVERGENCE) && matches!(warnings, KernelWarnings::TukeyQuantile) {
            ctx.warn_now(call.err_call(), "convergence failed in 'qtukey'")?;
        }
        invalid |= flags.has(Flags::NAN_PRODUCED);
        out.push(value);
    }
    if invalid {
        ctx.warn_now(call.err_call(), "NaNs produced")?;
    }
    ctx.scoped(|ctx| {
        let value = ctx.session_mut().alloc_real_from(&out)?;
        ctx.protect(value);
        copy_attrs(ctx, args.get(source), value)?;
        Ok(value)
    })
}

fn recycled_row<const N: usize>(columns: &[Vec<f64>; N], i: usize) -> [f64; N] {
    std::array::from_fn(|j| columns[j][i % columns[j].len()])
}

#[derive(Clone, Copy)]
enum RandomOutput {
    Integer,
    Real,
}

fn draw_count(ctx: &mut Ctx, call: &Call, n: SexpId) -> EvalResult<usize> {
    let len = ctx.session().xlength(n)?;
    if len > 1 {
        return Ok(len);
    }
    let n = number(ctx, call, n)?;
    if n.is_finite() && n >= 0.0 && n < usize::MAX as f64 {
        Ok(n as usize)
    } else {
        Err(ctx.stop(call.err_call(), "invalid arguments"))
    }
}

fn random<const N: usize>(
    ctx: &mut Ctx,
    call: &Call,
    args: &Args,
    output: RandomOutput,
    kernel: impl Fn([f64; N], &mut Rng, &mut Flags) -> f64,
) -> EvalResult {
    check_numeric_args(ctx, call, &args.values()[1..], "invalid arguments")?;
    let len = draw_count(ctx, call, args.get(0))?;
    limit::<f64>(ctx, call, len)?;
    if len == 0 {
        return match output {
            RandomOutput::Integer => Ok(ctx.session_mut().alloc_int_from(&[])?),
            RandomOutput::Real => Ok(ctx.session_mut().alloc_real_from(&[])?),
        };
    }
    let columns = columns::<N>(ctx, call, args, 1)?;
    let empty = columns.iter().any(Vec::is_empty);
    let mut out = vec![NA_REAL; len];
    let invalid = if empty {
        true
    } else {
        with_rng(ctx, call, |rng| {
            let mut invalid = false;
            for (i, value) in out.iter_mut().enumerate() {
                let row = recycled_row(&columns, i);
                let mut flags = Flags::default();
                let draw = if row.iter().any(|x| x.is_nan()) {
                    f64::NAN
                } else {
                    kernel(row, rng, &mut flags)
                };
                invalid |= draw.is_nan();
                *value = if draw.is_nan() && matches!(output, RandomOutput::Integer) {
                    NA_REAL
                } else {
                    draw
                };
            }
            invalid
        })?
    };
    if invalid {
        ctx.warn_now(call.err_call(), "NAs produced")?;
    }
    if matches!(output, RandomOutput::Integer)
        && out
            .iter()
            .all(|&x| is_na_real(x) || (0.0..=f64::from(i32::MAX)).contains(&x))
    {
        let integers: Vec<_> = out
            .iter()
            .map(|&x| if is_na_real(x) { NA_INTEGER } else { x as i32 })
            .collect();
        Ok(ctx.session_mut().alloc_int_from(&integers)?)
    } else {
        Ok(ctx.session_mut().alloc_real_from(&out)?)
    }
}

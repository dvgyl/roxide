use roxide_base::random::with_rng;
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{core::Flags, dist::discrete};
use roxide_value::{SEXP_NIL, ValueApi};

use super::support::{attr, limit, logical, number, numbers, set_dim, set_dimnames};

pub(super) fn density(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut counts = numbers(ctx, call, args.get(0))?;
    let prob = numbers(ctx, call, args.get(2))?;
    if counts.len() != prob.len() {
        return Err(ctx.stop(
            call.err_call(),
            "x[] and prob[] must be equal length vectors.",
        ));
    }
    if prob.iter().any(|p| !p.is_finite() || *p < 0.0) || !prob.iter().any(|p| *p > 0.0) {
        return Err(ctx.stop(
            call.err_call(),
            "probabilities must be finite, non-negative and not all 0",
        ));
    }
    for count in &mut counts {
        *count = (*count + 0.5).floor();
        if count.is_infinite() || *count > f64::from(i32::MAX) || *count <= f64::from(i32::MIN) {
            ctx.warn_now(
                call.err_call(),
                "NAs introduced by coercion to integer range",
            )?;
            *count = f64::NAN;
        }
    }
    if counts.iter().any(|x| *x < -1.0) {
        return Err(ctx.stop(call.err_call(), "'x' must be non-negative"));
    }
    if counts.iter().any(|x| x.is_nan()) {
        return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
    }
    let size: f64 = counts
        .iter_mut()
        .map(|x| {
            *x = x.max(0.0);
            *x
        })
        .sum();
    if args.get(1) != SEXP_NIL {
        let given = number(ctx, call, args.get(1))?;
        match ctx.session().xlength(args.get(1))? {
            0 => return Err(ctx.stop(call.err_call(), "argument is of length zero")),
            1 => {}
            _ => return Err(ctx.stop(call.err_call(), "the condition has length > 1")),
        }
        if given.is_nan() {
            return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
        }
        if given != size {
            return Err(ctx.stop(call.err_call(), "size != sum(x), i.e. one is wrong"));
        }
    }
    if prob.iter().sum::<f64>().is_infinite() {
        return Ok(ctx.session_mut().scalar_real(0.0)?);
    }
    let Some(log) = logical(ctx, call, args.get(3))? else {
        return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
    };
    let value = discrete::dmultinom(&counts, &prob, log, &mut Flags::default());
    Ok(ctx.session_mut().scalar_real(value)?)
}

pub(super) fn random(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let count = number(ctx, call, args.get(0))?.trunc();
    if !(0.0..=f64::from(i32::MAX)).contains(&count) {
        return Err(ctx.stop(call.err_call(), "invalid first argument 'n'"));
    }
    let size = number(ctx, call, args.get(1))?.trunc();
    if !(0.0..=f64::from(i32::MAX)).contains(&size) {
        return Err(ctx.stop(call.err_call(), "invalid second argument 'size'"));
    }
    let prob = numbers(ctx, call, args.get(2))?;
    for &p in &prob {
        if !p.is_finite() {
            return Err(ctx.stop(call.err_call(), "NA in probability vector"));
        }
        if p < 0.0 {
            return Err(ctx.stop(call.err_call(), "negative probability"));
        }
    }
    if !prob.iter().any(|p| *p > 0.0) {
        return Err(ctx.stop(call.err_call(), "no positive probabilities"));
    }
    let count = count as usize;
    let len = prob.len().saturating_mul(count);
    limit::<i32>(ctx, call, len)?;
    let mut result = vec![0; len];
    if count != 0 {
        let drawn = with_rng(ctx, call, |rng| {
            result
                .chunks_exact_mut(prob.len())
                .try_for_each(|column| discrete::rmultinom(rng, size as i32, &prob, column))
        })?;
        if drawn.is_err() {
            return Err(ctx.stop(call.err_call(), "cannot draw multinomial sample"));
        }
    }
    ctx.scoped(|ctx| {
        let names = attr(ctx, args.get(2), "names")?;
        let value = ctx.session_mut().alloc_int_from(&result)?;
        ctx.protect(value);
        set_dim(ctx, call, value, prob.len(), count)?;
        set_dimnames(ctx, value, names, SEXP_NIL)?;
        Ok(value)
    })
}

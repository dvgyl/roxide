use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{MatRef, linalg::linpack::dqrls};
use roxide_value::{NA_REAL, ValueApi};

use crate::distn::support::{attr, limit, logical, number, numbers};

pub(super) fn least_squares(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let xd = attr(ctx, args.get(0), "dim")?;
    let dims = ctx.session().int(xd)?;
    if dims.len() != 2 || dims.get(0) <= 0 || dims.get(1) <= 0 {
        return Err(ctx.stop(call.err_call(), "incompatible dimensions"));
    }
    let (n, p) = (dims.get(0) as usize, dims.get(1) as usize);
    let x = numbers(ctx, call, args.get(0))?;
    let y = numbers(ctx, call, args.get(1))?;
    let tol = number(ctx, call, args.get(2))?;
    let singular = logical(ctx, call, args.get(3))?;
    if x.iter().any(|v| !v.is_finite()) {
        return Err(ctx.stop(call.err_call(), "NA/NaN/Inf in 'x'"));
    }
    if y.iter().any(|v| !v.is_finite()) {
        return Err(ctx.stop(call.err_call(), "NA/NaN/Inf in 'y'"));
    }
    if y.is_empty() {
        return Err(ctx.stop(call.err_call(), "incompatible dimensions"));
    }
    let responses = y.len() / n;
    limit::<f64>(ctx, call, p.saturating_mul(responses))?;
    limit::<usize>(ctx, call, p)?;
    let response = MatRef::new(&y, n, responses)
        .map_err(|_| ctx.stop(call.err_call(), "incompatible dimensions"))?;
    let fit = dqrls(x, n, p, response, tol)
        .map_err(|_| ctx.stop(call.err_call(), "incompatible dimensions"))?;
    if fit.qr.rank < p {
        match singular {
            Some(true) => (),
            Some(false) => return Err(ctx.stop(call.err_call(), "singular fit encountered")),
            None => return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed")),
        }
    }

    let mut coefficients = vec![NA_REAL; fit.coefficients.len()];
    for (out, packed) in coefficients
        .chunks_exact_mut(p)
        .zip(fit.coefficients.chunks_exact(p))
    {
        for (&column, &value) in fit.qr.pivot[..fit.qr.rank].iter().zip(packed) {
            out[column] = value;
        }
    }
    let pivot = fit
        .qr
        .pivot
        .iter()
        .map(|&i| (i + 1) as i32)
        .collect::<Vec<_>>();
    let result = ctx.session_mut().alloc_list(8)?;
    ctx.protect(result);
    for (i, values) in [
        &coefficients,
        &fit.residuals,
        &fit.effects,
        &fit.fitted,
        &fit.qr.qr,
        &fit.qr.qraux,
    ]
    .into_iter()
    .enumerate()
    {
        let value = ctx.session_mut().alloc_real_from(values)?;
        ctx.protect(value);
        ctx.session_mut().list_set(result, i, value)?;
    }
    let pivot = ctx.session_mut().alloc_int_from(&pivot)?;
    ctx.protect(pivot);
    ctx.session_mut().list_set(result, 6, pivot)?;
    let rank = ctx.session_mut().alloc_int_from(&[fit.qr.rank as i32])?;
    ctx.protect(rank);
    ctx.session_mut().list_set(result, 7, rank)?;
    Ok(result)
}

pub(super) fn language(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    use roxide_gc::SexpId;
    use roxide_value::SEXP_NIL;

    let fields = args.get(1);
    limit::<SexpId>(ctx, call, ctx.session().xlength(fields)?)?;
    let labels = attr(ctx, fields, "names")?;
    let names = if labels == SEXP_NIL {
        Vec::new()
    } else {
        ctx.session().str_slice(labels)?.to_vec()
    };
    let values = ctx.session().list_slice(fields)?.to_vec();
    let mut tail = SEXP_NIL;
    for (index, &value) in values.iter().enumerate().rev() {
        let tag = if let Some(&name) = names.get(index) {
            let name = String::from_utf8_lossy(ctx.session().char_bytes(name)?).into_owned();
            if name.is_empty() {
                SEXP_NIL
            } else {
                ctx.session_mut().intern(&name)?.as_sexp()
            }
        } else {
            SEXP_NIL
        };
        tail = ctx.session_mut().cons(value, tail, tag)?;
        ctx.protect(tail);
    }
    Ok(ctx.session_mut().lcons(args.get(0), tail, SEXP_NIL)?)
}

pub(super) fn size(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let n = number(ctx, call, args.get(0))?;
    if !n.is_finite() || n > f64::from(i32::MAX) {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    limit::<roxide_gc::SexpId>(ctx, call, n as usize)?;
    Ok(roxide_value::SEXP_NIL)
}

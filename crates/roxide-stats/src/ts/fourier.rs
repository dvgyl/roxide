use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_math::{
    Cplx, MathError,
    fft::{NextLengthError, fft, fft_axis, fft_nd, nextn},
};
use roxide_value::{CharId, NA_INTEGER, RComplex, SEXP_NIL, SexpType, ValueApi};

use crate::distn::support::{attr, copy_attrs, limit, logical, numbers, numeric, set_attr};

use super::{dimensions, text_arg};

fn complex_input(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Vec<Cplx>> {
    limit::<Cplx>(ctx, call, ctx.session().xlength(x)?)?;
    if ctx.session().type_of(x)? == SexpType::Cplx {
        return Ok(ctx
            .session()
            .cplx(x)?
            .iter()
            .map(|z| Cplx::new(z.re, z.im))
            .collect());
    }
    if !numeric(ctx, x)? {
        return Err(ctx.stop(call.err_call(), "non-numeric argument"));
    }
    Ok(numbers(ctx, call, x)?
        .into_iter()
        .map(|re| Cplx::new(re, 0.0))
        .collect())
}

fn alloc_complex(ctx: &mut Ctx, data: &[Cplx]) -> EvalResult {
    let values: Vec<_> = data
        .iter()
        .map(|z| RComplex { re: z.re, im: z.im })
        .collect();
    Ok(ctx.session_mut().alloc_cplx_from(&values)?)
}

fn transform_limit(ctx: &mut Ctx) -> usize {
    ctx.host().limits().max_vector_bytes / size_of::<Cplx>()
}

fn fft_error(ctx: &mut Ctx, call: &Call, error: MathError) -> roxide_eval::Unwind {
    if error == MathError::TooLarge {
        ctx.stop(call.err_call(), "vector size limit exceeded")
    } else {
        ctx.stop(call.err_call(), error.to_string())
    }
}

pub(super) fn transform(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let inverse = logical(ctx, call, args.get(1))? == Some(true);
    let separate = logical(ctx, call, args.get(2))? == Some(true);
    let dims = dimensions(ctx, x)?;
    if separate && dims.len() != 2 {
        return Err(ctx.stop(
            call.err_call(),
            "vector-valued (multivariate) series required",
        ));
    }
    let mut data = complex_input(ctx, call, x)?;
    let limit = transform_limit(ctx);
    let result = match dims.as_slice() {
        [rows, _] if separate => fft_axis(&mut data, *rows, 1, inverse, limit),
        [] => fft(&mut data, inverse, limit),
        _ => fft_nd(&mut data, &dims, inverse, limit),
    };
    result.map_err(|e| fft_error(ctx, call, e))?;
    let out = alloc_complex(ctx, &data)?;
    ctx.protect(out);
    copy_attrs(ctx, x, out)?;
    Ok(out)
}

pub(super) fn next_length(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let n = args.get(0);
    if n != SEXP_NIL && !matches!(ctx.session().type_of(n)?, SexpType::Int | SexpType::Real) {
        return Err(ctx.stop(
            call.err_call(),
            r#"'n' must have typeof(.) "integer" or "double""#,
        ));
    }
    let n = numbers(ctx, call, n)?;
    let factors = numbers(ctx, call, args.get(1))?;
    if factors
        .iter()
        .any(|&x| !x.is_nan() && (x <= f64::from(i32::MIN) || x.trunc() > f64::from(i32::MAX)))
    {
        ctx.warn_now(
            call.err_call(),
            "NAs introduced by coercion to integer range",
        )?;
    }
    let factors: Vec<_> = factors
        .into_iter()
        .map(|f| {
            if f.is_finite() && f >= 2.0 && f.trunc() <= f64::from(i32::MAX) {
                f as u64
            } else {
                0
            }
        })
        .collect();
    if factors.is_empty() {
        return Err(ctx.stop(call.err_call(), "no factors"));
    }
    if factors.contains(&0) {
        return Err(ctx.stop(call.err_call(), "invalid factors"));
    }
    let mut double = n.iter().any(|&x| x > f64::from(i32::MAX));
    let mut no_result = false;
    let mut out = Vec::with_capacity(n.len());
    for &x in &n {
        let value = if x.is_nan() {
            roxide_value::NA_REAL
        } else {
            match nextn(x as u64, &factors) {
                Ok(Some(value)) => {
                    if value > 1 << 53 {
                        ctx.warn_now(
                            call.err_call(),
                            format!(r#"nextn() = {value} > 2^53 may not be exactly representable in R (as "double")"#),
                        )?;
                    }
                    value as f64
                }
                Ok(None) => {
                    no_result = true;
                    0.0
                }
                Err(NextLengthError::SearchLimit) => {
                    return Err(ctx.stop(call.err_call(), "transform length search limit exceeded"));
                }
                Err(_) => return Err(ctx.stop(call.err_call(), "invalid factors")),
            }
        };
        double |= value > f64::from(i32::MAX);
        out.push(value);
    }
    if !double && n.iter().any(|&x| x <= f64::from(i32::MIN)) {
        ctx.warn_now(
            call.err_call(),
            "NAs introduced by coercion to integer range",
        )?;
        for (value, x) in out.iter_mut().zip(n) {
            if x <= f64::from(i32::MIN) {
                *value = roxide_value::NA_REAL;
            }
        }
    }
    if no_result {
        ctx.warn_now(call.err_call(), "nextn<64>() found no solution < 18446744073709551615 = UINT64_MAX (the maximal integer)")?;
    }
    if double {
        Ok(ctx.session_mut().alloc_real_from(&out)?)
    } else {
        let out: Vec<_> = out
            .iter()
            .map(|&x| if x.is_nan() { NA_INTEGER } else { x as i32 })
            .collect();
        Ok(ctx.session_mut().alloc_int_from(&out)?)
    }
}

pub(super) fn convolve(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let y = args.get(1);
    let mode = text_arg(ctx, call, args.get(3))?;
    let (nx, ny) = (ctx.session().xlength(x)?, ctx.session().xlength(y)?);
    let circular = mode == "circular";
    if circular && nx != ny {
        return Err(ctx.stop(call.err_call(), "length mismatch in convolution"));
    }
    let conj = logical(ctx, call, args.get(2))?
        .ok_or_else(|| ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"))?;
    let (xdim, ydim) = if circular {
        (dimensions(ctx, x)?, dimensions(ctx, y)?)
    } else {
        if nx == 0 || ny == 0 {
            return Err(ctx.stop(call.err_call(), "invalid 'times' value"));
        }
        (Vec::new(), Vec::new())
    };
    if !xdim.is_empty() && !ydim.is_empty() && xdim != ydim {
        return Err(ctx.stop(call.err_call(), "non-conformable arrays"));
    }
    let pad = if circular {
        0
    } else {
        ny.checked_sub(1)
            .ok_or_else(|| ctx.stop(call.err_call(), "invalid 'times' value"))?
    };
    let len = nx
        .checked_add(pad)
        .filter(|&n| n <= transform_limit(ctx))
        .ok_or_else(|| ctx.stop(call.err_call(), "vector size limit exceeded"))?;
    let mut a = complex_input(ctx, call, x)?;
    let mut b = complex_input(ctx, call, y)?;
    let (from, to) = if mode == "filter" {
        (pad, nx.max(pad))
    } else {
        (0, len)
    };
    if from < to {
        a.resize(len, Cplx::default());
        a.rotate_right(pad);
        b.resize(len, Cplx::default());
        let limit = transform_limit(ctx);
        transform_data(ctx, call, &mut a, &xdim, false, limit)?;
        transform_data(ctx, call, &mut b, &ydim, false, limit)?;
        for (a, b) in a.iter_mut().zip(b) {
            let im = if conj { -b.im } else { b.im };
            *a = Cplx::new(a.re * b.re - a.im * im, a.im * b.re + a.re * im);
        }
        let dim = if xdim.is_empty() { &ydim } else { &xdim };
        transform_data(ctx, call, &mut a, dim, true, limit)?;
        for a in &mut a {
            a.re /= len as f64;
            a.im /= len as f64;
        }
    }
    let data = if from < to { &a[from..to] } else { &[] };
    let out = if ctx.session().type_of(x)? == SexpType::Cplx
        || ctx.session().type_of(y)? == SexpType::Cplx
    {
        alloc_complex(ctx, data)?
    } else {
        let real: Vec<_> = data.iter().map(|z| z.re).collect();
        ctx.session_mut().alloc_real_from(&real)?
    };
    ctx.protect(out);
    if circular {
        copy_attrs(ctx, y, out)?;
        copy_attrs(ctx, x, out)?;
        return Ok(out);
    }
    let xnames = attr(ctx, x, "names")?;
    let ynames = attr(ctx, y, "names")?;
    let (names, offset) = if xnames != SEXP_NIL {
        (xnames, pad)
    } else {
        (ynames, 0)
    };
    if names != SEXP_NIL && !data.is_empty() {
        let names = ctx.session().str_slice(names)?;
        let selected: Vec<_> = (from..to)
            .map(|i| {
                i.checked_sub(offset)
                    .and_then(|j| names.get(j))
                    .copied()
                    .unwrap_or(CharId::BLANK)
            })
            .collect();
        let names = ctx.session_mut().alloc_str_from(&selected)?;
        ctx.protect(names);
        set_attr(ctx, out, "names", names)?;
    }
    Ok(out)
}

fn transform_data(
    ctx: &mut Ctx,
    call: &Call,
    data: &mut [Cplx],
    dim: &[usize],
    inverse: bool,
    limit: usize,
) -> EvalResult<()> {
    if dim.is_empty() {
        fft(data, inverse, limit)
    } else {
        fft_nd(data, dim, inverse, limit)
    }
    .map_err(|e| fft_error(ctx, call, e))
}

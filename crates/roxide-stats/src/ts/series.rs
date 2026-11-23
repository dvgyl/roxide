use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{NA_REAL, SEXP_NIL, SexpType, ValueApi};

use crate::distn::support::{attr, limit, logical, number, numbers, set_attr};

use super::{array, dimensions, shape};

pub(super) fn set_tsp(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let value = args.get(1);
    if value == SEXP_NIL {
        let out = ctx.session_mut().shallow_duplicate(x)?;
        ctx.protect(out);
        set_attr(ctx, out, "tsp", SEXP_NIL)?;
        let class = attr(ctx, out, "class")?;
        if class != SEXP_NIL {
            let s = ctx.session();
            let classes: Vec<_> = s
                .str_slice(class)?
                .iter()
                .copied()
                .filter(|&id| !matches!(s.char_bytes(id), Ok(b"ts" | b"mts")))
                .collect();
            let class = if classes.is_empty() {
                SEXP_NIL
            } else {
                ctx.session_mut().alloc_str_from(&classes)?
            };
            ctx.protect(class);
            set_attr(ctx, out, "class", class)?;
        }
        return Ok(out);
    }
    if !matches!(
        ctx.session().type_of(value)?,
        SexpType::Int | SexpType::Real
    ) || ctx.session().xlength(value)? != 3
    {
        return Err(ctx.stop(
            call.err_call(),
            "'tsp' attribute must be numeric of length three",
        ));
    }
    let [start, end, frequency] =
        numbers(ctx, call, value)?
            .as_slice()
            .try_into()
            .map_err(|_| {
                ctx.stop(
                    call.err_call(),
                    "'tsp' attribute must be numeric of length three",
                )
            })?;
    let rows = dimensions(ctx, x)?
        .first()
        .copied()
        .unwrap_or(ctx.session().xlength(x)?);
    if rows == 0 {
        return Err(ctx.stop(call.err_call(), "cannot assign 'tsp' to zero-length vector"));
    }
    if frequency <= 0.0 {
        return Err(ctx.stop(
            call.err_call(),
            "invalid time series parameters specified (0)",
        ));
    }
    let tolerance = ctx.scoped(|ctx| {
        let key = ctx.session_mut().scalar_str("ts.eps")?;
        let tolerance = ctx.call_function("getOption", &[(None, key)], roxide_value::BASE_ENV)?;
        if tolerance == SEXP_NIL {
            Ok(1e-5)
        } else {
            number(ctx, call, tolerance)
        }
    })?;
    if ((end - start) * frequency - (rows - 1) as f64).abs() > tolerance {
        return Err(ctx.stop(
            call.err_call(),
            "invalid time series parameters specified (1)",
        ));
    }
    let value = ctx
        .session_mut()
        .alloc_real_from(&[start, end, frequency])?;
    ctx.protect(value);
    let out = ctx.session_mut().shallow_duplicate(x)?;
    ctx.protect(out);
    set_attr(ctx, out, "tsp", value)?;
    Ok(out)
}

pub(super) fn filter(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let (rows, cols) = shape(ctx, x)?;
    if rows == 0 {
        return Err(ctx.stop(
            call.err_call(),
            "'ts' object must have one or more observations",
        ));
    }
    let data = numbers(ctx, call, x)?;
    if rows.checked_mul(cols) != Some(data.len()) {
        return Err(ctx.stop(call.err_call(), "invalid time series"));
    }
    let weights = numbers(ctx, call, args.get(1))?;
    if weights.iter().any(|x| x.is_nan()) {
        return Err(ctx.stop(call.err_call(), "missing values in 'filter'"));
    }
    let width = weights.len();
    let recursive = logical(ctx, call, args.get(2))? == Some(true);
    if !recursive && width > rows {
        return Err(ctx.stop(call.err_call(), "'filter' is longer than time series"));
    }
    let mut output = vec![0.0; data.len()];
    if recursive {
        let init_len = width
            .checked_mul(cols)
            .ok_or_else(|| ctx.stop(call.err_call(), "vector size limit exceeded"))?;
        let init = if args.get(5) == SEXP_NIL {
            limit::<f64>(ctx, call, init_len)?;
            vec![0.0; init_len]
        } else {
            numbers(ctx, call, args.get(5))?
        };
        if init.len() != init_len {
            let message = if cols > 1 && init.len() == width {
                format!(
                    "dims [product {}] do not match the length of object [{}]",
                    width * cols,
                    init.len()
                )
            } else {
                "length of 'init' must equal length of 'filter'".to_owned()
            };
            return Err(ctx.stop(call.err_call(), message));
        }
        for (col, (input, out)) in data
            .chunks_exact(rows)
            .zip(output.chunks_exact_mut(rows))
            .enumerate()
        {
            let init = &init[col * width..(col + 1) * width];
            for i in 0..rows {
                let mut value = input[i];
                if value.is_nan() {
                    out[i] = NA_REAL;
                    continue;
                }
                for (j, &weight) in weights.iter().enumerate() {
                    let previous = if j < i { out[i - j - 1] } else { init[j - i] };
                    if previous.is_nan() {
                        value = NA_REAL;
                        break;
                    }
                    value += weight * previous;
                }
                out[i] = value;
            }
        }
    } else {
        let sides = number(ctx, call, args.get(3))?.trunc();
        if sides != 1.0 && sides != 2.0 {
            return Err(ctx.stop(call.err_call(), "argument 'sides' must be 1 or 2"));
        }
        if ctx.session().xlength(args.get(4))? == 0 {
            return Err(ctx.stop(call.err_call(), "argument is of length zero"));
        }
        let circular = logical(ctx, call, args.get(4))?
            .ok_or_else(|| ctx.stop(call.err_call(), "'circular' must be logical and not NA"))?;
        let ahead = if sides == 1.0 { 0 } else { width / 2 };
        for (input, out) in data.chunks_exact(rows).zip(output.chunks_exact_mut(rows)) {
            for (i, value) in out.iter_mut().enumerate() {
                let center = i + ahead;
                if !circular && width != 0 && (center >= rows || center + 1 < width) {
                    *value = NA_REAL;
                    continue;
                }
                *value = weights
                    .iter()
                    .enumerate()
                    .map(|(j, &w)| {
                        let k = (rows + center - j) % rows;
                        if input[k].is_nan() {
                            NA_REAL
                        } else {
                            w * input[k]
                        }
                    })
                    .sum::<f64>()
                    + 0.0;
            }
        }
    }
    if dimensions(ctx, x)?.len() == 2 {
        array(ctx, call, &output, &[rows, cols])
    } else {
        Ok(ctx.session_mut().alloc_real_from(&output)?)
    }
}

pub(super) fn format(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    use roxide_value::{Encoding, FormatOpts, format_int_vec, format_lgl_vec, format_real_vec};

    let x = args.get(0);
    let len = ctx.session().xlength(x)?;
    limit::<String>(ctx, call, len)?;
    let digits = number(ctx, call, args.get(1))?;
    let opts = FormatOpts {
        digits: if digits.is_nan() {
            ctx.opts().digits
        } else {
            digits.clamp(1.0, 22.0) as usize
        },
        scipen: ctx.opts().scipen,
    };
    let s = ctx.session();
    let text = match s.type_of(x)? {
        SexpType::Int => format_int_vec(&s.int(x)?.to_vec()),
        SexpType::Lgl => format_lgl_vec(&s.lgl(x)?.to_vec()),
        SexpType::Real => format_real_vec(&s.real(x)?.to_vec(), opts),
        _ => return Ok(ctx.session_mut().coerce(x, SexpType::Str)?.0),
    };
    let s = ctx.session_mut();
    let chars = text
        .iter()
        .map(|t| s.mk_char(t.as_bytes(), Encoding::Native))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(s.alloc_str_from(&chars)?)
}

use std::fmt::Write;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{FormatOpts, NA_REAL, SEXP_NIL, ValueApi, format_real_vec};

use crate::distn::support::{attr, limit, logical, number, numbers, set_attr};

use super::output::{blocks, labels as display_labels, text_width};
use super::support::{Matrix, area, choice, dimnames, matrix, pair_count, real, size, strings};

pub(super) fn dist(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let method = args.get(1);
    let p = args.get(4);
    let method_name = choice(ctx, method).unwrap_or_default().to_owned();
    let power = if method_name == "minkowski" {
        let power = number(ctx, call, p)?;
        if !power.is_finite() || power <= 0.0 {
            return Err(ctx.stop(call.err_call(), "distance(): invalid p"));
        }
        power
    } else {
        2.0
    };
    let input = Matrix::read(ctx, call, x)?;
    let n = input.rows;
    let mut values = Vec::with_capacity(pair_count(ctx, call, n)?);
    for j in 0..n {
        for i in j + 1..n {
            values.push(pair(ctx, call, &input, i, j, &method_name, power)?);
        }
    }

    let result = real(ctx, &values)?;
    let size = ctx.session_mut().scalar_int(n as i32)?;
    ctx.protect(size);
    let labels = dimnames(ctx, x)?.0;
    let class = strings(ctx, ["dist"])?;
    set_attr(ctx, result, "Size", size)?;
    set_attr(ctx, result, "Labels", labels)?;
    set_attr(ctx, result, "Diag", args.get(2))?;
    set_attr(ctx, result, "Upper", args.get(3))?;
    set_attr(ctx, result, "method", method)?;
    if method_name == "minkowski" {
        set_attr(ctx, result, "p", p)?;
    }
    set_attr(ctx, result, "call", args.get(5))?;
    set_attr(ctx, result, "class", class)?;
    Ok(result)
}

fn pair(
    ctx: &mut Ctx,
    call: &Call,
    x: &Matrix,
    i: usize,
    j: usize,
    method: &str,
    power: f64,
) -> EvalResult<f64> {
    let mut used = 0;
    let mut either = 0;
    let mut total: f64 = 0.0;
    for col in 0..x.cols {
        let a = x.at(i, col);
        let b = x.at(j, col);
        if a.is_nan() || b.is_nan() {
            continue;
        }
        if method == "binary" {
            if !a.is_finite() || !b.is_finite() {
                ctx.warn_now(call.err_call(), "treating non-finite values as NA")?;
                continue;
            }
            let (a, b) = (a != 0.0, b != 0.0);
            either += usize::from(a || b);
            total += f64::from(a != b);
        } else {
            let delta = (a - b).abs();
            if delta.is_nan() {
                continue;
            }
            match method {
                "euclidean" => total += delta * delta,
                "maximum" => total = total.max(delta),
                "manhattan" => total += delta,
                "canberra" => {
                    let denom = a.abs() + b.abs();
                    if denom <= f64::MIN_POSITIVE {
                        continue;
                    }
                    total += if delta.is_infinite() && denom.is_infinite() {
                        1.0
                    } else {
                        delta / denom
                    };
                }
                "minkowski" => total += delta.powf(power),
                _ => return Err(ctx.stop(call.err_call(), "invalid distance method")),
            }
        }
        used += 1;
    }
    if used == 0 {
        return Ok(NA_REAL);
    }
    let scaled = if used == x.cols {
        total
    } else {
        total / (used as f64 / x.cols as f64)
    };
    Ok(match method {
        "binary" => {
            if either == 0 {
                0.0
            } else {
                total / either as f64
            }
        }
        "maximum" => total,
        "euclidean" => scaled.sqrt(),
        "minkowski" => scaled.powf(1.0 / power),
        _ => scaled,
    })
}

pub(super) fn expand(ctx: &mut Ctx, call: &Call, d: SexpId, n: usize) -> EvalResult<Vec<f64>> {
    let len = area::<f64>(ctx, call, n, n)?;
    let mut out = vec![0.0; len];
    let distances = numbers(ctx, call, d)?;
    if n > 1 && distances.is_empty() {
        return Err(ctx.stop(call.err_call(), "replacement has length zero"));
    }
    if !distances.is_empty() && !(len.saturating_sub(n) / 2).is_multiple_of(distances.len()) {
        for _ in 0..2 {
            ctx.warn_now(
                call.err_call(),
                "number of items to replace is not a multiple of replacement length",
            )?;
        }
    }
    let mut values = distances.iter().cycle();
    for j in 0..n {
        for i in j + 1..n {
            if let Some(&value) = values.next() {
                out[j * n + i] = value;
                out[i * n + j] = value;
            }
        }
    }
    Ok(out)
}

fn labels(ctx: &mut Ctx, d: SexpId, n: usize) -> EvalResult {
    let names = attr(ctx, d, "Labels")?;
    if names == SEXP_NIL && n != 0 {
        strings(ctx, (1..=n).map(|i| i.to_string()))
    } else {
        Ok(names)
    }
}

pub(super) fn as_matrix(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let d = args.get(0);
    let n = size(ctx, call, d)?;
    let values = expand(ctx, call, d, n)?;
    let names = labels(ctx, d, n)?;
    let out = real(ctx, &values)?;
    matrix(ctx, call, out, (n, n), (names, names))?;
    if n == 0 {
        let names = ctx.session_mut().alloc_list(2)?;
        ctx.protect(names);
        set_attr(ctx, out, "dimnames", names)?;
    }
    Ok(out)
}

pub(super) fn print(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let d = args.get(0);
    let n = size(ctx, call, d)?;
    if n < 2 || ctx.session().xlength(d)? == 0 {
        let _ = ctx.host().write_out(b"dist(0)\n");
        return Ok(d);
    }
    let mut flags = [false; 2];
    for (i, attr_name) in ["Diag", "Upper"].into_iter().enumerate() {
        let argument = if args.get(i + 1) == SEXP_NIL {
            attr(ctx, d, attr_name)?
        } else {
            args.get(i + 1)
        };
        if argument != SEXP_NIL {
            if ctx.session().xlength(argument)? > 1 {
                return Err(ctx.stop(call.err_call(), "the condition has length > 1"));
            }
            flags[i] = logical(ctx, call, argument)?.ok_or_else(|| {
                ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed")
            })?;
        }
    }
    let [diag, upper] = flags;
    let digits = number(ctx, call, args.get(3))?.clamp(1.0, 22.0) as usize;
    let right = logical(ctx, call, args.get(4))?.unwrap_or(true);
    let values = expand(ctx, call, d, n)?;
    let names = labels(ctx, d, n)?;
    let labels = display_labels(ctx, names)?;
    if labels.len() != n {
        return Err(ctx.stop(call.err_call(), "invalid distance labels"));
    }
    let show = |i, j| (diag && i == j) || (upper && i < j) || i > j;
    let rows = usize::from(!diag && !upper)..n;
    let cols = 0..n - usize::from(!diag && !upper);
    let shown: Vec<_> = cols
        .clone()
        .flat_map(|j| {
            rows.clone()
                .filter(move |&i| show(i, j))
                .map(move |i| (i, j))
        })
        .map(|(i, j)| values[j * n + i])
        .collect();
    limit::<String>(ctx, call, shown.len())?;
    let formatted = format_real_vec(
        &shown,
        FormatOpts {
            digits,
            scipen: ctx.opts().scipen,
        },
    );
    let mut text = vec![""; values.len()];
    let mut next = formatted.iter();
    let row_width = rows
        .clone()
        .map(|i| text_width(&labels[i]))
        .max()
        .unwrap_or(0);
    let mut widths = Vec::new();
    for j in cols.clone() {
        let mut width = text_width(&labels[j]);
        for i in rows.clone().filter(|&i| show(i, j)) {
            if let Some(cell) = next.next() {
                text[j * n + i] = cell;
                width = width.max(text_width(cell));
            }
        }
        widths.push(width);
    }
    blocks(&widths, ctx.opts().width, row_width, |range| {
        let mut output = " ".repeat(row_width);
        for j in range.clone() {
            let width = widths[j];
            if right {
                let _ = write!(output, " {:>width$}", labels[j]);
            } else {
                let _ = write!(output, " {:<width$}", labels[j]);
            }
        }
        output.push('\n');
        let _ = ctx.host().write_out(output.as_bytes());
        for i in rows.clone() {
            output.clear();
            let _ = write!(output, "{:<row_width$}", labels[i]);
            for j in range.clone() {
                let width = widths[j];
                let _ = write!(output, " {:>width$}", text[j * n + i]);
            }
            output.push('\n');
            let _ = ctx.host().write_out(output.as_bytes());
        }
    });
    Ok(d)
}

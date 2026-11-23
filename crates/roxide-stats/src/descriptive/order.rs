use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, NA_INTEGER, NA_REAL, RLogical, SEXP_NIL, SexpType, ValueApi};

use crate::distn::support::{attr, has_class, limit, number, numbers, numeric, set_attr};

use super::{average, boolean};

fn sorted_chars(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    na_rm: bool,
) -> EvalResult<Option<Vec<CharId>>> {
    let values = ctx.session().str_slice(x)?;
    if !na_rm && values.contains(&CharId::NA) {
        return Ok(None);
    }
    limit::<CharId>(ctx, call, values.len())?;
    let mut values: Vec<_> = ctx
        .session()
        .str_slice(x)?
        .iter()
        .copied()
        .filter(|&s| s != CharId::NA)
        .collect();
    values.sort_unstable_by(|&a, &b| {
        let s = ctx.session();
        s.char_bytes(a).ok().cmp(&s.char_bytes(b).ok())
    });
    Ok(Some(values))
}

pub(super) fn median(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let na_rm = boolean(ctx, call, args.get(1))?;
    if x == SEXP_NIL {
        return Ok(x);
    }
    if has_class(ctx, x, "factor")? {
        return Err(ctx.stop(call.err_call(), "need numeric data"));
    }
    let ty = ctx.session().type_of(x)?;
    if ty == SexpType::Str {
        return match sorted_chars(ctx, call, x, na_rm)? {
            Some(chars) if chars.len() % 2 == 1 => Ok(ctx
                .session_mut()
                .alloc_str_from(&[chars[chars.len() / 2]])?),
            None => Ok(ctx.session_mut().alloc_str_from(&[CharId::NA])?),
            Some(chars) if chars.is_empty() => {
                Ok(ctx.session_mut().alloc_str_from(&[CharId::NA])?)
            }
            _ => {
                ctx.warn_now(
                    call.err_call(),
                    "argument is not numeric or logical: returning NA",
                )?;
                Ok(ctx.session_mut().scalar_real(NA_REAL)?)
            }
        };
    }
    if !numeric(ctx, x)? {
        return Err(ctx.stop(call.err_call(), "need numeric data"));
    }
    let mut values = numbers(ctx, call, x)?;
    if na_rm {
        values.retain(|x| !x.is_nan());
    } else if values.iter().any(|x| x.is_nan()) {
        values.clear();
    }
    values.sort_unstable_by(f64::total_cmp);
    let (low, high) = middle(&values);
    let result = if values.is_empty() || values.len() % 2 == 1 {
        match ty {
            SexpType::Int => {
                ctx.session_mut()
                    .scalar_int(if low.is_nan() { NA_INTEGER } else { low as i32 })?
            }
            SexpType::Lgl => ctx.session_mut().scalar_lgl(if low.is_nan() {
                RLogical::NA
            } else {
                if low != 0.0 {
                    RLogical::TRUE
                } else {
                    RLogical::FALSE
                }
            })?,
            _ => ctx.session_mut().scalar_real(low)?,
        }
    } else {
        ctx.session_mut().scalar_real(average(&[low, high]))?
    };
    copy_class(ctx, x, result)?;
    Ok(result)
}

fn copy_class(ctx: &mut Ctx, from: SexpId, to: SexpId) -> EvalResult<()> {
    if !has_class(ctx, from, "Date")?
        && !has_class(ctx, from, "POSIXct")?
        && !has_class(ctx, from, "difftime")?
    {
        return Ok(());
    }
    for name in ["class", "tzone", "units"] {
        let value = attr(ctx, from, name)?;
        if value != SEXP_NIL {
            set_attr(ctx, to, name, value)?;
        }
    }
    Ok(())
}

fn middle(x: &[f64]) -> (f64, f64) {
    if x.is_empty() {
        (NA_REAL, NA_REAL)
    } else {
        (x[(x.len() - 1) / 2], x[x.len() / 2])
    }
}

fn interpolate(a: f64, b: f64, weight: f64) -> f64 {
    if weight == 0.0 || a == b {
        a
    } else if weight == 1.0 {
        b
    } else {
        (1.0 - weight) * a + weight * b
    }
}

fn order_index(n: usize, position: f64) -> usize {
    (position as usize).clamp(1, n) - 1
}

fn quantile_position(p: f64, n: usize, kind: u8, fuzz: f64) -> (usize, usize, f64) {
    let size = n as f64;
    let position = match kind {
        1 | 2 | 4 => size * p,
        3 => size * p - 0.5,
        5 => size * p + 0.5,
        6 => (size + 1.0) * p,
        7 => 1.0 + (size - 1.0) * p,
        8 => {
            let a = 1.0 / 3.0;
            (size + 1.0 - a - a) * p + a
        }
        _ => (size + 0.25) * p + 0.375,
    };
    let j = (position + fuzz).floor();
    let weight = match kind {
        1 => f64::from(position > j),
        2 => {
            if position > j {
                1.0
            } else {
                0.5
            }
        }
        3 => f64::from(position != j || j % 2.0 != 0.0),
        7 => (position - j).max(0.0),
        _ if (position - j).abs() < fuzz => 0.0,
        _ => position - j,
    };
    (order_index(n, j), order_index(n, j + 1.0), weight)
}

fn percent(prob: f64, digits: usize) -> String {
    if prob.is_nan() {
        return String::new();
    }
    if prob == 0.0 {
        return "0%".into();
    }
    let scientific = format!("{:.*e}", digits - 1, 100.0 * prob);
    let Some((significand, exponent)) = scientific.split_once('e') else {
        return format!("{}%", 100.0 * prob);
    };
    let point = exponent.parse::<i32>().unwrap_or_default() + 1;
    let mut text = significand.replace('.', "");
    if point <= 0 {
        text = format!("0.{}{text}", "0".repeat((-point) as usize));
    } else if (point as usize) < text.len() {
        text.insert(point as usize, '.');
    } else {
        text.extend(std::iter::repeat_n('0', point as usize - text.len()));
    }
    if text.contains('.') {
        text.truncate(text.trim_end_matches('0').trim_end_matches('.').len());
    }
    text.push('%');
    text
}

fn checked_scalar(
    ctx: &mut Ctx,
    call: &Call,
    value: SexpId,
    test: impl FnOnce(f64) -> bool,
    error: &str,
) -> EvalResult<f64> {
    let count = ctx.session().xlength(value)?;
    let x = number(ctx, call, value)?;
    if count != 1 || !test(x) {
        return Err(ctx.stop(call.err_call(), error));
    }
    Ok(x)
}

pub(super) fn quantile(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let probs = args.get(1);
    let na_rm = boolean(ctx, call, args.get(2))?;
    let names = boolean(ctx, call, args.get(3))?;
    let kind = checked_scalar(
        ctx,
        call,
        args.get(4),
        |v| (1.0..=9.0).contains(&v) && v.fract() == 0.0,
        "'type' must be an integer in 1..9",
    )? as u8;
    let fuzz = checked_scalar(
        ctx,
        call,
        args.get(6),
        f64::is_finite,
        "is.finite(fuzz) is not TRUE",
    )?;
    if fuzz < 0.0 {
        return Err(ctx.stop(call.err_call(), "fuzz >= 0 is not TRUE"));
    }
    let digits = if names {
        let digits = checked_scalar(
            ctx,
            call,
            args.get(5),
            |x| x >= 1.0,
            "digits >= 1 is not TRUE",
        )?;
        if digits > 50.0 {
            ctx.warn_now(call.err_call(), "'digits' reduced to 50")?;
        }
        digits.clamp(2.0, 50.0) as usize
    } else {
        7
    };
    if probs != SEXP_NIL && !numeric(ctx, probs)? {
        return Err(ctx.stop(call.err_call(), "non-numeric argument to binary operator"));
    }
    let mut probs = numbers(ctx, call, probs)?;
    for p in &mut probs {
        if !p.is_nan() {
            if !(-100.0 * f64::EPSILON..=1.0 + 100.0 * f64::EPSILON).contains(p) {
                return Err(ctx.stop(call.err_call(), "'probs' outside [0,1]"));
            }
            *p = p.clamp(0.0, 1.0);
        }
    }

    let ty = ctx.session().type_of(x)?;
    let factor = has_class(ctx, x, "factor")?;
    let ordered = factor && has_class(ctx, x, "ordered")?;
    if factor && (!ordered || !matches!(kind, 1 | 3)) {
        return Err(ctx.stop(call.err_call(), "(unordered) factors are not allowed"));
    }
    let error = "missing values and NaN's not allowed if 'na.rm' is FALSE";
    ctx.scoped(|ctx| {
        let result = if ty == SexpType::Str {
            let Some(values) = sorted_chars(ctx, call, x, na_rm)? else {
                return Err(ctx.stop(call.err_call(), error));
            };
            if !matches!(kind, 1 | 3) {
                return Err(ctx.stop(call.err_call(), "non-numeric argument to binary operator"));
            }
            limit::<CharId>(ctx, call, probs.len())?;
            let out: Vec<_> = probs
                .iter()
                .map(|&p| {
                    if p.is_nan() || values.is_empty() {
                        CharId::NA
                    } else {
                        let (lo, hi, weight) = quantile_position(p, values.len(), kind, fuzz);
                        values[if weight == 0.0 { lo } else { hi }]
                    }
                })
                .collect();
            ctx.session_mut().alloc_str_from(&out)?
        } else {
            let mut values = numbers(ctx, call, x)?;
            if na_rm {
                values.retain(|v| !v.is_nan());
            } else if values.iter().any(|v| v.is_nan()) {
                return Err(ctx.stop(call.err_call(), error));
            }
            values.sort_unstable_by(f64::total_cmp);
            if has_class(ctx, x, "Date")?
                && (kind == 7
                    || values.is_empty()
                    || probs.iter().filter(|p| !p.is_nan()).any(|&p| {
                        let (lo, hi, weight) = quantile_position(p, values.len(), kind, fuzz);
                        weight > 0.0 && weight < 1.0 && values[lo] != values[hi]
                    }))
            {
                return Err(ctx.stop(call.err_call(), "* not defined for \"Date\" objects"));
            }
            let out: Vec<_> = probs
                .iter()
                .map(|&p| {
                    if p.is_nan() {
                        p
                    } else if values.is_empty() {
                        NA_REAL
                    } else {
                        let (lo, hi, weight) = quantile_position(p, values.len(), kind, fuzz);
                        interpolate(values[lo], values[hi], weight)
                    }
                })
                .collect();
            if ordered {
                let out: Vec<_> = out
                    .iter()
                    .map(|&x| if x.is_nan() { NA_INTEGER } else { x as i32 })
                    .collect();
                let result = ctx.session_mut().alloc_int_from(&out)?;
                ctx.protect(result);
                for name in ["levels", "class"] {
                    let attr = attr(ctx, x, name)?;
                    set_attr(ctx, result, name, attr)?;
                }
                result
            } else {
                let result = ctx.session_mut().alloc_real_from(&out)?;
                copy_class(ctx, x, result)?;
                result
            }
        };
        if names && !probs.is_empty() {
            ctx.protect(result);
            limit::<CharId>(ctx, call, probs.len())?;
            let mut labels = Vec::with_capacity(probs.len());
            for p in probs {
                let label = percent(p, digits);
                labels.push(
                    ctx.session_mut()
                        .mk_char(label.as_bytes(), Encoding::Utf8)?,
                );
            }
            let labels = ctx.session_mut().alloc_str_from(&labels)?;
            set_attr(ctx, result, "names", labels)?;
        }
        Ok(result)
    })
}

pub(super) fn mad(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut deviations = numbers(ctx, call, args.get(0))?;
    let low = boolean(ctx, call, args.get(1))?;
    let high = boolean(ctx, call, args.get(2))?;
    if low && high && deviations.len().is_multiple_of(2) {
        return Err(ctx.stop(call.err_call(), "'low' and 'high' cannot be both TRUE"));
    }
    if deviations.iter().any(|x| x.is_nan()) {
        deviations.clear();
    }
    deviations.sort_unstable_by(f64::total_cmp);
    let (a, b) = middle(&deviations);
    let value = if low || deviations.is_empty() {
        a
    } else if high {
        b
    } else {
        average(&[a, b])
    };
    Ok(ctx.session_mut().scalar_real(value)?)
}

pub(super) fn fivenum(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let na_rm = boolean(ctx, call, args.get(1))?;
    if args.get(0) != SEXP_NIL && !numeric(ctx, args.get(0))? {
        return Err(ctx.stop(call.err_call(), "non-numeric argument to binary operator"));
    }
    let mut x = numbers(ctx, call, args.get(0))?;
    if na_rm {
        x.retain(|v| !v.is_nan());
    } else if x.iter().any(|v| v.is_nan()) {
        x.clear();
    }
    let mut out = [NA_REAL; 5];
    if !x.is_empty() {
        x.sort_unstable_by(f64::total_cmp);
        let n = x.len() as f64;
        let lower_hinge = ((n + 3.0) / 2.0).floor() / 2.0;
        let positions = [1.0, lower_hinge, (n + 1.0) / 2.0, n + 1.0 - lower_hinge, n];
        for (value, i) in out.iter_mut().zip(positions) {
            *value = 0.5 * (x[i.floor() as usize - 1] + x[i.ceil() as usize - 1]);
        }
    }
    Ok(ctx.session_mut().alloc_real_from(&out)?)
}

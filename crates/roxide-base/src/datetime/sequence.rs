use super::*;

enum Step {
    Days(f64),
    Months(i64),
}

pub(super) fn seq_date(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let from = endpoint(ctx, call, args.get(0), "from")?;
    let to = endpoint(ctx, call, args.get(1), "to")?;
    let along = args.get(4);
    let mut length = None;
    if along != SEXP_NIL {
        length = Some(ctx.session().xlength(along)?);
    } else if args.get(3) != SEXP_NIL {
        let value = scalar(ctx, call, args.get(3), "length.out")?.ceil();
        if !value.is_finite() {
            return Err(ctx.stop(call.err_call(), "'length.out' must be a finite number"));
        }
        if value < 0.0 {
            return Err(ctx.stop(
                call.err_call(),
                "'length.out' must be a non-negative number",
            ));
        }
        length = Some(value as usize);
    }
    if let Some(length) = length {
        check_len::<f64>(ctx, call, length)?;
    }

    let by = args.get(2);
    if by == SEXP_NIL {
        let (start, end) = match (from, to, length) {
            (Some(from), Some(to), _) => (from, to),
            (Some(from), None, Some(len)) => (from, from + len.saturating_sub(1) as f64),
            (None, Some(to), Some(len)) => (to - len.saturating_sub(1) as f64, to),
            _ => return Err(ctx.stop(call.err_call(), "without 'by', when one of 'to', 'from' is missing, 'length.out' / 'along.with' must be specified")),
        };
        if let Some(len) = length {
            let step = (end - start) / len.saturating_sub(1).max(1) as f64;
            let mut values = linear(start, step, len);
            if len > 1 {
                values[len - 1] = end;
            }
            return sequence_value(ctx, &values, integer(start) && integer(step));
        }
        let step = if end < start { -1.0 } else { 1.0 };
        return bounded_days(ctx, call, start, end, step);
    }

    if usize::from(from.is_some()) + usize::from(to.is_some()) + usize::from(length.is_some()) != 2
    {
        return Err(ctx.stop(call.err_call(), "given 'by', exactly two of 'to', 'from' and 'length.out' / 'along.with' must be specified"));
    }
    let step = read_step(ctx, call, by)?;
    match (from, to, length, step) {
        (Some(from), Some(to), None, Step::Days(step)) => bounded_days(ctx, call, from, to, step),
        (from, to, Some(len), Step::Days(step)) => {
            if len == 0 {
                return sequence_value(ctx, &[], true);
            }
            let start = from.unwrap_or_else(|| to.unwrap_or_default() - (len - 1) as f64 * step);
            if !step.is_finite() {
                return Err(ctx.stop(call.err_call(), "invalid 'by' argument"));
            }
            let values = linear(start, step, len);
            sequence_value(ctx, &values, integer(start) && integer(step))
        }
        (from, to, len, Step::Months(step)) => months(ctx, call, from, to, len, step),
        _ => Err(ctx.stop(call.err_call(), "invalid date sequence")),
    }
}

fn endpoint(ctx: &mut Ctx, call: &Call, x: SexpId, arg: &str) -> EvalResult<Option<f64>> {
    if x == SEXP_NIL {
        return Ok(None);
    }
    let value = scalar(ctx, call, x, arg)?;
    if !value.is_finite() {
        return Err(ctx.stop(call.err_call(), format!("'{arg}' must be a finite number")));
    }
    Ok(Some(value))
}

fn read_step(ctx: &mut Ctx, call: &Call, by: SexpId) -> EvalResult<Step> {
    match ctx.session().type_of(by)? {
        SexpType::Int | SexpType::Real => return Ok(Step::Days(scalar(ctx, call, by, "by")?)),
        SexpType::Str => {}
        _ => return Err(ctx.stop(call.err_call(), "invalid mode for 'by'")),
    }
    let by = text(ctx, call, by, "by")?;
    if by.starts_with(char::is_whitespace) {
        return Err(ctx.stop(call.err_call(), "invalid 'by' string"));
    }
    let words = by.split_whitespace().collect::<Vec<_>>();
    let (count, unit) = match words.as_slice() {
        [unit] => (1.0, *unit),
        [count, unit] => {
            let Ok(count) = count.parse::<f64>() else {
                return Err(ctx.stop(call.err_call(), "'by' is NA"));
            };
            if !count.is_finite() || count.abs() > f64::from(i32::MAX) {
                return Err(ctx.stop(call.err_call(), "'by' is NA"));
            }
            (count.trunc(), *unit)
        }
        _ => return Err(ctx.stop(call.err_call(), "invalid 'by' string")),
    };
    for (name, magnitude, months) in [
        ("days", 1, false),
        ("weeks", 7, false),
        ("months", 1, true),
        ("quarters", 3, true),
        ("years", 12, true),
    ] {
        if name.starts_with(unit) {
            return Ok(if months {
                Step::Months(count as i64 * magnitude)
            } else {
                Step::Days(count * magnitude as f64)
            });
        }
    }
    Err(ctx.stop(call.err_call(), "invalid string for 'by'"))
}

fn linear(start: f64, step: f64, len: usize) -> Vec<f64> {
    (0..len).map(|i| start + i as f64 * step).collect()
}

fn bounded_days(ctx: &mut Ctx, call: &Call, from: f64, to: f64, step: f64) -> EvalResult {
    if from == to {
        return sequence_value(ctx, &[from], false);
    }
    let steps = (to - from) / step;
    if !steps.is_finite() || !step.is_finite() {
        return Err(ctx.stop(call.err_call(), "invalid '(to - from)/by'"));
    }
    if steps < 0.0 {
        return Err(ctx.stop(call.err_call(), "wrong sign in 'by' argument"));
    }
    let len = ((steps + 1e-10).floor() as usize).saturating_add(1);
    check_len::<f64>(ctx, call, len)?;
    let mut values = linear(from, step, len);
    if let Some(last) = values.last_mut() {
        *last = if step < 0.0 {
            last.max(to)
        } else {
            last.min(to)
        };
    }
    sequence_value(
        ctx,
        &values,
        integer(from) && integer(to) && step.abs() == 1.0,
    )
}

fn months(
    ctx: &mut Ctx,
    call: &Call,
    from: Option<f64>,
    to: Option<f64>,
    len: Option<usize>,
    step: i64,
) -> EvalResult {
    let civil = |days| Civil::from_seconds(days * DAY).map(|c| c.date);
    let Some(anchor) = from.or(to).and_then(civil) else {
        return Err(ctx.stop(call.err_call(), "invalid date"));
    };
    let len = if let (Some(from), Some(to)) = (from, to) {
        if from == to {
            return sequence_value(ctx, &[from.floor()], false);
        }
        if step == 0 {
            return Err(ctx.stop(call.err_call(), "invalid '(to - from)/by'"));
        }
        let Some(end) = civil(to) else {
            return Err(ctx.stop(call.err_call(), "invalid date"));
        };
        let months = (end.year - anchor.year) * 12 + end.month - anchor.month;
        if months != 0 && months.signum() != step.signum() {
            return Err(ctx.stop(call.err_call(), "wrong sign in 'by' argument"));
        }
        (months / step) as usize + 1
    } else {
        len.unwrap_or_default()
    };
    if len == 0 {
        return Err(ctx.stop(
            call.err_call(),
            "zero-length component [[5]] in non-empty \"POSIXlt\" structure",
        ));
    }
    check_len::<f64>(ctx, call, len)?;
    let mut values = Vec::with_capacity(len);
    for i in 0..len {
        let index = if from.is_some() {
            i as i64
        } else {
            i as i64 + 1 - len as i64
        };
        let Some(month) = index
            .checked_mul(step)
            .filter(|month| month.abs() <= 26_000_000_000)
        else {
            return Err(ctx.stop(call.err_call(), "invalid date"));
        };
        let value = anchor.add_months(month).days() as f64;
        let in_range =
            from.is_none() || to.is_none_or(|to| if step < 0 { value >= to } else { value <= to });
        if in_range {
            values.push(value);
        }
    }
    sequence_value(ctx, &values, false)
}

fn integer(value: f64) -> bool {
    value.abs() <= f64::from(i32::MAX) && value.fract() == 0.0
}

fn sequence_value(ctx: &mut Ctx, values: &[f64], use_integer: bool) -> EvalResult {
    if use_integer && values.iter().all(|&value| integer(value)) {
        let ints = values.iter().map(|&value| value as i32).collect::<Vec<_>>();
        let result = ctx.session_mut().alloc_int_from(&ints)?;
        class(ctx, result, DATE)
    } else {
        classed_real(ctx, values, SEXP_NIL, DATE)
    }
}

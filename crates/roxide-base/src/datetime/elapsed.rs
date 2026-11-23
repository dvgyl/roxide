use super::*;

const DIFF: &[&str] = &["difftime"];

pub(super) fn differences(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let lag = scalar(ctx, call, args.get(1), "lag")?;
    let count = scalar(ctx, call, args.get(2), "differences")?;
    if !lag.is_finite() || !count.is_finite() || lag < 1.0 || count < 1.0 {
        return Err(ctx.stop(
            call.err_call(),
            "'lag' and 'differences' must be integers >= 1",
        ));
    }
    let lag = lag as usize;
    let count = count as usize;
    let date = flag(ctx, call, args.get(3), "Date")?;
    let x = args.get(0);
    let mut values = real(ctx, x)?;
    if count > values.len().saturating_sub(1) / lag {
        values.clear();
    } else {
        for _ in 0..count {
            let len = values.len() - lag;
            for i in 0..len {
                values[i] = values[i + lag] - values[i];
            }
            values.truncate(len);
            if ctx.host().poll_interrupt() {
                return Err(ctx.stop(call.err_call(), "interrupted"));
            }
        }
    }
    let names = attr(ctx, x, "names")?;
    let names = if names != SEXP_NIL && !values.is_empty() {
        let ids = ctx.session().str_slice(names)?;
        let ids = ids[ids.len().saturating_sub(values.len())..].to_vec();
        ctx.session_mut().alloc_str_from(&ids)?
    } else {
        SEXP_NIL
    };
    if date {
        elapsed_value(ctx, &values, names, "days")
    } else if ctx.session().type_of(x)? == SexpType::Int {
        let ints = values
            .iter()
            .map(|&v| {
                if v.is_finite() && v.abs() <= f64::from(i32::MAX) {
                    v as i32
                } else {
                    NA_INTEGER
                }
            })
            .collect::<Vec<_>>();
        let result = ctx.session_mut().alloc_int_from(&ints)?;
        inherit_names(ctx, result, names)
    } else {
        let result = ctx.session_mut().alloc_real_from(&values)?;
        inherit_names(ctx, result, names)
    }
}

const UNITS: &[(&str, f64)] = &[
    ("secs", 1.0),
    ("mins", 60.0),
    ("hours", 3600.0),
    ("days", DAY),
    ("weeks", 7.0 * DAY),
];

fn units(ctx: &mut Ctx, call: &Call, value: SexpId, auto: bool) -> EvalResult<(&'static str, f64)> {
    let name = text(ctx, call, value, "units")?;
    if auto && !name.is_empty() && "auto".starts_with(&name) {
        return Ok(("auto", 1.0));
    }
    if let Some(&unit) = UNITS
        .iter()
        .find(|(unit, _)| !name.is_empty() && unit.starts_with(&name))
    {
        return Ok(unit);
    }
    Err(ctx.stop(call.err_call(), "invalid units specified"))
}

fn elapsed_value(ctx: &mut Ctx, values: &[f64], names: SexpId, unit: &str) -> EvalResult {
    let result = classed_real(ctx, values, names, DIFF)?;
    let unit = ctx.session_mut().scalar_str(unit)?;
    set_attr(ctx, result, "units", unit)?;
    Ok(result)
}

fn seconds_elapsed(
    ctx: &mut Ctx,
    mut values: Vec<f64>,
    names: SexpId,
    (mut unit, mut scale): (&str, f64),
) -> EvalResult {
    if unit == "auto" {
        let min = values
            .iter()
            .filter(|v| !v.is_nan())
            .map(|v| v.abs())
            .reduce(f64::min);
        if let Some(min) = min.filter(|min| min.is_finite()) {
            (unit, scale) = UNITS[..4]
                .iter()
                .rev()
                .copied()
                .find(|&(_, scale)| min >= scale)
                .unwrap_or(UNITS[0]);
        } else {
            unit = "secs";
        }
    }
    for value in &mut values {
        *value /= scale;
    }
    elapsed_value(ctx, &values, names, unit)
}

pub(super) fn elapsed(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let tz = text(ctx, call, args.get(2), "tz")?;
    let unit = units(ctx, call, args.get(3), true)?;
    let (a, an) = posix::epoch_values(ctx, call, args.get(0), &tz)?;
    let (b, bn) = posix::epoch_values(ctx, call, args.get(1), &tz)?;
    let values = recycle(&a, &b, |a, b| a - b);
    seconds_elapsed(
        ctx,
        values,
        if a.len() >= b.len() && an != SEXP_NIL {
            an
        } else {
            bn
        },
        unit,
    )
}

pub(super) fn as_elapsed(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    if has_class(ctx, x, "difftime")? {
        return Ok(x);
    }
    let names = attr(ctx, x, "names")?;
    let (unit, scale) = units(ctx, call, args.get(2), true)?;
    if ctx.session().type_of(x)? == SexpType::Str {
        let zone = Zone::fixed("UTC", 0);
        let rows = posix::parse_rows(ctx, call, x, args.get(1), &[], false, &zone)?;
        let day = (ctx.host().now().unix_us as f64 / 1_000_000.0 / DAY).floor();
        let values = rows
            .iter()
            .map(|row| {
                row.civil.map_or(row.second, |civil| {
                    (civil.date.days() as f64 - day) * DAY
                        + (civil.hour * 3600 + civil.minute * 60) as f64
                        + civil.second
                })
            })
            .collect();
        return seconds_elapsed(ctx, values, names, (unit, scale));
    }
    if !matches!(ctx.session().type_of(x)?, SexpType::Int | SexpType::Real) {
        return Err(ctx.stop(call.err_call(), "'tim' is not character or numeric"));
    }
    if unit == "auto" {
        return Err(ctx.stop(
            call.err_call(),
            "need explicit units for numeric conversion",
        ));
    }
    let values = real(ctx, x)?;
    elapsed_value(ctx, &values, names, unit)
}

pub(super) fn set_units(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let (unit, scale) = units(ctx, call, args.get(1), false)?;
    let old = attr(ctx, x, "units")?;
    let (old_unit, old_scale) = units(ctx, call, old, false)?;
    if unit == old_unit {
        return Ok(x);
    }
    let names = attr(ctx, x, "names")?;
    let mut values = real(ctx, x)?;
    for value in &mut values {
        *value *= old_scale / scale;
    }
    let result = ctx.session_mut().alloc_real_from(&values)?;
    inherit_names(ctx, result, names)?;
    let unit = ctx.session_mut().scalar_str(unit)?;
    set_attr(ctx, result, "units", unit)?;
    class(ctx, result, DIFF)
}

pub(super) fn format_elapsed(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let units = attr(ctx, x, "units")?;
    let units = text(ctx, call, units, "units")?;
    let values = real(ctx, x)?;
    let opts = roxide_value::FormatOpts {
        digits: ctx.opts().digits,
        scipen: ctx.opts().scipen,
    };
    let out = roxide_value::format_real_vec(&values, opts)
        .iter()
        .map(|value| format!("{value} {units}"))
        .collect::<Vec<_>>();
    let result = strings(ctx, out.iter().map(|value| Some(value.as_str())))?;
    let names = attr(ctx, x, "names")?;
    inherit_names(ctx, result, names)
}

pub(super) fn print_elapsed(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let digits = scalar(ctx, call, args.get(1), "digits")?.clamp(1.0, 22.0) as usize;
    let units = attr(ctx, x, "units")?;
    let units = text(ctx, call, units, "units")?;
    let len = ctx.session().xlength(x)?;
    let heading = match len {
        0 => "difftime of length 0\n".to_owned(),
        1 => {
            let value = scalar(ctx, call, x, "x")?;
            let opts = roxide_value::FormatOpts {
                digits,
                scipen: ctx.opts().scipen,
            };
            let value = roxide_value::format::format_real_scalar(value, opts);
            format!("Time difference of {value} {units}\n")
        }
        _ => format!("Time differences in {units}\n"),
    };
    let _ = ctx.host().write_out(heading.as_bytes());
    if len > 1 {
        ctx.print_value_default(x, digits)?;
    }
    Ok(x)
}

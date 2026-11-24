use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, FormatOpts, SEXP_NIL, SexpType, ValueApi, format_int_vec, format_lgl_vec,
};

use crate::numbers::{complex_fields, real_fields};
use crate::ops::common::{copy_attrs, error, flag, number, string, vector_limit};

struct Settings {
    number: FormatOpts,
    nsmall: usize,
    width: usize,
    justify: Alignment,
    trim: bool,
    na_encode: bool,
    mark: String,
    interval: usize,
    decimal: String,
    drop_zeroes: bool,
}

#[derive(Clone, Copy)]
enum Alignment {
    Left,
    Right,
    Centre,
    None,
}

pub(crate) fn format(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let digits = if args.get(2) == SEXP_NIL {
        ctx.opts().digits
    } else {
        bounded(ctx, call, args.get(2), "digits", 1, 22)?
    };
    let width = if args.get(5) == SEXP_NIL {
        0
    } else {
        bounded(ctx, call, args.get(5), "width", 0, i32::MAX as usize)?
    };
    let scipen = match ctx.session().type_of(args.get(7))? {
        SexpType::Lgl => match ctx
            .session()
            .lgl(args.get(7))?
            .iter()
            .next()
            .and_then(|v| v.get())
        {
            Some(true) => -i32::MAX,
            Some(false) => i32::MAX,
            None => ctx.opts().scipen,
        },
        _ => {
            let penalty = number(ctx.session_mut(), args.get(7))?;
            if penalty.is_nan() {
                ctx.opts().scipen
            } else {
                penalty as i32
            }
        }
    };
    let justify = match string(ctx.session(), args.get(4)).as_deref() {
        Some("left") => Alignment::Left,
        Some("right") => Alignment::Right,
        Some("centre") => Alignment::Centre,
        Some("none") => Alignment::None,
        _ => return Err(error(ctx, call, "invalid 'justify' argument")),
    };
    let settings = Settings {
        number: FormatOpts { digits, scipen },
        nsmall: bounded(ctx, call, args.get(3), "nsmall", 0, 20)?,
        width,
        justify,
        trim: flag(ctx, args.get(1), false)?,
        na_encode: flag(ctx, args.get(6), true)?,
        mark: string(ctx.session(), args.get(8)).unwrap_or_default(),
        interval: bounded(ctx, call, args.get(9), "big.interval", 1, i32::MAX as usize)?,
        decimal: string(ctx.session(), args.get(10)).unwrap_or_else(|| ".".to_owned()),
        drop_zeroes: flag(ctx, args.get(11), false)?,
    };
    let x = args.get(0);
    let fields = fields(ctx, call, x, &settings, 0)?;
    let s = ctx.session_mut();
    let chars = fields
        .into_iter()
        .map(|field| match field {
            Some(value) => crate::ops::common::ch(s, &value),
            None => Ok(CharId::NA),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let result = s.alloc_str_from(&chars)?;
    copy_attrs(s, x, result, |name| {
        matches!(name, "names" | "dim" | "dimnames")
    })?;
    Ok(result)
}

fn bounded(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    name: &str,
    min: usize,
    max: usize,
) -> EvalResult<usize> {
    let x = number(ctx.session_mut(), x)?;
    if !(min as f64..=max as f64).contains(&x) {
        return Err(error(ctx, call, format!("invalid '{name}' argument")));
    }
    Ok(x as usize)
}

fn fields(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    settings: &Settings,
    depth: usize,
) -> EvalResult<Vec<Option<String>>> {
    let stack_cap = (ctx.stack_budget() / 4096).clamp(128, 2048);
    if depth >= ctx.opts().expressions.min(stack_cap) {
        return Err(error(ctx, call, "evaluation nested too deeply"));
    }
    let s = ctx.session();
    let ty = s.type_of(x)?;
    if ty == SexpType::Vec {
        let values = s.list_slice(x)?.to_vec();
        return values
            .into_iter()
            .map(|value| {
                let values = fields(ctx, call, value, settings, depth + 1)?;
                let joined = values
                    .iter()
                    .map(|field| field.as_deref().unwrap_or("NA").trim())
                    .collect::<Vec<_>>()
                    .join(", ");
                vector_limit(ctx, call, joined.len(), 1)?;
                Ok(Some(joined))
            })
            .collect();
    }
    let mut values: Vec<Option<String>> = match ty {
        SexpType::Lgl => format_lgl_vec(&s.lgl(x)?.to_vec())
            .into_iter()
            .map(Some)
            .collect(),
        SexpType::Int => format_int_vec(&s.int(x)?.to_vec())
            .into_iter()
            .map(Some)
            .collect(),
        SexpType::Real => real_fields(&s.real(x)?.to_vec(), settings.number, settings.nsmall)
            .into_iter()
            .map(Some)
            .collect(),
        SexpType::Cplx => complex_fields(
            &s.cplx(x)?.to_vec(),
            settings.number,
            settings.nsmall,
            settings.trim,
        )
        .into_iter()
        .map(Some)
        .collect(),
        SexpType::Str => s
            .str_slice(x)?
            .iter()
            .map(|&id| {
                if id == CharId::NA {
                    settings.na_encode.then(|| "NA".to_owned())
                } else {
                    let encoded = roxide_value::encode_string(s.char_bytes(id).unwrap_or_default());
                    Some(encoded[1..encoded.len() - 1].replace("\\\"", "\""))
                }
            })
            .collect(),
        SexpType::Env => return Ok(vec![Some(ctx.environment_label(x)?)]),
        SexpType::Sym => return Ok(vec![Some(s.sym_name_of(x)?.to_owned())]),
        SexpType::Nil => return Ok(vec![Some("NULL".to_owned())]),
        SexpType::Lang | SexpType::Expr | SexpType::Clo => {
            return Ok(vec![Some(
                roxide_value::deparse(s, x, Default::default())?.join("\n"),
            )]);
        }
        _ => {
            let (coerced, _) = ctx.session_mut().coerce(x, SexpType::Str)?;
            return fields(ctx, call, coerced, settings, depth + 1);
        }
    };
    if matches!(ty, SexpType::Int | SexpType::Real) {
        for field in values.iter_mut().flatten() {
            *field = decorate(field, settings);
        }
    }
    let (align, pad) = if ty == SexpType::Str {
        (settings.justify, true)
    } else {
        (Alignment::Right, !settings.trim)
    };
    let settings_width = if matches!(align, Alignment::None) {
        0
    } else {
        settings.width
    };
    let width = if pad && !matches!(align, Alignment::None) {
        values
            .iter()
            .flatten()
            .map(|v| v.chars().count())
            .max()
            .unwrap_or(0)
            .max(settings_width)
    } else {
        settings_width
    };
    vector_limit(ctx, call, values.len(), width)?;
    for field in values.iter_mut().flatten() {
        *field = match align {
            Alignment::Left => format!("{field:<width$}"),
            Alignment::Right => format!("{field:>width$}"),
            Alignment::Centre => format!("{field:^width$}"),
            Alignment::None => continue,
        };
    }
    Ok(values)
}

fn decorate(field: &str, settings: &Settings) -> String {
    let (mantissa, exponent) = field.split_once('e').unwrap_or((field, ""));
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let (sign, digits) = whole.strip_prefix('-').map_or(("", whole), |s| ("-", s));
    let mut result = sign.to_owned();
    for (i, ch) in digits.char_indices() {
        if i > 0 && (digits.len() - i).is_multiple_of(settings.interval) && exponent.is_empty() {
            result.push_str(&settings.mark);
        }
        result.push(ch);
    }
    let fraction = if settings.drop_zeroes {
        fraction.trim_end_matches('0')
    } else {
        fraction
    };
    if !fraction.is_empty() {
        result.push_str(&settings.decimal);
        result.push_str(fraction);
    }
    if !exponent.is_empty() {
        result.push('e');
        result.push_str(exponent);
    }
    result
}

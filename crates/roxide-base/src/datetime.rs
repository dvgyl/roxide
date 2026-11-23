#![forbid(unsafe_code)]

//! Calendar dates, date-time conversions, and elapsed times.

mod calendar;
mod elapsed;
mod pattern;
mod posix;
mod sequence;
mod zone;

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind,
    Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, NA_INTEGER, NA_REAL, SEXP_NIL, SexpType, ValueApi, is_na_real,
};

use calendar::{Civil, DAY};
use pattern::Pattern;
use zone::Zone;

const DATE: &[&str] = &["Date"];
const CT: &[&str] = &["POSIXct", "POSIXt"];
const LT: &[&str] = &["POSIXlt", "POSIXt"];
const STANDARD: &str = "character string is not in a standard unambiguous format";

/// Base date-time functions and their S3 methods.
pub const MODULE: Module = Module {
    prims: &[
        internal("rox_date", date, 5),
        internal("rox_ct", posix::clock, 6),
        internal("rox_lt", posix::calendar, 6),
        internal("rox_strptime", posix::strptime, 3),
        internal("rox_datetime_format", datetime_format, 5),
        PrimDef {
            visibility: Visibility::Off,
            ..internal("rox_datetime_print", datetime_print, 2)
        },
        internal("rox_strftime", strftime, 5),
        internal("rox_seq_date", sequence::seq_date, 5),
        internal("rox_diff", elapsed::differences, 4),
        internal("rox_difftime", elapsed::elapsed, 4),
        internal("rox_as_difftime", elapsed::as_elapsed, 3),
        internal("rox_set_units", elapsed::set_units, 2),
        internal("rox_format_difftime", elapsed::format_elapsed, 1),
        PrimDef {
            visibility: Visibility::Off,
            ..internal("rox_print_difftime", elapsed::print_elapsed, 2)
        },
    ],
    prelude: r#"
as.Date <- function(x, ...) UseMethod("as.Date")
as.Date.default <- function(x, ...) .Internal(rox_date(x, NULL, NULL, "UTC", FALSE))
as.Date.Date <- function(x, ...) x
as.Date.integer <- as.Date.numeric <- function(x, origin = NULL, ...)
    .Internal(rox_date(x, NULL, origin, "UTC", FALSE))
as.Date.character <- function(x, format = NULL, tryFormats = c("%Y-%m-%d", "%Y/%m/%d"), optional = FALSE, ...)
    .Internal(rox_date(x, format, tryFormats, "UTC", optional))
as.Date.POSIXct <- function(x, tz = "UTC", ...) .Internal(rox_date(x, NULL, NULL, tz, FALSE))
as.Date.POSIXlt <- function(x, ...) .Internal(rox_date(x, NULL, NULL, "UTC", FALSE))

as.POSIXct <- function(x, tz = "", ...) UseMethod("as.POSIXct")
as.POSIXct.default <- function(x, tz = "", ...) .Internal(rox_ct(x, tz, NULL, NULL, NULL, FALSE))
as.POSIXct.integer <- as.POSIXct.numeric <- function(x, tz = "", origin = NULL, ...)
    .Internal(rox_ct(x, tz, origin, NULL, NULL, FALSE))
as.POSIXct.character <- function(x, tz = "", format = NULL, ..., tryFormats = NULL, optional = FALSE)
    .Internal(rox_ct(x, tz, NULL, format, tryFormats, optional))
as.POSIXct.Date <- function(x, tz = "UTC", ...) .Internal(rox_ct(x, tz, NULL, NULL, NULL, FALSE))
as.POSIXct.POSIXct <- function(x, tz, ...) {
    if (missing(tz)) x else .Internal(rox_ct(x, tz, NULL, NULL, NULL, FALSE))
}
as.POSIXct.POSIXlt <- function(x, tz = "", ...) .Internal(rox_ct(x, tz, NULL, NULL, NULL, FALSE))

as.POSIXlt <- function(x, tz = "", ...) UseMethod("as.POSIXlt")
as.POSIXlt.default <- function(x, tz = "", ...) .Internal(rox_lt(x, tz, NULL, NULL, NULL, FALSE))
as.POSIXlt.integer <- as.POSIXlt.numeric <- function(x, tz = "", origin = NULL, ...)
    .Internal(rox_lt(x, tz, origin, NULL, NULL, FALSE))
as.POSIXlt.character <- function(x, tz = "", format = NULL, ..., tryFormats = NULL, optional = FALSE)
    .Internal(rox_lt(x, tz, NULL, format, tryFormats, optional))
as.POSIXlt.Date <- function(x, tz = "UTC", ...) .Internal(rox_lt(x, tz, NULL, NULL, NULL, FALSE))
as.POSIXlt.POSIXct <- function(x, tz = "", ...) .Internal(rox_lt(x, tz, NULL, NULL, NULL, FALSE))
as.POSIXlt.POSIXlt <- function(x, tz = "", ...) .Internal(rox_lt(x, tz, NULL, NULL, NULL, FALSE))

strptime <- function(x, format, tz = "") .Internal(rox_strptime(x, format, tz))
strftime <- function(x, format = "", tz = "", usetz = FALSE, ...)
    .Internal(rox_strftime(x, format, tz, usetz, getOption("digits.secs", 0L)))
format <- function(x, ...) UseMethod("format")
format.default <- function(x, ...) as.character(x)
format.Date <- function(x, format = "%Y-%m-%d", ...)
    .Internal(rox_datetime_format(x, format, "UTC", FALSE, 0L))
format.POSIXct <- function(x, format = "", tz = "", usetz = FALSE, ...)
    .Internal(rox_datetime_format(x, format, tz, usetz, getOption("digits.secs", 0L)))
format.POSIXlt <- function(x, format = "", usetz = FALSE, ...)
    .Internal(rox_datetime_format(x, format, "", usetz, getOption("digits.secs", 0L)))
as.character.Date <- function(x, ...) format.Date(x, ...)
as.character.POSIXct <- function(x, ...) format.POSIXct(x, ...)
as.character.POSIXlt <- function(x, ...) format.POSIXlt(x, ...)
print.default <- print
print <- function(x, ...) UseMethod("print")
print.Date <- function(x, ...) .Internal(rox_datetime_print(x, format.Date(x)))
print.POSIXct <- function(x, tz = "", usetz = TRUE, ...)
    .Internal(rox_datetime_print(x, format.POSIXct(x, tz = tz, usetz = usetz, ...)))
print.POSIXlt <- function(x, usetz = TRUE, ...)
    .Internal(rox_datetime_print(x, format.POSIXlt(x, usetz = usetz, ...)))

seq <- function(...) UseMethod("seq")
seq.default <- function(...) seq.int(...)
seq.Date <- function(from = NULL, to = NULL, by = NULL, length.out = NULL, along.with = NULL, ...)
    .Internal(rox_seq_date(from, to, by, length.out, along.with))
diff <- function(x, ...) UseMethod("diff")
diff.default <- function(x, lag = 1L, differences = 1L, ...)
    .Internal(rox_diff(x, lag, differences, FALSE))
diff.Date <- function(x, lag = 1L, differences = 1L, ...)
    .Internal(rox_diff(x, lag, differences, TRUE))

difftime <- function(time1, time2, tz = "", units = "auto")
    .Internal(rox_difftime(time1, time2, tz, units))
as.difftime <- function(tim, format = "%X", units = "auto")
    .Internal(rox_as_difftime(tim, format, units))
units <- function(x) UseMethod("units")
units.difftime <- function(x) attr(x, "units")
`units<-` <- function(x, value) UseMethod("units<-")
`units<-.difftime` <- function(x, value) .Internal(rox_set_units(x, value))
format.difftime <- function(x, ...) .Internal(rox_format_difftime(x))
print.difftime <- function(x, digits = getOption("digits"), ...)
    .Internal(rox_print_difftime(x, digits))
"#,
};

const fn internal(name: &'static str, fun: PrimFn, count: usize) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(count),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
    }
}

fn attr(ctx: &mut Ctx, x: SexpId, key: &str) -> EvalResult {
    let sym = ctx.session_mut().intern(key)?;
    Ok(ctx.session().attr(x, sym))
}

fn set_attr(ctx: &mut Ctx, x: SexpId, key: &str, value: SexpId) -> EvalResult<()> {
    let sym = ctx.session_mut().intern(key)?;
    Ok(ctx.session_mut().set_attrib(x, sym, value)?)
}

fn inherit_names(ctx: &mut Ctx, result: SexpId, names: SexpId) -> EvalResult {
    if names != SEXP_NIL && ctx.session().xlength(result)? == ctx.session().xlength(names)? {
        set_attr(ctx, result, "names", names)?;
    }
    Ok(result)
}

fn text_values(ctx: &Ctx, x: SexpId) -> EvalResult<Vec<Option<String>>> {
    if x == SEXP_NIL {
        return Ok(Vec::new());
    }
    ctx.session()
        .str_slice(x)?
        .iter()
        .map(|&id| {
            if id == CharId::NA {
                return Ok(None);
            }
            Ok(Some(
                String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned(),
            ))
        })
        .collect()
}

fn strings<'a>(ctx: &mut Ctx, values: impl IntoIterator<Item = Option<&'a str>>) -> EvalResult {
    let values = values
        .into_iter()
        .map(|value| {
            value.map_or(Ok(CharId::NA), |s| {
                ctx.session_mut().mk_char(s.as_bytes(), Encoding::Native)
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ctx.session_mut().alloc_str_from(&values)?)
}

fn text(ctx: &mut Ctx, call: &Call, x: SexpId, arg: &str) -> EvalResult<String> {
    if let Ok([id]) = ctx.session().str_slice(x)
        && *id != CharId::NA
    {
        return Ok(String::from_utf8_lossy(ctx.session().char_bytes(*id)?).into_owned());
    }
    Err(ctx.stop(call.err_call(), format!("invalid '{arg}' argument")))
}

fn has_class(ctx: &mut Ctx, x: SexpId, class: &str) -> EvalResult<bool> {
    let classes = attr(ctx, x, "class")?;
    Ok(text_values(ctx, classes)?
        .iter()
        .any(|c| c.as_deref() == Some(class)))
}

fn class(ctx: &mut Ctx, x: SexpId, classes: &[&str]) -> EvalResult {
    let class = strings(ctx, classes.iter().copied().map(Some))?;
    set_attr(ctx, x, "class", class)?;
    Ok(x)
}

fn real(ctx: &mut Ctx, x: SexpId) -> EvalResult<Vec<f64>> {
    if x == SEXP_NIL {
        return Ok(Vec::new());
    }
    let (numeric, _) = ctx.session_mut().coerce(x, SexpType::Real)?;
    Ok(ctx.session().real(numeric)?.to_vec())
}

fn scalar(ctx: &mut Ctx, call: &Call, x: SexpId, arg: &str) -> EvalResult<f64> {
    if let [value] = real(ctx, x)?.as_slice() {
        return Ok(*value);
    }
    Err(ctx.stop(call.err_call(), format!("invalid '{arg}' argument")))
}

fn flag(ctx: &mut Ctx, call: &Call, x: SexpId, arg: &str) -> EvalResult<bool> {
    let value = scalar(ctx, call, x, arg)?;
    if value.is_nan() {
        return Err(ctx.stop(call.err_call(), format!("invalid '{arg}' argument")));
    }
    Ok(value != 0.0)
}

fn check_len<T>(ctx: &mut Ctx, call: &Call, len: usize) -> EvalResult<()> {
    if len > ctx.host().limits().max_vector_bytes / std::mem::size_of::<T>() {
        return Err(ctx.stop(
            call.err_call(),
            "vector allocation exceeds the session limit",
        ));
    }
    Ok(())
}

fn classed_real(ctx: &mut Ctx, values: &[f64], names: SexpId, classes: &[&str]) -> EvalResult {
    let result = ctx.session_mut().alloc_real_from(values)?;
    inherit_names(ctx, result, names)?;
    class(ctx, result, classes)
}

fn date(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    to_date(
        ctx,
        call,
        args.get(0),
        args.get(1),
        args.get(2),
        args.get(3),
        args.get(4),
    )
}

fn to_date(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    format: SexpId,
    origin: SexpId,
    tz: SexpId,
    optional: SexpId,
) -> EvalResult {
    let names = attr(ctx, x, "names")?;
    let values = if has_class(ctx, x, "Date")? {
        return Ok(x);
    } else if has_class(ctx, x, "POSIXlt")? {
        let data = posix::read_list(ctx, call, x)?;
        let values = data
            .rows
            .iter()
            .map(|row| {
                row.civil
                    .map_or(row.second, |civil| civil.date.days() as f64)
            })
            .collect::<Vec<_>>();
        return classed_real(ctx, &values, data.names, DATE);
    } else if has_class(ctx, x, "POSIXct")? {
        let tz = text(ctx, call, tz, "tz")?;
        let zone = Zone::load(ctx.host(), &tz);
        real(ctx, x)?
            .into_iter()
            .map(|value| {
                if !value.is_finite() {
                    return value;
                }
                ((value + f64::from(zone.at(value).seconds)) / DAY).floor()
            })
            .collect()
    } else if ctx.session().type_of(x)? == SexpType::Str {
        let zone = Zone::fixed("UTC", 0);
        let optional = optional != SEXP_NIL && flag(ctx, call, optional, "optional")?;
        let patterns = if origin == SEXP_NIL {
            vec![Some("%F".into()), Some("%Y/%m/%d".into())]
        } else {
            text_values(ctx, origin)?
        };
        posix::parse_rows(ctx, call, x, format, &patterns, optional, &zone)?
            .iter()
            .map(|row| {
                row.civil
                    .map_or(row.second, |civil| civil.date.days() as f64)
            })
            .collect()
    } else {
        let mut values = real(ctx, x)?;
        if matches!(ctx.session().type_of(x)?, SexpType::Int | SexpType::Real) {
            if origin != SEXP_NIL {
                let base = if has_class(ctx, origin, "Date")? {
                    origin
                } else {
                    to_date(ctx, call, origin, SEXP_NIL, SEXP_NIL, tz, SEXP_NIL)?
                };
                let base = real(ctx, base)?;
                values = recycle(&values, &base, |a, b| a + b);
                check_len::<f64>(ctx, call, values.len())?;
            }
        } else if !values.iter().all(|v| is_na_real(*v)) {
            return Err(ctx.stop(
                call.err_call(),
                "do not know how to convert 'x' to class “Date”",
            ));
        }
        values
    };
    classed_real(ctx, &values, names, DATE)
}

fn datetime_format(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let tz = text(ctx, call, args.get(2), "tz")?;
    let date = has_class(ctx, x, "Date")?;
    let data = posix::format_data(ctx, call, x, &tz)?;
    format_rows(ctx, call, &data, args, date)
}

fn strftime(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let tz = text(ctx, call, args.get(2), "tz")?;
    let data = if ctx.session().type_of(x)? == SexpType::Str {
        let zone = Zone::load(ctx.host(), &tz);
        let rows = posix::parse_rows(
            ctx,
            call,
            x,
            SEXP_NIL,
            &posix::default_patterns(),
            false,
            &zone,
        )?;
        posix::Data {
            rows,
            names: attr(ctx, x, "names")?,
            zone,
        }
    } else {
        posix::format_data(ctx, call, x, &tz)?
    };
    format_rows(ctx, call, &data, args, false)
}

fn format_rows(
    ctx: &mut Ctx,
    call: &Call,
    data: &posix::Data,
    args: &Args,
    date: bool,
) -> EvalResult {
    let use_tz = flag(ctx, call, args.get(3), "usetz")?;
    let digits = scalar(ctx, call, args.get(4), "digits.secs")?.clamp(0.0, 6.0) as usize;
    let input = text_values(ctx, args.get(1))?;
    if input.is_empty() {
        return Err(ctx.stop(call.err_call(), "invalid 'format' argument"));
    }
    let midnight = data.rows.iter().all(|row| {
        row.civil
            .is_none_or(|civil| civil.hour == 0 && civil.minute == 0 && civil.second == 0.0)
    });
    let patterns = input
        .iter()
        .map(|format| {
            format.as_deref().map(|format| {
                Pattern::new(if !format.is_empty() {
                    format
                } else if date || midnight {
                    "%F"
                } else if digits == 0 {
                    "%F %T"
                } else {
                    "%F %H:%M:%OS"
                })
            })
        })
        .collect::<Vec<_>>();
    let len = if data.rows.is_empty() {
        0
    } else {
        data.rows.len().max(patterns.len())
    };
    check_len::<CharId>(ctx, call, len)?;
    let mut out = Vec::with_capacity(len);
    let mut bytes = len * size_of::<CharId>();
    for i in 0..len {
        let row = &data.rows[i % data.rows.len()];
        let value = match (row.civil, &patterns[i % patterns.len()]) {
            (_, None) => None,
            (Some(civil), Some(pattern)) => {
                let offset = row
                    .offset
                    .as_ref()
                    .or_else(|| data.zone.resolve(civil, row.dst).map(|(_, offset)| offset));
                let mut formatted = pattern.format(civil, offset, digits);
                let name = if row.label.as_deref().is_some_and(|name| !name.is_empty()) {
                    row.label.as_deref()
                } else if row.label.is_none() {
                    offset.map(|offset| offset.name.as_str())
                } else {
                    None
                };
                if let Some(name) = name.filter(|name| use_tz && !name.is_empty()) {
                    formatted.push(' ');
                    formatted.push_str(name);
                }
                Some(formatted)
            }
            (None, _) if !is_na_real(row.second) && !row.second.is_finite() => Some(
                if row.second.is_nan() {
                    "NaN"
                } else if row.second.is_sign_negative() {
                    "-Inf"
                } else {
                    "Inf"
                }
                .into(),
            ),
            _ => None,
        };
        bytes = bytes.saturating_add(value.as_ref().map_or(0, String::len));
        check_len::<u8>(ctx, call, bytes)?;
        out.push(value);
    }
    let out = strings(ctx, out.iter().map(Option::as_deref))?;
    inherit_names(ctx, out, data.names)
}

fn datetime_print(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    ctx.print_value_default(args.get(1), ctx.opts().digits)?;
    Ok(args.get(0))
}

fn recycle(a: &[f64], b: &[f64], f: impl Fn(f64, f64) -> f64) -> Vec<f64> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    (0..a.len().max(b.len()))
        .map(|i| f(a[i % a.len()], b[i % b.len()]))
        .collect()
}

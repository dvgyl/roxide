use super::calendar::Date;
use super::zone::Offset;
use super::*;

const FIELDS: [&str; 11] = [
    "sec", "min", "hour", "mday", "mon", "year", "wday", "yday", "isdst", "zone", "gmtoff",
];

pub(super) struct Row {
    pub civil: Option<Civil>,
    pub second: f64,
    pub dst: Option<bool>,
    pub label: Option<String>,
    pub offset: Option<Offset>,
}

impl Row {
    fn missing(second: f64, zone: &Zone) -> Self {
        Self {
            civil: None,
            second,
            dst: None,
            label: Some(if zone.utc {
                zone.at(0.0).name.clone()
            } else {
                String::new()
            }),
            offset: zone.utc.then(|| zone.at(0.0).clone()),
        }
    }

    fn from_epoch(epoch: f64, zone: &Zone) -> Self {
        let offset = zone.at(epoch);
        let Some(mut civil) = Civil::from_seconds(epoch.floor() + f64::from(offset.seconds)) else {
            return Self::missing(if epoch.is_finite() { NA_REAL } else { epoch }, zone);
        };
        civil.second += epoch - epoch.floor();
        Self {
            civil: Some(civil),
            second: civil.second,
            dst: Some(offset.dst),
            label: Some(offset.name.clone()),
            offset: Some(offset.clone()),
        }
    }

    fn epoch(&self, zone: &Zone) -> f64 {
        if let Some(civil) = self.civil {
            zone.resolve(civil, self.dst)
                .map_or(NA_REAL, |(epoch, _)| epoch)
        } else {
            self.second
        }
    }
}

pub(super) struct Data {
    pub rows: Vec<Row>,
    pub names: SexpId,
    pub zone: Zone,
}

pub(super) fn default_patterns() -> Vec<Option<String>> {
    [
        "%Y-%m-%d %H:%M:%OS",
        "%Y/%m/%d %H:%M:%OS",
        "%Y-%m-%d %H:%M",
        "%Y/%m/%d %H:%M",
        "%F",
        "%Y/%m/%d",
    ]
    .into_iter()
    .map(|s| Some(s.to_owned()))
    .collect()
}

pub(super) fn parse_rows(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    format: SexpId,
    defaults: &[Option<String>],
    optional: bool,
    zone: &Zone,
) -> EvalResult<Vec<Row>> {
    let input = text_values(ctx, x)?;
    let now = ctx.host().now().unix_us as f64 / 1_000_000.0;
    let today = Date::from_days(((now + f64::from(zone.at(now).seconds)) / DAY).floor() as i64);
    let formats = if format != SEXP_NIL {
        text_values(ctx, format)?
    } else if let Some(first) = input.iter().flatten().next() {
        let mut found = None;
        for format in defaults.iter().flatten() {
            let pattern = Pattern::new(format);
            if pattern.has_zone_name() {
                return Err(ctx.stop(call.err_call(), "use of %Z for input is not supported"));
            }
            if pattern.parse(first, today).is_some() {
                found = Some(format.clone());
                break;
            }
        }
        if found.is_none() && !optional {
            return Err(ctx.stop(call.err_call(), STANDARD));
        }
        vec![found]
    } else {
        vec![None]
    };
    if formats.is_empty() {
        return Err(ctx.stop(call.err_call(), "invalid 'format' argument"));
    }
    let patterns = formats
        .iter()
        .map(|s| s.as_deref().map(Pattern::new))
        .collect::<Vec<_>>();
    if patterns.iter().flatten().any(Pattern::has_zone_name) {
        return Err(ctx.stop(call.err_call(), "use of %Z for input is not supported"));
    }
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let len = input.len().max(patterns.len());
    check_len::<Row>(ctx, call, len)?;
    Ok((0..len)
        .map(|i| {
            let parsed = input[i % input.len()]
                .as_deref()
                .zip(patterns[i % patterns.len()].as_ref())
                .and_then(|(input, pattern)| pattern.parse(input, today));
            let Some((civil, explicit_offset)) = parsed else {
                return Row::missing(NA_REAL, zone);
            };
            if let Some(offset) = explicit_offset {
                let mut row = Row::from_epoch(civil.seconds() - f64::from(offset), zone);
                if !zone.utc {
                    row.label = Some(String::new());
                    row.offset = None;
                }
                return row;
            }
            let offset = zone.resolve(civil, None).map(|(_, offset)| offset);
            Row {
                civil: Some(civil),
                second: civil.second,
                dst: offset.map(|offset| offset.dst),
                label: Some(offset.map_or_else(String::new, |offset| offset.name.clone())),
                offset: offset.filter(|_| zone.utc).cloned(),
            }
        })
        .collect())
}

fn to_list(ctx: &mut Ctx, data: Data) -> EvalResult {
    let list = ctx.session_mut().alloc_list(FIELDS.len())?;
    let columns = strings(ctx, FIELDS.into_iter().map(Some))?;
    set_attr(ctx, list, "names", columns)?;
    let sec = data.rows.iter().map(|row| row.second).collect::<Vec<_>>();
    let sec = ctx.session_mut().alloc_real_from(&sec)?;
    ctx.session_mut().list_set(list, 0, sec)?;
    for i in 1..9 {
        let values = data
            .rows
            .iter()
            .map(|row| {
                if i == 8 {
                    return row.dst.map_or(-1, i32::from);
                }
                row.civil.map_or(NA_INTEGER, |civil| match i {
                    1 => civil.minute as i32,
                    2 => civil.hour as i32,
                    3 => civil.date.day as i32,
                    4 => civil.date.month as i32 - 1,
                    5 => (civil.date.year - 1900) as i32,
                    6 => civil.date.weekday() as i32,
                    _ => civil.date.ordinal() as i32,
                })
            })
            .collect::<Vec<_>>();
        let field = ctx.session_mut().alloc_int_from(&values)?;
        if i == 5 {
            inherit_names(ctx, field, data.names)?;
        }
        ctx.session_mut().list_set(list, i, field)?;
    }
    let labels = strings(ctx, data.rows.iter().map(|row| row.label.as_deref()))?;
    ctx.session_mut().list_set(list, 9, labels)?;
    let offsets = data
        .rows
        .iter()
        .map(|row| {
            row.offset
                .as_ref()
                .map_or(NA_INTEGER, |offset| offset.seconds)
        })
        .collect::<Vec<_>>();
    let offsets = ctx.session_mut().alloc_int_from(&offsets)?;
    ctx.session_mut().list_set(list, 10, offsets)?;
    class(ctx, list, LT)?;
    let tz = strings(ctx, data.zone.attributes().into_iter().map(Some))?;
    set_attr(ctx, list, "tzone", tz)?;
    let balanced = ctx.session_mut().scalar_lgl(roxide_value::RLogical::TRUE)?;
    set_attr(ctx, list, "balanced", balanced)?;
    Ok(list)
}

fn tzone(ctx: &mut Ctx, x: SexpId) -> EvalResult<String> {
    let value = attr(ctx, x, "tzone")?;
    Ok(text_values(ctx, value)?
        .into_iter()
        .next()
        .flatten()
        .unwrap_or_default())
}

pub(super) fn read_list(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Data> {
    let zone_name = tzone(ctx, x)?;
    let zone = Zone::load(ctx.host(), &zone_name);
    let list = ctx.session().list_slice(x)?.to_vec();
    let field = |i| list.get(i).copied().unwrap_or(SEXP_NIL);
    let names = attr(ctx, field(5), "names")?;
    let values = (0..9)
        .map(|i| real(ctx, field(i)))
        .collect::<EvalResult<Vec<_>>>()?;
    let labels = text_values(ctx, field(9))?;
    let offsets = real(ctx, field(10))?;
    let len = values
        .iter()
        .map(Vec::len)
        .chain([labels.len(), offsets.len()])
        .max()
        .unwrap_or_default();
    check_len::<Row>(ctx, call, len)?;
    let rows = (0..len)
        .map(|i| {
            let element = |col: &[f64]| col.get(i % col.len().max(1)).copied().unwrap_or(NA_REAL);
            let sec = element(&values[0]);
            let dst = match element(&values[8]) {
                0.0 => Some(false),
                1.0 => Some(true),
                _ => None,
            };
            let label = labels.get(i % labels.len().max(1)).cloned().flatten();
            let offset = element(&offsets);
            let offset = offset.is_finite().then(|| Offset {
                seconds: offset as i32,
                dst: dst.unwrap_or(false),
                name: label.clone().unwrap_or_default(),
            });
            let components = [1, 2, 3, 4, 5].map(|index| element(&values[index]));
            let civil = if sec.is_finite()
                && components
                    .iter()
                    .all(|&v| v.is_finite() && v.abs() <= f64::from(i32::MAX))
            {
                let [minute, hour, mday, mon, year] = components.map(|v| v as i64);
                let date = Date {
                    year: 1900 + year,
                    month: 1,
                    day: mday,
                }
                .add_months(mon);
                let civil = Civil {
                    date,
                    hour,
                    minute,
                    second: sec,
                };
                if (0..24).contains(&hour)
                    && (0..60).contains(&minute)
                    && (0.0..62.0).contains(&sec)
                {
                    let date = Date::from_days(civil.date.days());
                    Some(Civil { date, ..civil })
                } else {
                    Civil::from_seconds(civil.seconds())
                }
            } else {
                None
            };
            let second = if civil.is_none() && sec.is_finite() {
                NA_REAL
            } else {
                sec
            };
            Row {
                civil,
                second,
                dst,
                label,
                offset,
            }
        })
        .collect();
    Ok(Data { rows, names, zone })
}

fn from_values(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    values: &[f64],
    zone: Zone,
) -> EvalResult<Data> {
    check_len::<Row>(ctx, call, values.len())?;
    let rows = values
        .iter()
        .map(|&epoch| Row::from_epoch(epoch, &zone))
        .collect();
    let names = attr(ctx, x, "names")?;
    Ok(Data { rows, names, zone })
}

pub(super) fn format_data(ctx: &mut Ctx, call: &Call, x: SexpId, tz: &str) -> EvalResult<Data> {
    if has_class(ctx, x, "POSIXlt")? {
        return read_list(ctx, call, x);
    }
    let date = has_class(ctx, x, "Date")?;
    let values = real(ctx, x)?;
    if date {
        let mut zone = Zone::fixed(if tz.is_empty() { "UTC" } else { tz }, 0);
        zone.name = tz.to_owned();
        let seconds = values.iter().map(|d| d.floor() * DAY).collect::<Vec<_>>();
        return from_values(ctx, call, x, &seconds, zone);
    }
    let tz = if tz.is_empty() {
        tzone(ctx, x)?
    } else {
        tz.to_owned()
    };
    let tz = effective_tz(ctx, &tz);
    let zone = Zone::load(ctx.host(), &tz);
    from_values(ctx, call, x, &values, zone)
}

pub(super) fn strptime(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = args.get(0);
    let tz = text(ctx, call, args.get(2), "tz")?;
    let zone = Zone::load(ctx.host(), &tz);
    let names = attr(ctx, input, "names")?;
    let (input, _) = ctx.session_mut().coerce(input, SexpType::Str)?;
    let rows = parse_rows(ctx, call, input, args.get(1), &[], false, &zone)?;
    to_list(ctx, Data { rows, names, zone })
}

fn character_data(ctx: &mut Ctx, call: &Call, args: &Args, zone: Zone) -> EvalResult<Data> {
    let x = args.get(0);
    let formats = if args.get(4) == SEXP_NIL {
        default_patterns()
    } else {
        text_values(ctx, args.get(4))?
    };
    let optional = flag(ctx, call, args.get(5), "optional")?;
    let rows = parse_rows(ctx, call, x, args.get(3), &formats, optional, &zone)?;
    Ok(Data {
        rows,
        names: attr(ctx, x, "names")?,
        zone,
    })
}

fn numeric_epoch(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    origin: SexpId,
    zone: &Zone,
) -> EvalResult<Vec<f64>> {
    let mut values = real(ctx, x)?;
    if origin == SEXP_NIL {
        if matches!(ctx.session().type_of(x)?, SexpType::Int | SexpType::Real)
            || values.iter().all(|v| is_na_real(*v))
        {
            return Ok(values);
        }
        return Err(ctx.stop(
            call.err_call(),
            "do not know how to convert 'x' to class “POSIXct”",
        ));
    }
    let (base, _) = epoch_values(ctx, call, origin, &zone.name)?;
    values = recycle(&values, &base, |a, b| a + b);
    Ok(values)
}

fn make_clock(ctx: &mut Ctx, values: &[f64], names: SexpId, tz: &str) -> EvalResult {
    let x = ctx.session_mut().alloc_real_from(values)?;
    inherit_names(ctx, x, names)?;
    class(ctx, x, CT)?;
    let zone = ctx.session_mut().scalar_str(tz)?;
    set_attr(ctx, x, "tzone", zone)?;
    Ok(x)
}

pub(super) fn epoch_values(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    tz: &str,
) -> EvalResult<(Vec<f64>, SexpId)> {
    let names = attr(ctx, x, "names")?;
    let values = if has_class(ctx, x, "POSIXlt")? {
        let data = read_list(ctx, call, x)?;
        let zone = if tz.is_empty() {
            data.zone
        } else {
            Zone::load(ctx.host(), tz)
        };
        return Ok((
            data.rows.iter().map(|row| row.epoch(&zone)).collect(),
            data.names,
        ));
    } else if has_class(ctx, x, "Date")? {
        real(ctx, x)?.into_iter().map(|d| d * DAY).collect()
    } else if ctx.session().type_of(x)? == SexpType::Str {
        let zone = Zone::load(ctx.host(), tz);
        parse_rows(ctx, call, x, SEXP_NIL, &default_patterns(), false, &zone)?
            .iter()
            .map(|row| row.epoch(&zone))
            .collect()
    } else {
        numeric_epoch(ctx, call, x, SEXP_NIL, &Zone::fixed(tz, 0))?
    };
    Ok((values, names))
}

pub(super) fn clock(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let tz = text(ctx, call, args.get(1), "tz")?;
    let zone = Zone::load(ctx.host(), &tz);
    if ctx.session().type_of(x)? == SexpType::Str {
        let data = character_data(ctx, call, args, zone)?;
        let values = data
            .rows
            .iter()
            .map(|row| row.epoch(&data.zone))
            .collect::<Vec<_>>();
        return make_clock(ctx, &values, data.names, &tz);
    }
    if has_class(ctx, x, "POSIXct")? {
        let result = ctx.session_mut().shallow_duplicate(x)?;
        let tz = ctx.session_mut().scalar_str(&tz)?;
        set_attr(ctx, result, "tzone", tz)?;
        return Ok(result);
    }
    let date = has_class(ctx, x, "Date")?;
    let lt = has_class(ctx, x, "POSIXlt")?;
    if date || lt {
        let (values, names) = epoch_values(ctx, call, x, &tz)?;
        let tz = if lt && tz.is_empty() {
            tzone(ctx, x)?
        } else {
            tz
        };
        return make_clock(ctx, &values, names, &tz);
    }
    let values = numeric_epoch(ctx, call, x, args.get(2), &zone)?;
    let names = attr(ctx, x, "names")?;
    make_clock(ctx, &values, names, &tz)
}

pub(super) fn calendar(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let tz = text(ctx, call, args.get(1), "tz")?;
    if has_class(ctx, x, "POSIXlt")? {
        if tz.is_empty() {
            return Ok(x);
        }
        let result = ctx.session_mut().shallow_duplicate(x)?;
        let tz = ctx.session_mut().scalar_str(&tz)?;
        set_attr(ctx, result, "tzone", tz)?;
        return Ok(result);
    }
    if ctx.session().type_of(x)? == SexpType::Str {
        let zone = Zone::load(ctx.host(), &tz);
        let data = character_data(ctx, call, args, zone)?;
        return to_list(ctx, data);
    }
    if has_class(ctx, x, "Date")? || has_class(ctx, x, "POSIXct")? {
        let data = format_data(ctx, call, x, &tz)?;
        return to_list(ctx, data);
    }
    let effective = effective_tz(ctx, &tz);
    let zone = Zone::load(ctx.host(), &effective);
    let values = numeric_epoch(ctx, call, x, args.get(2), &zone)?;
    let data = from_values(ctx, call, x, &values, zone)?;
    to_list(ctx, data)
}

fn effective_tz(ctx: &mut Ctx, tz: &str) -> String {
    if tz.is_empty() {
        ctx.host()
            .env_var("TZ")
            .and_then(|s| String::from_utf8(s).ok())
            .unwrap_or_default()
    } else {
        tz.to_owned()
    }
}

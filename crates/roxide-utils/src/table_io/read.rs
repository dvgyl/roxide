use std::collections::HashSet;

use roxide_base::scan::{
    Fields, InputOptions, RowOptions, base_call, dialect, r_strings, read_columns, scan_column,
    strips,
};
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, FALSE_SCALAR, NA_INTEGER, SEXP_NIL, SexpType, TRUE_SCALAR, ValueApi};

use super::convert::Conversion;
use super::{attr, flag, has_class, limit, set, string};

pub(super) fn count_fields(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let dialect = dialect(
        ctx,
        call,
        [
            args.get(1),
            args.get(2),
            args.get(5),
            FALSE_SCALAR,
            args.get(4),
        ],
    )?;
    let skip = limit(ctx, args.get(3))?;
    let mut lexer = Fields::open(
        ctx,
        call,
        args.get(0),
        SEXP_NIL,
        InputOptions {
            skip,
            encoding: "unknown",
            skip_nul: false,
        },
        &dialect,
    )?;
    let max = ctx.host().limits().max_vector_bytes / size_of::<i32>();
    let mut counts = Vec::new();
    while let Some(record) = lexer.record(ctx, call, usize::MAX, |_| (true, false))? {
        let span = record.end_line - record.start_line;
        if span.saturating_add(counts.len()) >= max {
            return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
        }
        counts.resize(counts.len() + span, NA_INTEGER);
        if record.unclosed_quote {
            ctx.warn_now(call.err_call(), "EOF within quoted string")?;
        } else {
            counts.push(record.fields.len() as i32);
        }
    }
    if counts.is_empty() {
        Ok(SEXP_NIL)
    } else {
        Ok(ctx.session_mut().alloc_int_from(&counts)?)
    }
}

pub(super) fn read_table(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let dialect = dialect(
        ctx,
        call,
        [
            args.get(3),
            args.get(4),
            args.get(20),
            args.get(21),
            args.get(19),
        ],
    )?;
    let conversion = Conversion::new(
        ctx,
        call,
        args.get(12),
        args.get(5),
        args.get(6),
        args.get(11),
    )?;
    let nrows = limit(ctx, args.get(14))?;
    let skip = limit(ctx, args.get(15))?;
    let encoding = string(ctx, call, args.get(23), "encoding")?;
    let skip_nul = flag(ctx, args.get(25))?.unwrap_or(false);
    if args.get(24) == SEXP_NIL
        && ctx.session().type_of(args.get(0))? == SexpType::Str
        && string(ctx, call, args.get(0), "file")?.is_empty()
    {
        return Err(ctx.stop(
            call.err_call(),
            "'file' must be a character string or connection",
        ));
    }
    let mut preview = Fields::open(
        ctx,
        call,
        args.get(0),
        args.get(24),
        InputOptions {
            skip,
            encoding: &encoding,
            skip_nul,
        },
        &dialect,
    )?;
    let mut header = flag(ctx, args.get(1))?.unwrap_or(false);
    let auto_header = flag(ctx, args.get(2))?.unwrap_or(false);
    let lookahead = if nrows == 0 {
        5
    } else {
        5.min(nrows.saturating_add(usize::from(header)))
    };
    let start = preview.start_preview();
    let first = preview.record(ctx, call, usize::MAX, |_| (true, true))?;
    let has_input = first.is_some();
    let supplied = args.get(9) != SEXP_NIL;
    if !has_input && !supplied {
        return Err(ctx.stop(call.err_call(), "no lines available in input"));
    }
    let mut first_names = Vec::new();
    let mut width = 0;
    let mut after_header = None;
    if let Some(first) = first {
        check_header_quote(ctx, call, args, first.unclosed_quote)?;
        first_names = first.fields.into_iter().map(Some).collect();
        if first_names.is_empty() && dialect.separator.is_some() {
            first_names.push(Some(String::new()));
        }
        width = first_names.len();
        after_header = Some(preview.position());
        for _ in 1..lookahead {
            let Some(record) = preview.record(ctx, call, usize::MAX, |_| (true, false))? else {
                break;
            };
            check_header_quote(ctx, call, args, record.unclosed_quote)?;
            width = width.max(record.fields.len());
        }
    }
    let mut names = if supplied {
        r_strings(ctx, args.get(9))?
    } else {
        first_names.clone()
    };
    let inferred_rows = (header || auto_header) && width == names.len().saturating_add(1);
    header |= auto_header && inferred_rows;
    if !has_input {
        width = names.len();
    }
    if header && supplied && has_input && names.len() != first_names.len() {
        ctx.warn_now(
            call.err_call(),
            "header and 'col.names' are of different lengths",
        )?;
    }
    if !header && !supplied {
        names = (1..=width).map(|i| Some(format!("V{i}"))).collect();
    }
    if width > names.len() + usize::from(inferred_rows) {
        return Err(ctx.stop(call.err_call(), "more columns than column names"));
    }
    width = names.len() + usize::from(inferred_rows);
    let check_names = flag(ctx, args.get(16))?.unwrap_or(false);
    if check_names {
        let x = make_strings(ctx, &names)?;
        let repaired = base_call(ctx, call, "make.names", &[x, TRUE_SCALAR])?;
        names = r_strings(ctx, repaired)?;
    }
    if inferred_rows {
        names.insert(0, Some("row.names".into()));
    }

    let classes = column_classes(ctx, call, args.get(13), &names)?;
    let types: Vec<_> = classes
        .iter()
        .map(|class| scan_type(class.as_deref()))
        .collect();
    if ctx.session().xlength(args.get(18))? > 1 {
        return Err(ctx.stop(call.err_call(), "invalid 'strip.white' length"));
    }
    let strip = strips(ctx, args.get(18), width)?;
    let as_is = AsIs::from_value(ctx, call, args.get(10), &names)?;
    preview.end_preview(if header {
        after_header.unwrap_or(start)
    } else {
        start
    });
    let options = RowOptions {
        max_rows: nrows,
        nlines: 0,
        flush: flag(ctx, args.get(22))?.unwrap_or(false),
        fill: flag(ctx, args.get(17))?.unwrap_or(false),
        multi_line: false,
        flush_at_limit: true,
    };
    let rows = read_columns(ctx, call, &mut preview, &types, &strip, options);
    preview.finish(ctx);
    let rows = rows?;
    let n = rows.first().map_or(0, Vec::len);
    let types_first_is_dropped = types.first() == Some(&SexpType::Nil);
    ctx.scoped(|ctx| {
        let mut columns = Vec::new();
        for (i, ((fields, class), ty)) in rows.into_iter().zip(classes).zip(types).enumerate() {
            if ty == SexpType::Nil {
                continue;
            }
            let value = scan_column(
                ctx,
                call,
                fields,
                ty,
                conversion.missing(),
                conversion.decimal(),
            )?;
            ctx.protect(value);
            let value = match class.as_deref() {
                None if i == 0 && inferred_rows => value,
                None => conversion.infer(ctx, call, value, as_is.contains(i))?,
                Some("factor") => base_call(ctx, call, "factor", &[value])?,
                Some("Date") => base_call(ctx, call, "as.Date", &[value])?,
                Some("POSIXct") => base_call(ctx, call, "as.POSIXct", &[value])?,
                Some(
                    "character" | "logical" | "integer" | "numeric" | "double" | "complex" | "raw",
                ) => value,
                Some(class) => {
                    return Err(ctx.stop(
                        call.err_call(),
                        format!("no method or default for coercing “character” to “{class}”"),
                    ));
                }
            };
            ctx.protect(value);
            columns.push((names[i].clone(), value));
        }
        let explicit = flag(ctx, args.get(7))?.unwrap_or(false);
        let row_names = if explicit {
            args.get(8)
        } else if inferred_rows && types_first_is_dropped {
            if !columns.is_empty() {
                return Err(ctx.stop(call.err_call(), "invalid 'row.names' length"));
            }
            ctx.session_mut().alloc_str_from(&[])?
        } else if inferred_rows && !columns.is_empty() {
            columns.remove(0).1
        } else {
            SEXP_NIL
        };
        let n = if columns.is_empty()
            && (row_names == SEXP_NIL || inferred_rows && types_first_is_dropped)
        {
            0
        } else {
            n
        };
        let rows = row_names_value(ctx, call, row_names, &mut columns, n, explicit)?;
        ctx.protect(rows);
        frame(ctx, &columns, rows)
    })
}

fn check_header_quote(ctx: &mut Ctx, call: &Call, args: &Args, unclosed: bool) -> EvalResult<()> {
    if unclosed {
        let label = if args.get(24) != SEXP_NIL {
            "text".into()
        } else if ctx.session().type_of(args.get(0))? == SexpType::Str {
            string(ctx, call, args.get(0), "file")?
        } else {
            "connection".into()
        };
        return Err(ctx.stop(
            call.err_call(),
            format!("incomplete final line found by readTableHeader on '{label}'"),
        ));
    }
    Ok(())
}

fn scan_type(class: Option<&str>) -> SexpType {
    match class {
        Some("NULL") => SexpType::Nil,
        Some("integer") => SexpType::Int,
        Some("logical") => SexpType::Lgl,
        Some("numeric" | "double") => SexpType::Real,
        Some("complex") => SexpType::Cplx,
        Some("raw") => SexpType::Raw,
        _ => SexpType::Str,
    }
}

fn column_classes(
    ctx: &mut Ctx,
    call: &Call,
    value: SexpId,
    columns: &[Option<String>],
) -> EvalResult<Vec<Option<String>>> {
    if value == SEXP_NIL && !columns.is_empty() {
        return Err(ctx.stop(
            call.err_call(),
            "cannot replicate NULL to a non-zero length",
        ));
    }
    let names = attr(ctx.session_mut(), value, "names")?;
    let classes = r_strings(ctx, value)?;
    if classes.is_empty() {
        return Ok(vec![None; columns.len()]);
    }
    if names == SEXP_NIL {
        return Ok(classes.into_iter().cycle().take(columns.len()).collect());
    }
    let names = r_strings(ctx, names)?;
    let mut result = vec![None; columns.len()];
    let mut unmatched = false;
    for (name, class) in names.iter().zip(classes) {
        if let Some(i) = columns.iter().position(|c| c == name) {
            result[i] = class;
        } else {
            unmatched = true;
        }
    }
    if unmatched {
        ctx.warn_now(
            call.err_call(),
            "not all columns named in 'colClasses' exist",
        )?;
    }
    Ok(result)
}

enum AsIs {
    All(bool),
    Columns(HashSet<usize>),
}

impl AsIs {
    fn from_value(
        ctx: &mut Ctx,
        call: &Call,
        value: SexpId,
        names: &[Option<String>],
    ) -> EvalResult<Self> {
        let s = ctx.session();
        let n = s.xlength(value)?;
        let mut selected = HashSet::new();
        match s.type_of(value)? {
            SexpType::Lgl => {
                let v = s.lgl(value)?;
                if n == 1 {
                    return Ok(Self::All(v.get(0).get() == Some(true)));
                }
                if n != 0 {
                    selected.extend((0..names.len()).filter(|&i| v.get(i % n).get() == Some(true)));
                }
            }
            SexpType::Str => {
                for name in r_strings(ctx, value)? {
                    if let Some(i) = names.iter().position(|n| *n == name) {
                        selected.insert(i);
                    }
                }
            }
            SexpType::Int | SexpType::Real => {
                let (v, _) = ctx.session_mut().coerce(value, SexpType::Int)?;
                let index: Vec<_> = ctx.session().int(v)?.iter().collect();
                if index.iter().any(|&i| i < 0 && i != NA_INTEGER) {
                    selected
                        .extend((0..names.len()).filter(|&i| !index.contains(&(-(i as i32) - 1))));
                } else {
                    selected.extend(index.into_iter().filter(|&i| i > 0).map(|i| i as usize - 1));
                }
            }
            _ => return Err(ctx.stop(call.err_call(), "invalid 'as.is' argument")),
        }
        Ok(Self::Columns(selected))
    }

    fn contains(&self, i: usize) -> bool {
        match self {
            Self::All(value) => *value,
            Self::Columns(columns) => columns.contains(&i),
        }
    }
}

fn row_names_value(
    ctx: &mut Ctx,
    call: &Call,
    value: SexpId,
    columns: &mut Vec<(Option<String>, SexpId)>,
    n: usize,
    explicit: bool,
) -> EvalResult {
    if value == SEXP_NIL {
        let compact = if n == 0 {
            Vec::new()
        } else {
            vec![NA_INTEGER, -(n as i32)]
        };
        return Ok(ctx.session_mut().alloc_int_from(&compact)?);
    }
    let mut rows = value;
    if explicit {
        let len = ctx.session().xlength(value)?;
        let ty = ctx.session().type_of(value)?;
        if len == 1 {
            let i = match ty {
                SexpType::Str => {
                    let name = r_strings(ctx, value)?.into_iter().next().flatten();
                    columns.iter().position(|(c, _)| *c == name)
                }
                SexpType::Int | SexpType::Real => {
                    let value = limit(ctx, value)?;
                    value.checked_sub(1).filter(|&i| i < columns.len())
                }
                _ => return Err(ctx.stop(call.err_call(), "invalid 'row.names' specification")),
            };
            let i = i.ok_or_else(|| {
                ctx.stop(
                    call.err_call(),
                    "attempt to select less than one element in get1index",
                )
            })?;
            rows = columns.remove(i).1;
        } else if ty != SexpType::Str {
            return Err(ctx.stop(call.err_call(), "invalid 'row.names' specification"));
        }
    }
    if ctx.session().xlength(rows)? != n {
        return Err(ctx.stop(call.err_call(), "invalid 'row.names' length"));
    }
    if has_class(ctx, rows, "factor")? || ctx.session().type_of(rows)? != SexpType::Int {
        rows = base_call(ctx, call, "as.character", &[rows])?;
    }
    let names = r_strings(ctx, rows)?;
    if names.iter().any(Option::is_none) {
        return Err(ctx.stop(
            call.err_call(),
            "missing values in 'row.names' are not allowed",
        ));
    }
    let mut seen = HashSet::new();
    if names.iter().any(|v| !seen.insert(v)) {
        return Err(ctx.stop(call.err_call(), "duplicate 'row.names' are not allowed"));
    }
    Ok(rows)
}

fn frame(ctx: &mut Ctx, columns: &[(Option<String>, SexpId)], rows: SexpId) -> EvalResult {
    let names = columns
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let names = make_strings(ctx, &names)?;
    let s = ctx.session_mut();
    let result = s.alloc_list(columns.len())?;
    for (i, (_, col)) in columns.iter().enumerate() {
        s.list_set(result, i, *col)?;
    }
    let class = s.scalar_str("data.frame")?;
    set(s, result, "names", names)?;
    set(s, result, "class", class)?;
    set(s, result, "row.names", rows)?;
    Ok(result)
}

pub(super) fn make_strings(ctx: &mut Ctx, strings: &[Option<String>]) -> EvalResult {
    let s = ctx.session_mut();
    let ids = strings
        .iter()
        .map(|x| {
            x.as_ref().map_or(Ok(CharId::NA), |v| {
                s.mk_char(v.as_bytes(), roxide_value::Encoding::Utf8)
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(s.alloc_str_from(&ids)?)
}

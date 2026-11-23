use std::fmt::Write;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, FormatOpts, SEXP_NIL, ValueApi, encode_string, format_real_vec};

use crate::distn::support::{attr, limit, number, numbers};

use super::support::{Matrix, dimnames};

pub(super) fn labels(ctx: &Ctx, id: SexpId) -> EvalResult<Vec<String>> {
    Ok(ctx
        .session()
        .str_slice(id)?
        .iter()
        .map(|&id| {
            if id == CharId::NA {
                "<NA>".to_owned()
            } else {
                String::from_utf8_lossy(ctx.session().char_bytes(id).unwrap_or_default())
                    .into_owned()
            }
        })
        .collect())
}

pub(super) fn text_width(value: &str) -> usize {
    value.chars().count()
}

fn format_options(ctx: &mut Ctx, call: &Call, digits: SexpId) -> EvalResult<FormatOpts> {
    let digits = if digits == SEXP_NIL {
        ctx.opts().digits
    } else {
        number(ctx, call, digits)?.clamp(1.0, 22.0) as usize
    };
    Ok(FormatOpts {
        digits,
        scipen: ctx.opts().scipen,
    })
}

pub(super) fn blocks(
    widths: &[usize],
    limit: usize,
    margin: usize,
    mut print: impl FnMut(std::ops::Range<usize>),
) {
    let mut start = 0;
    while start < widths.len() {
        let mut end = start + 1;
        let mut used = margin + widths[start] + 1;
        while end < widths.len() && used + widths[end] < limit {
            used += widths[end] + 1;
            end += 1;
        }
        print(start..end);
        start = end;
    }
}

pub(super) fn print_named(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let names = attr(ctx, x, "names")?;
    let opts = format_options(ctx, call, args.get(1))?;
    if names == SEXP_NIL {
        ctx.print_value_default(x, opts.digits)?;
        return Ok(x);
    }
    let values = numbers(ctx, call, x)?;
    limit::<String>(ctx, call, values.len())?;
    let values = format_real_vec(&values, opts);
    let names = labels(ctx, names)?;
    let widths: Vec<_> = values
        .iter()
        .zip(&names)
        .map(|(value, name)| text_width(value).max(text_width(name)))
        .collect();
    blocks(&widths, ctx.opts().width, 0, |range| {
        let mut out = String::new();
        for i in range.clone() {
            let width = widths[i];
            let _ = write!(out, "{:>width$} ", names[i]);
        }
        out.push('\n');
        for i in range {
            let width = widths[i];
            let _ = write!(out, "{:>width$} ", values[i]);
        }
        out.push('\n');
        let _ = ctx.host().write_out(out.as_bytes());
    });
    Ok(x)
}

pub(super) fn print_fields(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    limit::<String>(ctx, call, ctx.session().xlength(x)?)?;
    let names: Vec<_> = ctx
        .session()
        .str_slice(x)?
        .iter()
        .map(|&id| {
            if id == CharId::NA {
                "NA".to_owned()
            } else {
                encode_string(ctx.session().char_bytes(id).unwrap_or_default())
            }
        })
        .collect();
    if names.is_empty() {
        let _ = ctx.host().write_out(b"character(0)\n");
        return Ok(x);
    }
    let width = names.iter().map(|name| text_width(name)).max().unwrap_or(0);
    let index_width = names.len().to_string().len() + 2;
    let columns = (ctx.opts().width.saturating_sub(index_width) / (width + 1)).max(1);
    for (row, values) in names.chunks(columns).enumerate() {
        let index = format!("[{}]", row * columns + 1);
        let mut out = format!("{index:>index_width$}");
        for value in values {
            let _ = write!(out, " {value:<width$}");
        }
        out.push('\n');
        let _ = ctx.host().write_out(out.as_bytes());
    }
    Ok(x)
}

pub(super) fn print_matrix(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let a = Matrix::read(ctx, call, x)?;
    let opts = format_options(ctx, call, args.get(1))?;
    let (rows, cols) = dimnames(ctx, x)?;
    let rows = if rows == SEXP_NIL {
        (1..=a.rows).map(|i| format!("[{i},]")).collect()
    } else {
        labels(ctx, rows)?
    };
    let cols = if cols == SEXP_NIL {
        (1..=a.cols).map(|i| format!("[,{i}]")).collect()
    } else {
        labels(ctx, cols)?
    };
    if rows.len() != a.rows || cols.len() != a.cols {
        return Err(ctx.stop(call.err_call(), "invalid matrix dimnames"));
    }
    if a.rows == 0 || a.cols == 0 {
        ctx.print_value_default(x, opts.digits)?;
        return Ok(x);
    }
    limit::<String>(ctx, call, a.values.len())?;
    let columns: Vec<_> = a
        .values
        .chunks_exact(a.rows)
        .map(|col| format_real_vec(col, opts))
        .collect();
    let widths: Vec<_> = columns
        .iter()
        .zip(&cols)
        .map(|(col, label)| {
            col.iter()
                .map(|v| text_width(v))
                .max()
                .unwrap_or(0)
                .max(text_width(label))
        })
        .collect();
    let row_width = rows.iter().map(|x| text_width(x)).max().unwrap_or(0);
    blocks(&widths, ctx.opts().width, row_width, |range| {
        let mut out = " ".repeat(row_width);
        for j in range.clone() {
            let width = widths[j];
            let _ = write!(out, " {:>width$}", cols[j]);
        }
        out.push('\n');
        let _ = ctx.host().write_out(out.as_bytes());
        for (i, label) in rows.iter().enumerate() {
            out.clear();
            let _ = write!(out, "{label:<row_width$}");
            for j in range.clone() {
                let width = widths[j];
                let _ = write!(out, " {:>width$}", columns[j][i]);
            }
            out.push('\n');
            let _ = ctx.host().write_out(out.as_bytes());
        }
    });
    Ok(x)
}

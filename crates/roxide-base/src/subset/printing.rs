use std::fmt::Write as _;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, FormatOpts, RSession, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi, encode_string,
    format_int_vec, format_lgl_vec, format_real_vec,
};

use crate::ops::attributes::{has_class, row_count};
use crate::ops::common::{attr, chars, element, error, formals, gather, names, number, text};

use super::containers::{factor_values, matrix_dims, named_element};

pub(super) fn print(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x] = formals(ctx.session(), args, ["x"]);
    if x == SEXP_MISSING {
        return Err(error(
            ctx,
            call,
            "argument \"x\" is missing, with no default",
        ));
    }
    let digits = args.named(ctx.session(), "digits");
    let digits = if let Some(v) = digits {
        number(ctx.session_mut(), v)?.clamp(1.0, 22.0) as usize
    } else {
        ctx.opts().digits
    };
    let fmt = FormatOpts {
        digits,
        scipen: ctx.opts().scipen,
    };
    let s = ctx.session_mut();
    let output = if has_class(s, x, "rle") {
        print_rle(s, x, fmt)?
    } else if has_class(s, x, "data.frame") {
        print_frame(s, x, fmt)?
    } else if has_class(s, x, "factor") {
        print_factor(s, x, fmt)?
    } else if let Some((nr, nc)) = matrix_dims(s, x)? {
        print_matrix(s, x, nr, nc, fmt)?
    } else {
        ctx.print_value_default(x, digits)?;
        return Ok(x);
    };
    let _ = ctx.host().write_out(output.as_bytes());
    Ok(x)
}

fn display(s: &mut RSession, x: SexpId, fmt: FormatOpts, quote: bool) -> EvalResult<Vec<String>> {
    let factor = has_class(s, x, "factor");
    let x = if factor { factor_values(s, x)? } else { x };
    Ok(match s.type_of(x)? {
        SexpType::Lgl => format_lgl_vec(&s.lgl(x)?.to_vec()),
        SexpType::Int => format_int_vec(&s.int(x)?.to_vec()),
        SexpType::Real => format_real_vec(&s.real(x)?.to_vec(), fmt),
        SexpType::Raw => s.raw(x)?.iter().map(|v| format!("{v:02x}")).collect(),
        SexpType::Str => s
            .str_slice(x)?
            .iter()
            .map(|&v| {
                if v == CharId::NA {
                    return if quote { "NA" } else { "<NA>" }.into();
                }
                if quote {
                    encode_string(s.char_bytes(v).unwrap_or_default())
                } else {
                    text(s, v)
                }
            })
            .collect(),
        _ => {
            let mut out = Vec::new();
            for i in 0..s.xlength(x)? {
                let v = element(s, x, i)?;
                out.push(roxide_value::deparse(s, v, Default::default())?.join(" "));
            }
            out
        }
    })
}

fn print_rle(s: &mut RSession, x: SexpId, fmt: FormatOpts) -> EvalResult<String> {
    let mut out = String::from("Run Length Encoding\n");
    for (name, label) in [("lengths", "lengths:"), ("values", "values :")] {
        let v = named_element(s, x, name);
        let n = s.xlength(v)?;
        let ty = s.type_of(v)?;
        let kind = match ty {
            SexpType::Int => "int",
            SexpType::Lgl => "logi",
            SexpType::Real => "num",
            SexpType::Str => "chr",
            SexpType::Cplx => "cplx",
            _ => ty.type_name(false),
        };
        let limit = if ty == SexpType::Str { 80 } else { 10 };
        let positions: Vec<_> = (0..n.min(limit)).map(Some).collect();
        let selected = gather(s, v, &positions)?;
        let values = display(s, selected, fmt, true)?;
        let _ = write!(out, "  {label} {kind}");
        if n == 0 {
            out.push_str("(0) ");
        }
        if n > 0 {
            let _ = write!(out, " [1:{n}] {}", values.join(" "));
        }
        if n > limit {
            out.push_str(" ...");
        }
        out.push('\n');
    }
    Ok(out)
}

fn print_factor(s: &mut RSession, x: SexpId, fmt: FormatOpts) -> EvalResult<String> {
    let values = display(s, x, fmt, false)?;
    let names = names(s, x);
    let mut out = String::new();
    if names.is_empty() {
        if values.is_empty() {
            out.push_str("factor(0)");
        } else {
            out.push_str("[1]");
            let width = values.iter().map(|v| v.chars().count()).max().unwrap_or(0);
            for (i, v) in values.iter().enumerate() {
                if i + 1 < values.len() {
                    let _ = write!(out, " {v:<width$}");
                } else {
                    let _ = write!(out, " {v}");
                }
            }
        }
    } else {
        for (&name, value) in names.iter().zip(&values) {
            let name = text(s, name);
            let width = name.len().max(value.len());
            let _ = write!(out, "{name:>width$} ");
        }
        out.push('\n');
        for (&name, value) in names.iter().zip(&values) {
            let width = text(s, name).len().max(value.len());
            let _ = write!(out, "{value:>width$} ");
        }
    }
    let levels: Vec<_> = chars(s, attr(s, x, "levels"))
        .iter()
        .map(|&v| text(s, v))
        .collect();
    let sep = if has_class(s, x, "ordered") {
        " < "
    } else {
        " "
    };
    let _ = write!(out, "\nLevels:");
    if !levels.is_empty() {
        let _ = write!(out, " {}", levels.join(sep));
    }
    out.push('\n');
    Ok(out)
}

fn print_frame(s: &mut RSession, x: SexpId, fmt: FormatOpts) -> EvalResult<String> {
    let labels: Vec<_> = names(s, x).iter().map(|&v| text(s, v)).collect();
    let nr = row_count(s, x)?;
    let r = s.attr_row_names(x)?;
    let rows = display(s, r, fmt, false)?;
    if labels.is_empty() {
        return Ok(format!("data frame with 0 columns and {nr} rows\n"));
    }
    if nr == 0 {
        return Ok(format!(
            "[1] {}\n<0 rows> (or 0-length row.names)\n",
            labels.join(" ")
        ));
    }
    let x = s.list_slice(x)?.to_vec();
    let cols = x
        .into_iter()
        .map(|v| display(s, v, fmt, false))
        .collect::<EvalResult<Vec<_>>>()?;
    Ok(table(&rows, &labels, &cols, false))
}

fn print_matrix(
    s: &mut RSession,
    x: SexpId,
    nr: usize,
    nc: usize,
    fmt: FormatOpts,
) -> EvalResult<String> {
    let axis = attr(s, x, "dimnames");
    let labels = |i, n| -> Vec<String> {
        let ids = chars(s, s.list_get(axis, i).unwrap_or(SEXP_NIL));
        if ids.is_empty() {
            (1..=n)
                .map(|j| {
                    if i == 0 {
                        format!("[{j},]")
                    } else {
                        format!("[,{j}]")
                    }
                })
                .collect()
        } else {
            ids.iter().map(|&v| text(s, v)).collect()
        }
    };
    let rows = labels(0, nr);
    let columns = labels(1, nc);
    let values = display(s, x, fmt, true)?;
    let cols: Vec<_> = (0..nc)
        .map(|i| values[i * nr..(i + 1) * nr].to_vec())
        .collect();
    let left = s.type_of(x)? == SexpType::Str;
    if nr == 0 && nc == 0 {
        return Ok("<0 x 0 matrix>\n".into());
    }
    Ok(table(&rows, &columns, &cols, left))
}

fn table(rows: &[String], labels: &[String], cols: &[Vec<String>], left: bool) -> String {
    let width = |v: &str| v.chars().count();
    let row_width = rows.iter().map(|v| width(v)).max().unwrap_or(0);
    let widths: Vec<_> = labels
        .iter()
        .zip(cols)
        .map(|(n, v)| v.iter().map(|v| width(v)).max().unwrap_or(0).max(width(n)))
        .collect();
    let mut out = " ".repeat(row_width);
    for (label, &w) in labels.iter().zip(&widths) {
        if left {
            let _ = write!(out, " {label:<w$}");
        } else {
            let _ = write!(out, " {label:>w$}");
        }
    }
    out.push('\n');
    for (i, label) in rows.iter().enumerate() {
        let _ = write!(out, "{label:<row_width$}");
        for (col, &w) in cols.iter().zip(&widths) {
            let value = col.get(i).map_or("", String::as_str);
            if left {
                let _ = write!(out, " {value:<w$}");
            } else {
                let _ = write!(out, " {value:>w$}");
            }
        }
        out.push('\n');
    }
    out
}

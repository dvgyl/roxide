use std::fmt::Write as _;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, FormatOpts, RSession, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi, encode_string,
    format_int_vec, format_lgl_vec,
};

use crate::numbers::{complex_fields, real_fields};
use crate::ops::attributes::{has_class, row_count};
use crate::ops::common::{
    atomic, attr, chars, element, error, flag, formals, gather, names, number, text, vector_limit,
};

use super::containers::{factor_values, matrix_dims, named_element};

pub(super) fn print(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, digits, quote, _na_print, gap, right] = formals(
        ctx.session(),
        args,
        ["x", "digits", "quote", "na.print", "print.gap", "right"],
    );
    if x == SEXP_MISSING {
        return Err(error(
            ctx,
            call,
            "argument \"x\" is missing, with no default",
        ));
    }
    if ctx.session().is_s4(x)? && ctx.show_s4(x, call.env)? {
        return Ok(x);
    }
    let digits = if matches!(digits, SEXP_MISSING | SEXP_NIL) {
        ctx.opts().digits
    } else {
        number(ctx.session_mut(), digits)?.clamp(1.0, 22.0) as usize
    };
    let gap = if matches!(gap, SEXP_MISSING | SEXP_NIL) {
        1
    } else {
        let gap = number(ctx.session_mut(), gap)?;
        if !(0.0..=1024.0).contains(&gap) {
            return Err(error(ctx, call, "invalid 'print.gap' argument"));
        }
        gap as usize
    };
    let quote = flag(ctx, quote, true)?;
    let right = flag(ctx, right, false)?;
    let settings = PrintSettings {
        fmt: FormatOpts {
            digits,
            scipen: ctx.opts().scipen,
        },
        width: ctx.opts().width,
        quote,
        right,
        gap,
    };
    print_default(ctx, call, x, &settings, "", 0)?;
    Ok(x)
}

struct PrintSettings {
    fmt: FormatOpts,
    width: usize,
    quote: bool,
    right: bool,
    gap: usize,
}

fn print_default(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    settings: &PrintSettings,
    prefix: &str,
    depth: usize,
) -> EvalResult<()> {
    let stack_cap = (ctx.stack_budget() / 4096).clamp(128, 2048);
    if depth >= ctx.opts().expressions.min(stack_cap) {
        return Err(error(ctx, call, "evaluation nested too deeply"));
    }
    let s = ctx.session_mut();
    let ty = s.type_of(x)?;
    let output = if has_class(s, x, "rle") {
        Some(print_rle(s, x, settings.fmt)?)
    } else if has_class(s, x, "data.frame") {
        Some(print_frame(s, x, settings.fmt)?)
    } else if has_class(s, x, "factor") {
        Some(print_factor(s, x, settings.fmt)?)
    } else if let Some((nr, nc)) = matrix_dims(s, x)? {
        Some(print_matrix(s, x, nr, nc, settings)?)
    } else if ty == SexpType::Vec {
        print_list(ctx, call, x, settings, prefix, depth)?;
        None
    } else if atomic(ty) {
        vector_limit(ctx, call, ctx.session().xlength(x)?, size_of::<String>())?;
        Some(print_vector(ctx.session_mut(), x, settings)?)
    } else if let Some(output) = print_bytecode(ctx, x)? {
        Some(output)
    } else {
        ctx.print_value_default(x, settings.fmt.digits)?;
        None
    };
    if let Some(output) = output {
        write_out(ctx, call, &output)?;
    }
    if ["rle", "factor", "data.frame"]
        .iter()
        .any(|class| has_class(ctx.session(), x, class))
    {
        return Ok(());
    }
    for (sym, value) in ctx.session().attrs(x)? {
        let name = ctx.session().sym_name(sym)?;
        if matches!(name, "names" | "dim" | "dimnames" | "class" | "tsp") {
            continue;
        }
        let label = format!("attr(,{})", encode_string(name.as_bytes()));
        write_out(ctx, call, &format!("{label}\n"))?;
        print_element(ctx, call, value, settings, &label, depth + 1)?;
    }
    Ok(())
}

fn print_bytecode(ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<String>> {
    if crate::compiler::source(ctx, value).is_some() {
        return Ok(Some(format!("<bytecode: 0x{:x}>\n", value.to_bits())));
    }
    let Ok((formals, body, env)) = ctx.session().closure_parts(value) else {
        return Ok(None);
    };
    let source = crate::compiler::source(ctx, body);
    if source.is_none() && !roxide_eval::vm::is_compiled(ctx, value) {
        return Ok(None);
    }
    let closure = if let Some(source) = source {
        ctx.session_mut().alloc_closure(formals, source, env)?
    } else {
        value
    };
    let mut out = ctx.deparse_lines(closure, Default::default())?.join("\n");
    let address = if source.is_some() { body } else { value };
    let _ = write!(out, "\n<bytecode: 0x{:x}>\n", address.to_bits());
    if env != roxide_value::GLOBAL_ENV {
        out.push_str(&ctx.environment_label(env)?);
        out.push('\n');
    }
    Ok(Some(out))
}

fn write_out(ctx: &mut Ctx, call: &Call, output: &str) -> EvalResult<()> {
    ctx.host()
        .write_out(output.as_bytes())
        .map_err(|err| error(ctx, call, err.to_string()))
}

fn print_list(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    settings: &PrintSettings,
    prefix: &str,
    depth: usize,
) -> EvalResult<()> {
    let values = ctx.session().list_slice(x)?.to_vec();
    let labels = names(ctx.session(), x).to_vec();
    if values.is_empty() {
        let named = attr(ctx.session(), x, "names") != SEXP_NIL;
        return write_out(ctx, call, if named { "named list()\n" } else { "list()\n" });
    }
    for (i, x) in values.into_iter().enumerate() {
        let suffix = match labels.get(i) {
            None | Some(&CharId::BLANK) => format!("[[{}]]", i + 1),
            Some(&CharId::NA) => "$<NA>".to_owned(),
            Some(&label) => {
                let label = text(ctx.session(), label);
                let sym = ctx.session_mut().intern(&label)?;
                let escaped =
                    roxide_value::deparse(ctx.session(), sym.as_sexp(), Default::default())?
                        .join("");
                format!("${escaped}")
            }
        };
        let label = format!("{prefix}{suffix}");
        write_out(ctx, call, &format!("{label}\n"))?;
        print_element(ctx, call, x, settings, &label, depth + 1)?;
        write_out(ctx, call, "\n")?;
    }
    Ok(())
}

fn print_element(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    settings: &PrintSettings,
    prefix: &str,
    depth: usize,
) -> EvalResult<()> {
    if ctx.session().is_s4(x)? && ctx.show_s4(x, call.env)? {
        return Ok(());
    }
    if attr(ctx.session(), x, "class") != SEXP_NIL {
        ctx.call_function("print", &[(None, x)], call.env)?;
        return Ok(());
    }
    print_default(ctx, call, x, settings, prefix, depth)
}

fn print_vector(s: &mut RSession, x: SexpId, settings: &PrintSettings) -> EvalResult<String> {
    let ty = s.type_of(x)?;
    let values = display(s, x, settings.fmt, settings.quote)?;
    let dim = attr(s, x, "dim");
    let is_1d_array = dim != SEXP_NIL && s.type_of(dim)? == SexpType::Int && s.int(dim)?.len() == 1;
    let mut array_header: Option<String> = None;
    let mut dim1_labels: Option<Vec<String>> = None;
    if is_1d_array {
        let dimnames = attr(s, x, "dimnames");
        if dimnames != SEXP_NIL
            && s.type_of(dimnames)? == SexpType::Vec
            && s.xlength(dimnames)? >= 1
        {
            if let Some(&hdr_id) = names(s, dimnames).first()
                && hdr_id != CharId::BLANK
                && hdr_id != CharId::NA
            {
                let h = text(s, hdr_id);
                if !h.is_empty() {
                    array_header = Some(h);
                }
            }
            let first = s.list_get(dimnames, 0)?;
            if first != SEXP_NIL && s.type_of(first)? == SexpType::Str {
                dim1_labels = Some(
                    s.str_slice(first)?
                        .iter()
                        .map(|&id| {
                            if id == CharId::NA {
                                "<NA>".to_owned()
                            } else {
                                text(s, id)
                            }
                        })
                        .collect(),
                );
            }
        }
    }
    let named = dim1_labels.is_some() || attr(s, x, "names") != SEXP_NIL;
    if values.is_empty() {
        let kind = if ty == SexpType::Real {
            "numeric"
        } else {
            ty.type_name(false)
        };
        return Ok(format!("{}{kind}(0)\n", if named { "named " } else { "" }));
    }
    let labels: Vec<_> = if let Some(lbls) = dim1_labels {
        lbls
    } else {
        names(s, x)
            .iter()
            .map(|&id| {
                if id == CharId::NA {
                    "<NA>".to_owned()
                } else {
                    text(s, id)
                }
            })
            .collect()
    };
    let width = values
        .iter()
        .chain(&labels)
        .map(|v| v.chars().count())
        .max()
        .unwrap_or(0);
    let gap = " ".repeat(settings.gap);
    let mut out = String::new();
    if let Some(hdr) = array_header {
        out.push_str(&hdr);
        out.push('\n');
    }
    if named {
        let across = (settings.width / (width + settings.gap).max(1)).max(1);
        for (row, values) in values.chunks(across).enumerate() {
            for label in labels.iter().skip(row * across).take(values.len()) {
                let _ = write!(out, "{label:>width$}{gap}");
            }
            out.push('\n');
            for value in values {
                let _ = write!(out, "{value:>width$}{gap}");
            }
            out.push('\n');
        }
    } else {
        let index_width = values.len().to_string().len() + 2;
        let across =
            ((settings.width.saturating_sub(index_width)) / (width + settings.gap).max(1)).max(1);
        for (i, row) in values.chunks(across).enumerate() {
            let label = format!("[{}]", i * across + 1);
            let _ = write!(out, "{label:>index_width$}");
            for value in row {
                if ty == SexpType::Str && !settings.right {
                    let _ = write!(out, "{gap}{value:<width$}");
                } else {
                    let _ = write!(out, "{gap}{value:>width$}");
                }
            }
            out.push('\n');
        }
    }
    Ok(out)
}

fn display(s: &mut RSession, x: SexpId, fmt: FormatOpts, quote: bool) -> EvalResult<Vec<String>> {
    let factor = has_class(s, x, "factor");
    let x = if factor { factor_values(s, x)? } else { x };
    Ok(match s.type_of(x)? {
        SexpType::Lgl => format_lgl_vec(&s.lgl(x)?.to_vec()),
        SexpType::Int => format_int_vec(&s.int(x)?.to_vec()),
        SexpType::Real => real_fields(&s.real(x)?.to_vec(), fmt, 0),
        SexpType::Cplx => complex_fields(&s.cplx(x)?.to_vec(), fmt, 0, false),
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
                    let encoded = encode_string(s.char_bytes(v).unwrap_or_default());
                    encoded[1..encoded.len() - 1].replace("\\\"", "\"")
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
    settings: &PrintSettings,
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
    let cols = (0..nc)
        .map(|i| {
            let pos: Vec<_> = (i * nr..(i + 1) * nr).map(Some).collect();
            let col = gather(s, x, &pos)?;
            display(s, col, settings.fmt, settings.quote)
        })
        .collect::<EvalResult<Vec<_>>>()?;
    let left = s.type_of(x)? == SexpType::Str && !settings.right;
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

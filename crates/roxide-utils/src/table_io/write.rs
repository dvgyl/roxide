use roxide_base::scan::{base_call, r_strings};
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, FALSE_SCALAR, SEXP_NIL, SexpType, TRUE_SCALAR, ValueApi};

use super::{attr, choice, flag, has_class, string};

pub(super) fn write_table(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sep = string(ctx, call, args.get(4), "sep")?;
    let eol = string(ctx, call, args.get(5), "eol")?;
    let na = string(ctx, call, args.get(6), "na")?;
    let decimal = roxide_base::scan::decimal(ctx, call, args.get(7))?;
    let double = choice(ctx, call, args.get(10), &["escape", "double"])? == "double";
    let append = flag(ctx, args.get(2))?.unwrap_or(false);
    let encoding = string(ctx, call, args.get(11), "fileEncoding")?;
    let table = Table::new(ctx, call, args.get(0), decimal)?;
    let quote = Quotations::new(ctx, call, args.get(3), table.names.len())?;
    let rows = match ctx.session().type_of(args.get(8))? {
        SexpType::Lgl => match flag(ctx, args.get(8))? {
            Some(true) => Some(table.row_names),
            Some(false) => None,
            None => return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed")),
        },
        SexpType::Nil => return Err(ctx.stop(call.err_call(), "invalid 'row.names' specification")),
        _ => {
            let names = r_strings(ctx, args.get(8))?;
            if names.len() != table.rows {
                return Err(ctx.stop(call.err_call(), "invalid 'row.names' specification"));
            }
            Some(names)
        }
    };
    let mut output = Output {
        buffer: String::new(),
        sep,
        eol,
        double,
        na,
        bound: ctx.host().limits().max_vector_bytes,
    };
    let names = args.get(9);
    match ctx.session().type_of(names)? {
        SexpType::Str => {
            let names = r_strings(ctx, names)?;
            if names.len() != table.names.len() {
                return Err(ctx.stop(call.err_call(), "invalid 'col.names' specification"));
            }
            output.header(ctx, call, &names, false, quote.header())?;
        }
        SexpType::Lgl => {
            let slot = flag(ctx, names)?;
            if slot != Some(false) {
                if slot.is_none() && rows.is_none() {
                    return Err(ctx.stop(
                        call.err_call(),
                        "'col.names = NA' makes no sense when 'row.names = FALSE'",
                    ));
                }
                output.header(ctx, call, &table.names, slot.is_none(), quote.header())?;
            }
        }
        _ => return Err(ctx.stop(call.err_call(), "invalid 'col.names' specification")),
    }
    for i in 0..table.rows {
        if let Some(rows) = &rows {
            output.cell(
                ctx,
                call,
                rows.get(i).and_then(Option::as_deref),
                quote.rows(),
            )?;
        }
        for j in 0..table.names.len() {
            if j != 0 || rows.is_some() {
                output.separator(ctx, call)?;
            }
            let (column, index) = if table.matrix {
                (&table.columns[0], j * table.rows + i)
            } else {
                let column = &table.columns[j];
                (column, i % column.values.len().max(1))
            };
            output.cell(
                ctx,
                call,
                column.values.get(index).and_then(Option::as_deref),
                quote.column(j, column.text),
            )?;
        }
        output.end_line(ctx, call)?;
    }
    commit_output(ctx, call, args.get(1), &output.buffer, append, &encoding)
}

struct Table {
    rows: usize,
    row_names: Vec<Option<String>>,
    names: Vec<Option<String>>,
    columns: Vec<Column>,
    matrix: bool,
}

impl Table {
    fn new(ctx: &mut Ctx, call: &Call, value: SexpId, decimal: char) -> EvalResult<Self> {
        let s = ctx.session_mut();
        let dims = attr(s, value, "dim")?;
        if dims != SEXP_NIL && s.xlength(dims)? == 2 {
            let dims = s.int(dims)?;
            let rows = dims.get(0).max(0) as usize;
            let width = dims.get(1).max(0) as usize;
            guard_shape(ctx, call, rows, width)?;
            let dn = attr(ctx.session_mut(), value, "dimnames")?;
            let (row_names, names) = if dn == SEXP_NIL {
                (default_rows(rows), default_cols(width))
            } else {
                let s = ctx.session();
                let r = s.list_get(dn, 0)?;
                let c = s.list_get(dn, 1)?;
                let rows = if r == SEXP_NIL {
                    default_rows(rows)
                } else {
                    r_strings(ctx, r)?
                };
                let cols = if c == SEXP_NIL {
                    default_cols(width)
                } else {
                    r_strings(ctx, c)?
                };
                (rows, cols)
            };
            return Ok(Self {
                rows,
                row_names,
                names,
                columns: vec![Column::new(ctx, call, value, decimal)?],
                matrix: true,
            });
        }
        let is_frame = has_class(ctx, value, "data.frame")?;
        match ctx.session().type_of(value)? {
            SexpType::Vec => {
                let s = ctx.session_mut();
                let values = s.list_slice(value)?.to_vec();
                let width = values.len();
                let labels = attr(s, value, "names")?;
                let row_labels = s.attr_row_names(value)?;
                let mut rows = s.xlength(row_labels)?;
                if !is_frame {
                    for &value in &values {
                        rows = rows.max(s.xlength(value)?);
                    }
                }
                guard_shape(ctx, call, rows, width)?;
                let row_names = if row_labels == SEXP_NIL {
                    default_rows(rows)
                } else {
                    r_strings(ctx, row_labels)?
                };
                let names = if labels == SEXP_NIL {
                    default_cols(width)
                } else {
                    r_strings(ctx, labels)?
                };
                let mut columns = Vec::with_capacity(width);
                for value in values {
                    let column = Column::new(ctx, call, value, decimal)?;
                    if (is_frame && rows != column.values.len())
                        || (rows != 0
                            && (column.values.is_empty() || rows % column.values.len() != 0))
                    {
                        return Err(
                            ctx.stop(call.err_call(), "arguments imply differing number of rows")
                        );
                    }
                    columns.push(column);
                }
                Ok(Self {
                    rows,
                    row_names,
                    names,
                    columns,
                    matrix: false,
                })
            }
            SexpType::Nil => Ok(Self {
                rows: 0,
                row_names: Vec::new(),
                names: Vec::new(),
                columns: Vec::new(),
                matrix: false,
            }),
            _ => {
                let s = ctx.session_mut();
                let rows = s.xlength(value)?;
                guard_shape(ctx, call, rows, 1)?;
                let names = attr(ctx.session_mut(), value, "names")?;
                let row_names = if names == SEXP_NIL {
                    default_rows(rows)
                } else {
                    r_strings(ctx, names)?
                };
                Ok(Self {
                    rows,
                    row_names,
                    names: vec![Some("x".into())],
                    columns: vec![Column::new(ctx, call, value, decimal)?],
                    matrix: false,
                })
            }
        }
    }
}

fn guard_shape(ctx: &mut Ctx, call: &Call, rows: usize, width: usize) -> EvalResult<()> {
    let limit = ctx.host().limits().max_vector_bytes;
    if rows.max(width).max(rows.saturating_mul(width)) > limit / size_of::<Option<String>>() {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    Ok(())
}

fn default_rows(n: usize) -> Vec<Option<String>> {
    (1..=n).map(|i| Some(i.to_string())).collect()
}

fn default_cols(n: usize) -> Vec<Option<String>> {
    (1..=n).map(|i| Some(format!("V{i}"))).collect()
}

struct Column {
    values: Vec<Option<String>>,
    text: bool,
}

impl Column {
    fn new(ctx: &mut Ctx, call: &Call, value: SexpId, decimal: char) -> EvalResult<Self> {
        let factor = has_class(ctx, value, "factor")?;
        let ty = ctx.session().type_of(value)?;
        let text = factor || ty == SexpType::Str;
        let class = attr(ctx.session_mut(), value, "class")?;
        let formatted = class != SEXP_NIL && (factor || !has_class(ctx, value, "AsIs")?);
        let scipen = ctx.opts().scipen;
        let s = ctx.session();
        let mut values = match ty {
            _ if formatted => {
                let chars = base_call(ctx, call, "as.character", &[value])?;
                r_strings(ctx, chars)?
            }
            SexpType::Real => s.real(value)?.iter().map(|n| real(n, scipen)).collect(),
            SexpType::Cplx => s
                .cplx(value)?
                .iter()
                .map(|n| {
                    if n.re.is_nan() || n.im.is_nan() {
                        None
                    } else {
                        let re = real(n.re, scipen)?;
                        let im = real(n.im.abs(), scipen)?;
                        let sign = if n.im < 0.0 { '-' } else { '+' };
                        Some(format!("{re}{sign}{im}i"))
                    }
                })
                .collect(),
            _ => r_strings(ctx, value)?,
        };
        if decimal != '.' && !formatted && matches!(ty, SexpType::Real | SexpType::Cplx) {
            for value in values.iter_mut().flatten() {
                *value = value.replace('.', &decimal.to_string());
            }
        }
        Ok(Self { values, text })
    }
}

fn real(value: f64, scipen: i32) -> Option<String> {
    if value.is_nan() {
        return None;
    }
    if value.is_infinite() {
        return Some(
            if value.is_sign_positive() {
                "Inf"
            } else {
                "-Inf"
            }
            .into(),
        );
    }
    if value == 0.0 {
        return Some("0".into());
    }
    let scientific = format!("{value:.14e}");
    let (mantissa, power) = scientific.split_once('e')?;
    let exp = power.parse::<i32>().ok()?;
    let mantissa = trim_decimal(mantissa);
    let sci = format!("{mantissa}e{exp:+03}");
    let places = (14 - exp).max(0) as usize;
    let fixed = format!("{value:.places$}");
    let fixed = trim_decimal(&fixed);
    Some(
        if sci.len() as i64 + i64::from(scipen) < fixed.len() as i64 {
            sci
        } else {
            fixed.to_owned()
        },
    )
}

fn trim_decimal(text: &str) -> &str {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    }
}

enum Quotations {
    Text,
    None,
    Selected(Vec<usize>),
}

impl Quotations {
    fn new(ctx: &mut Ctx, call: &Call, value: SexpId, width: usize) -> EvalResult<Self> {
        let s = ctx.session();
        match s.type_of(value)? {
            SexpType::Lgl if s.xlength(value)? == 1 => match flag(ctx, value)? {
                Some(true) => Ok(Self::Text),
                Some(false) => Ok(Self::None),
                None => Err(ctx.stop(
                    call.err_call(),
                    "'quote' must be 'TRUE', 'FALSE' or numeric",
                )),
            },
            SexpType::Int | SexpType::Real => {
                let (numbers, _) = ctx.session_mut().coerce(value, SexpType::Int)?;
                let numbers = ctx.session().int(numbers)?;
                if numbers.iter().any(|i| i <= 0 || i as usize > width) {
                    return Err(ctx.stop(call.err_call(), "invalid numbers in 'quote'"));
                }
                Ok(Self::Selected(numbers.iter().map(|i| i as usize).collect()))
            }
            _ => Err(ctx.stop(call.err_call(), "invalid 'quote' specification")),
        }
    }

    fn header(&self) -> bool {
        !matches!(self, Self::None)
    }

    fn rows(&self) -> bool {
        match self {
            Self::Text => true,
            Self::None => false,
            Self::Selected(_) => true,
        }
    }

    fn column(&self, column: usize, text: bool) -> bool {
        match self {
            Self::Text => text,
            Self::None => false,
            Self::Selected(columns) => text && columns.contains(&(column + 1)),
        }
    }
}

struct Output {
    buffer: String,
    sep: String,
    eol: String,
    double: bool,
    na: String,
    bound: usize,
}

impl Output {
    fn append(
        ctx: &mut Ctx,
        call: &Call,
        buf: &mut String,
        value: &str,
        bound: usize,
    ) -> EvalResult<()> {
        if buf.len().saturating_add(value.len()) > bound {
            return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
        }
        buf.push_str(value);
        Ok(())
    }

    fn header(
        &mut self,
        ctx: &mut Ctx,
        call: &Call,
        names: &[Option<String>],
        first_slot: bool,
        quote: bool,
    ) -> EvalResult<()> {
        if first_slot || names.is_empty() {
            self.cell(ctx, call, Some(""), quote)?;
            if !names.is_empty() {
                self.separator(ctx, call)?;
            }
        }
        for (i, name) in names.iter().enumerate() {
            if i != 0 {
                self.separator(ctx, call)?;
            }
            self.cell(ctx, call, name.as_deref().or(Some("NA")), quote)?;
        }
        self.end_line(ctx, call)
    }

    fn separator(&mut self, ctx: &mut Ctx, call: &Call) -> EvalResult<()> {
        Self::append(ctx, call, &mut self.buffer, &self.sep, self.bound)
    }

    fn end_line(&mut self, ctx: &mut Ctx, call: &Call) -> EvalResult<()> {
        Self::append(ctx, call, &mut self.buffer, &self.eol, self.bound)
    }

    fn cell(
        &mut self,
        ctx: &mut Ctx,
        call: &Call,
        text: Option<&str>,
        quote: bool,
    ) -> EvalResult<()> {
        let value = text.unwrap_or(&self.na);
        if quote && text.is_some() {
            let escaped = value.replace('"', if self.double { "\"\"" } else { "\\\"" });
            Self::append(
                ctx,
                call,
                &mut self.buffer,
                &format!("\"{escaped}\""),
                self.bound,
            )
        } else {
            Self::append(ctx, call, &mut self.buffer, value, self.bound)
        }
    }
}

fn commit_output(
    ctx: &mut Ctx,
    call: &Call,
    dest: SexpId,
    output: &str,
    append: bool,
    encoding: &str,
) -> EvalResult {
    ctx.scoped(|ctx| {
        let path = if ctx.session().type_of(dest)? == SexpType::Str {
            Some(string(ctx, call, dest, "file")?)
        } else {
            None
        };
        if path.as_deref() == Some("") {
            ctx.host()
                .write_out(output.as_bytes())
                .map_err(|e| ctx.stop(call.err_call(), e.to_string()))?;
            return Ok(SEXP_NIL);
        }
        let data = match encoding {
            "latin1" | "Latin-1" | "ISO-8859-1" => output
                .chars()
                .map(|c| u8::try_from(c as u32).unwrap_or(b'?'))
                .collect(),
            _ => output.as_bytes().to_vec(),
        };
        let s = ctx.session_mut();
        let id = s.mk_char(&data, Encoding::Bytes)?;
        let payload = s.alloc_str_from(&[id])?;
        ctx.protect(payload);
        let sep = ctx.session_mut().alloc_str_from(&[CharId::BLANK])?;
        ctx.protect(sep);
        let open = path.is_some() && append;
        let con = if open {
            let mode = ctx.session_mut().scalar_str("at")?;
            base_call(ctx, call, "file", &[dest, mode])?
        } else {
            dest
        };
        ctx.protect(con);
        let result = base_call(
            ctx,
            call,
            "writeLines",
            &[
                payload,
                con,
                sep,
                if encoding.is_empty() {
                    FALSE_SCALAR
                } else {
                    TRUE_SCALAR
                },
            ],
        );
        if open {
            let closed = base_call(ctx, call, "close", &[con]);
            result.and(closed)
        } else {
            result
        }
    })
}

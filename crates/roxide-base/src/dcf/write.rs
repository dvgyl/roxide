use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, SEXP_NIL, SexpType, TRUE_SCALAR, ValueApi};

use super::{attr, flag, keys};
use crate::scan::{base_call, r_strings};

pub(super) fn write_dcf(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let append = flag(ctx, call, args.get(2))?;
    let _use_bytes = flag(ctx, call, args.get(3))?;
    let bound = ctx.host().limits().max_vector_bytes;
    let indent = number(ctx, call, args.get(4), bound)?;
    let width = number(ctx, call, args.get(5), bound)?;
    let keep = keys(ctx, args.get(6))?;
    let data = Table::new(ctx, call, args.get(0))?;
    if data
        .rows
        .saturating_mul(data.names.len())
        .saturating_mul(size_of::<SexpId>())
        > bound
    {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    let mut output = Output {
        text: String::new(),
        bound,
    };
    for row in 0..data.rows {
        let mut first = true;
        for (col, name) in data.names.iter().enumerate() {
            let Some(name) = name.as_deref() else {
                return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
            };
            let preserve = keep.iter().flatten().any(|key| key == name.as_bytes());
            let values = data.cell(ctx, call, row, col)?;
            for value in values.iter().flatten() {
                if first && !output.text.is_empty() {
                    output.push(ctx, call, "\n")?;
                }
                first = false;
                output.field(ctx, call, name, value, (width, indent), preserve)?;
            }
        }
    }
    write_output(ctx, call, args.get(1), append, output.text)
}

struct Table {
    rows: usize,
    names: Vec<Option<String>>,
    columns: Vec<Column>,
    matrix: bool,
}

enum Column {
    Atomic(Vec<Option<String>>),
    List(Vec<SexpId>),
}

impl Column {
    fn new(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Self> {
        if ctx.session().type_of(x)? == SexpType::Vec {
            return Ok(Self::List(ctx.session().list_slice(x)?.to_vec()));
        }
        Ok(Self::Atomic(strings(ctx, call, x)?))
    }

    fn len(&self) -> usize {
        match self {
            Self::Atomic(x) => x.len(),
            Self::List(x) => x.len(),
        }
    }

    fn cell(&self, ctx: &mut Ctx, call: &Call, index: usize) -> EvalResult<Vec<Option<String>>> {
        match self {
            Self::Atomic(values) => Ok(values.get(index).cloned().into_iter().collect()),
            Self::List(values) => match values.get(index) {
                Some(&value) => strings(ctx, call, value),
                None => Ok(Vec::new()),
            },
        }
    }
}

impl Table {
    fn new(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Self> {
        let s = ctx.session_mut();
        let dim = attr(s, x, "dim")?;
        if dim != SEXP_NIL && s.xlength(dim)? == 2 {
            let rows = s.int(dim)?.get(0).max(0) as usize;
            let dimnames = attr(s, x, "dimnames")?;
            let names = if dimnames == SEXP_NIL {
                SEXP_NIL
            } else {
                s.list_get(dimnames, 1)?
            };
            if names == SEXP_NIL && s.xlength(x)? > 0 {
                return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
            }
            return Ok(Self {
                rows,
                names: r_strings(ctx, names)?,
                columns: vec![Column::new(ctx, call, x)?],
                matrix: true,
            });
        }
        let names = attr(s, x, "names")?;
        let row_names = s.attr_row_names(x)?;
        let rows = s.xlength(row_names)?;
        let list = s.type_of(x)? == SexpType::Vec;
        if names == SEXP_NIL && s.xlength(x)? != 0 {
            return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
        }
        let names = r_strings(ctx, names)?;
        let columns = if list {
            let values = ctx.session().list_slice(x)?.to_vec();
            values
                .into_iter()
                .map(|x| Column::new(ctx, call, x))
                .collect::<EvalResult<Vec<_>>>()?
        } else {
            strings(ctx, call, x)?
                .into_iter()
                .map(|x| Column::Atomic(vec![x]))
                .collect()
        };
        let rows = if rows == 0 {
            columns.iter().map(Column::len).max().unwrap_or(0)
        } else {
            rows
        };
        if columns.iter().any(|column| {
            column.len() == 0 && rows != 0
                || rows.checked_rem(column.len()).is_some_and(|rem| rem != 0)
        }) {
            return Err(ctx.stop(call.err_call(), "arguments imply differing number of rows"));
        }
        Ok(Self {
            rows,
            names,
            columns,
            matrix: false,
        })
    }

    fn cell(
        &self,
        ctx: &mut Ctx,
        call: &Call,
        row: usize,
        col: usize,
    ) -> EvalResult<Vec<Option<String>>> {
        if self.matrix {
            self.columns[0].cell(ctx, call, col * self.rows + row)
        } else {
            let column = &self.columns[col];
            column.cell(ctx, call, row % column.len().max(1))
        }
    }
}

fn strings(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<Vec<Option<String>>> {
    ctx.scoped(|ctx| {
        let chars = base_call(ctx, call, "as.character", &[value])?;
        ctx.protect(chars);
        r_strings(ctx, chars)
    })
}

struct Output {
    text: String,
    bound: usize,
}

impl Output {
    fn push(&mut self, ctx: &mut Ctx, call: &Call, text: &str) -> EvalResult<()> {
        if text.len() > self.bound.saturating_sub(self.text.len()) {
            return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
        }
        self.text.push_str(text);
        Ok(())
    }

    fn field(
        &mut self,
        ctx: &mut Ctx,
        call: &Call,
        name: &str,
        value: &str,
        (width, indent): (usize, usize),
        preserve: bool,
    ) -> EvalResult<()> {
        self.push(ctx, call, name)?;
        self.push(ctx, call, ":")?;
        if value.is_empty() {
            return self.push(ctx, call, "\n");
        }
        if preserve {
            let mut lines = value.split_terminator('\n');
            let first = lines.next().unwrap_or_default();
            self.push(ctx, call, " ")?;
            self.push(ctx, call, first)?;
            self.push(ctx, call, "\n")?;
            for line in lines {
                self.push(ctx, call, " ")?;
                self.push(ctx, call, if line.is_empty() { "." } else { line })?;
                self.push(ctx, call, "\n")?;
            }
            if value.ends_with("\n\n") {
                self.push(ctx, call, " \n")?;
            }
            return Ok(());
        }

        let value = value.trim_end();
        let lines: Vec<_> = value.split('\n').collect();
        let mut paragraph = String::new();
        let mut initial = true;
        for (index, line) in lines.iter().enumerate() {
            let line = line.trim();
            if line.is_empty() && index + 1 < lines.len() {
                self.wrap(
                    ctx,
                    call,
                    &paragraph,
                    if initial { name.chars().count() + 1 } else { 1 },
                    width,
                    indent,
                )?;
                self.push(ctx, call, " .\n")?;
                paragraph.clear();
                initial = false;
            } else if !line.is_empty() {
                if !paragraph.is_empty() {
                    paragraph.push(' ');
                }
                paragraph.push_str(line);
            }
        }
        self.wrap(
            ctx,
            call,
            &paragraph,
            if initial { name.chars().count() + 1 } else { 1 },
            width,
            indent,
        )
    }

    fn wrap(
        &mut self,
        ctx: &mut Ctx,
        call: &Call,
        paragraph: &str,
        prefix: usize,
        width: usize,
        indent: usize,
    ) -> EvalResult<()> {
        let mut used = prefix;
        let mut first = true;
        for word in paragraph.split_whitespace() {
            let len = word.chars().count();
            if used.saturating_add(1 + len) >= width && (!first || prefix != 1) {
                self.push(ctx, call, "\n")?;
                if indent > self.bound.saturating_sub(self.text.len()) {
                    return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
                }
                self.text.extend(std::iter::repeat_n(' ', indent));
                used = indent;
            } else {
                self.push(ctx, call, " ")?;
                used += 1;
            }
            self.push(ctx, call, word)?;
            used = used.saturating_add(len);
            first = false;
        }
        self.push(ctx, call, "\n")
    }
}

fn number(ctx: &mut Ctx, call: &Call, value: SexpId, bound: usize) -> EvalResult<usize> {
    let value = ctx.session_mut().coerce(value, SexpType::Real)?.0;
    let num = ctx.session().real(value)?.iter().next().unwrap_or(f64::NAN);
    if !num.is_finite() || num < 0.0 || num > bound as f64 {
        return Err(ctx.stop(call.err_call(), "invalid DCF indentation or width"));
    }
    Ok(num as usize)
}

fn write_output(
    ctx: &mut Ctx,
    call: &Call,
    dest: SexpId,
    append: bool,
    output: String,
) -> EvalResult {
    let path = ctx.session().type_of(dest)? == SexpType::Str;
    if path && ctx.session().str_slice(dest)? == [CharId::BLANK] {
        ctx.host()
            .write_out(output.as_bytes())
            .map_err(|err| ctx.stop(call.err_call(), err.to_string()))?;
        return Ok(SEXP_NIL);
    }
    let s = ctx.session_mut();
    let chars = s.mk_char(output.as_bytes(), Encoding::Utf8)?;
    let payload = s.alloc_str_from(&[chars])?;
    ctx.protect(payload);
    let separator = ctx.session_mut().scalar_str("")?;
    ctx.protect(separator);
    let con = if path && append {
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
        &[payload, con, separator, TRUE_SCALAR],
    );
    if path && append {
        let closed = base_call(ctx, call, "close", &[con]);
        result.and(closed)
    } else {
        result
    }
}

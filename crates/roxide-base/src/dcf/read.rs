use std::collections::{HashMap, HashSet};
use std::io::{self, Read};

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{CharId, Encoding, NA_INTEGER, SEXP_NIL, ValueApi};

use super::{flag, keys, set};
use crate::io_os::connection::{self, Access};

type Record = HashMap<usize, Vec<CharId>>;

struct Document {
    fields: Vec<CharId>,
    columns: HashMap<Vec<u8>, usize>,
    records: Vec<Record>,
    repeated: HashSet<usize>,
}

pub(super) fn read_dcf(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let all = flag(ctx, call, args.get(2))?;
    let fields = keys(ctx, args.get(1))?;
    let keep: HashSet<_> = keys(ctx, args.get(3))?.into_iter().flatten().collect();
    let bytes = connection::read(ctx, call, args.get(0), Access::Lines, |source, bound| {
        let mut bytes = Vec::new();
        source
            .take((bound as u64).saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() > bound {
            return Err(io::Error::other("vector size limit exceeded"));
        }
        Ok(bytes)
    })?;
    let input = if all {
        bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes)
    } else {
        &bytes
    };
    let document = Document::parse(ctx, call, input, &keep, all)?;
    document.finish(ctx, call, fields, args.get(1) != SEXP_NIL, all)
}

impl Document {
    fn parse(
        ctx: &mut Ctx,
        call: &Call,
        source: &[u8],
        keep: &HashSet<Vec<u8>>,
        all: bool,
    ) -> EvalResult<Self> {
        let mut doc = Self {
            fields: Vec::new(),
            columns: HashMap::new(),
            records: Vec::new(),
            repeated: HashSet::new(),
        };
        let mut row = Record::new();
        let mut field: Option<Field> = None;
        let bound = ctx.host().limits().max_vector_bytes;
        let mut storage = 0usize;
        let mut input = source;
        while !input.is_empty() {
            let end = input
                .iter()
                .position(|&byte| matches!(byte, b'\r' | b'\n'))
                .unwrap_or(input.len());
            let (line, rest) = input.split_at(end);
            input = rest
                .strip_prefix(b"\r\n")
                .or_else(|| rest.get(1..))
                .unwrap_or_default();
            let line = line.split(|&b| b == 0).next().unwrap_or_default();
            if line.starts_with(b"#") {
                continue;
            }
            let content = trim(line);
            let blank = content.is_empty();
            if !blank && line.first().is_some_and(|&b| space(b)) {
                let Some(current) = &mut field else {
                    return Err(malformed(ctx, call, line, all, true));
                };
                current.continue_with(line, content);
                continue;
            }
            if let Some(field) = field.take() {
                let (column, value) = field.finish(ctx)?;
                let values = row.entry(column).or_default();
                if !values.is_empty() {
                    doc.repeated.insert(column);
                    if !all {
                        values.clear();
                    }
                }
                values.push(value);
            }
            if blank {
                if !row.is_empty() {
                    doc.records.push(std::mem::take(&mut row));
                }
                continue;
            }
            let Some(colon) = line.iter().position(|&b| b == b':').filter(|&p| p > 0) else {
                return Err(malformed(ctx, call, line, all, false));
            };
            storage = storage.saturating_add(2 * size_of::<Vec<CharId>>());
            if storage > bound {
                return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
            }
            let name = &line[..colon];
            let column = if let Some(&column) = doc.columns.get(name) {
                column
            } else {
                let column = doc.fields.len();
                doc.fields
                    .push(ctx.session_mut().mk_char(name, Encoding::Native)?);
                doc.columns.insert(name.to_vec(), column);
                column
            };
            let preserve = keep.contains(name);
            let value = &line[colon + 1..];
            let value = if preserve {
                trim_start(value)
            } else {
                trim(value)
            };
            field = Some(Field {
                column,
                preserve,
                value: value.to_vec(),
            });
        }
        if let Some(field) = field {
            let (column, value) = field.finish(ctx)?;
            let values = row.entry(column).or_default();
            if !values.is_empty() {
                doc.repeated.insert(column);
                if !all {
                    values.clear();
                }
            }
            values.push(value);
        }
        if !row.is_empty() {
            doc.records.push(row);
        }
        Ok(doc)
    }

    fn finish(
        &self,
        ctx: &mut Ctx,
        call: &Call,
        requested: Vec<Option<Vec<u8>>>,
        supplied: bool,
        all: bool,
    ) -> EvalResult {
        let rows = self.records.len();
        if all && rows == 0 {
            return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
        }
        let mut names = Vec::new();
        let mut selected = Vec::new();
        if supplied && (all || !requested.is_empty()) {
            let mut seen = HashSet::new();
            for key in requested {
                let column = key
                    .as_ref()
                    .and_then(|name| self.columns.get(name))
                    .copied();
                if all && column.is_none() {
                    return Err(ctx.stop(call.err_call(), "undefined columns selected"));
                }
                selected.push(column.filter(|column| all || seen.insert(*column)));
                names.push(match key {
                    Some(key) => ctx.session_mut().mk_char(&key, Encoding::Native)?,
                    None => CharId::NA,
                });
            }
        } else {
            names.clone_from(&self.fields);
            selected.extend((0..names.len()).map(Some));
        }
        let width = selected.len();
        let bound = ctx.host().limits().max_vector_bytes;
        let rows_i32 =
            i32::try_from(rows).map_err(|_| ctx.stop(call.err_call(), "too many DCF records"))?;
        let width_i32 =
            i32::try_from(width).map_err(|_| ctx.stop(call.err_call(), "too many DCF fields"))?;
        if rows
            .saturating_mul(width)
            .saturating_mul(size_of::<roxide_gc::SexpId>())
            > bound
        {
            return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
        }

        let names = ctx.session_mut().alloc_str_from(&names)?;
        ctx.protect(names);
        if !all {
            let data: Vec<_> = selected
                .iter()
                .flat_map(|&column| self.records.iter().map(move |row| cell(row, column)[0]))
                .collect();
            let s = ctx.session_mut();
            let matrix = s.alloc_str_from(&data)?;
            let dim = s.alloc_int_from(&[rows_i32, width_i32])?;
            set(s, matrix, "dim", dim)?;
            let dimnames = s.alloc_list(2)?;
            if width != 0 {
                s.list_set(dimnames, 1, names)?;
            }
            set(s, matrix, "dimnames", dimnames)?;
            return Ok(matrix);
        }

        let s = ctx.session_mut();
        let frame = s.alloc_list(width)?;
        for (i, column) in selected.into_iter().enumerate() {
            let list = column.is_some_and(|column| self.repeated.contains(&column));
            let values = if list {
                let out = s.alloc_list(rows)?;
                for (j, row) in self.records.iter().enumerate() {
                    let value = s.alloc_str_from(cell(row, column))?;
                    s.list_set(out, j, value)?;
                }
                out
            } else {
                let data: Vec<_> = self
                    .records
                    .iter()
                    .map(|row| cell(row, column)[0])
                    .collect();
                s.alloc_str_from(&data)?
            };
            s.list_set(frame, i, values)?;
        }
        set(s, frame, "names", names)?;
        let row_names = s.alloc_int_from(&[NA_INTEGER, -rows_i32])?;
        set(s, frame, "row.names", row_names)?;
        let class = s.scalar_str("data.frame")?;
        set(s, frame, "class", class)?;
        Ok(frame)
    }
}

fn cell(row: &Record, column: Option<usize>) -> &[CharId] {
    column
        .and_then(|column| row.get(&column))
        .map_or(&[CharId::NA], Vec::as_slice)
}

struct Field {
    column: usize,
    preserve: bool,
    value: Vec<u8>,
}

impl Field {
    fn continue_with(&mut self, line: &[u8], trimmed: &[u8]) {
        self.value.push(b'\n');
        if trimmed != b"." {
            self.value
                .extend_from_slice(if self.preserve { line } else { trimmed });
        }
    }

    fn finish(mut self, ctx: &mut Ctx) -> EvalResult<(usize, CharId)> {
        if !self.preserve {
            self.value = self.value.trim_ascii().to_vec();
        }
        Ok((
            self.column,
            ctx.session_mut().mk_char(&self.value, Encoding::Native)?,
        ))
    }
}

fn space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

fn trim_start(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|&byte| !space(byte))
        .unwrap_or(bytes.len());
    &bytes[start..]
}

fn trim(bytes: &[u8]) -> &[u8] {
    let head = trim_start(bytes);
    let end = head
        .iter()
        .rposition(|&byte| !space(byte))
        .map_or(0, |i| i + 1);
    &head[..end]
}

fn malformed(
    ctx: &mut Ctx,
    call: &Call,
    line: &[u8],
    all: bool,
    orphan: bool,
) -> roxide_eval::Unwind {
    let text = String::from_utf8_lossy(line);
    let text: String = text.chars().take(if all { 40 } else { 20 }).collect();
    let message = match (all, orphan) {
        (false, false) => format!("Line starting '{text} ...' is malformed!"),
        (false, true) => {
            format!("Found continuation line starting '{text} ...' at begin of record.")
        }
        (true, false) => format!(
            "Invalid DCF format.\nRegular lines must have a tag.\nOffending lines start with:\n  {text}"
        ),
        (true, true) => {
            String::from("Invalid DCF format.\nContinuation lines must not start a record.")
        }
    };
    ctx.stop(call.err_call(), message)
}

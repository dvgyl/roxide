//! Match positions and slicing for R match objects.

use std::ops::Range;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, NA_INTEGER, RLogical, RSession, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi,
};

use super::{Mode, Options, char_output, compile, input, input_bytes, next_unit, single_pattern};
use crate::ops::common::{
    attr, copy_attrs, error, formals, list, names, set, set_names, string, vector_limit,
};
use crate::strings::character;

use super::engine::Match;

pub(in crate::strings) fn regexpr(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    locate(ctx, call, args, false)
}

pub(in crate::strings) fn gregexpr(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    locate(ctx, call, args, true)
}

#[derive(Clone, Copy)]
enum Units {
    Unicode,
    Ascii,
    Bytes,
}

fn locate(ctx: &mut Ctx, call: &Call, args: &Args, all: bool) -> EvalResult {
    let [pattern, x, ignore, perl, fixed, bytes] = formals(
        ctx.session(),
        args,
        [
            "pattern",
            "text",
            "ignore.case",
            "perl",
            "fixed",
            "useBytes",
        ],
    );
    let options = Options::read(ctx, call, ignore, perl, fixed, bytes)?;
    let pattern = single_pattern(ctx, call, pattern, "pattern")?;
    if pattern == CharId::NA {
        return Err(error(ctx, call, "invalid 'pattern' argument"));
    }
    let ids = character(ctx.session_mut(), x)?;
    for &id in &ids {
        ctx.protect(id.as_sexp());
    }
    let options = options
        .with_byte_marks(ctx.session(), &[pattern])?
        .with_byte_marks(ctx.session(), &ids)?;
    let s = ctx.session();
    let mut units = if options.bytes {
        Units::Bytes
    } else {
        Units::Ascii
    };
    if !options.bytes {
        for id in std::iter::once(pattern).chain(ids.iter().copied()) {
            if !s.char_bytes(id)?.is_ascii() {
                units = Units::Unicode;
                break;
            }
        }
    }
    let matcher = compile(ctx, call, pattern, options)?;
    let capture_names = if options.mode == Mode::Perl {
        matcher.capture_names()
    } else {
        Vec::new()
    };
    let mut captures = Captures::new(&capture_names);
    let mut starts = Vec::new();
    let mut lengths = Vec::new();
    let mut objects = Vec::new();
    let limit =
        ctx.host().limits().max_vector_bytes / (2 * size_of::<i32>() * (capture_names.len() + 1));
    let mut total = 0;
    for (i, id) in ids.into_iter().enumerate() {
        let text = if id == CharId::NA {
            None
        } else {
            input_bytes(ctx, call, id, options, i)?
        };
        if let Some(text) = &text {
            let mut offset = 0;
            let row_start = starts.len();
            let mut char_count = 0;
            let mut last = 0;
            if !all || !text.is_empty() || options.mode != Mode::Posix {
                while let Some(found) = matcher
                    .find(text, offset, !capture_names.is_empty())
                    .map_err(|msg| error(ctx, call, msg))?
                {
                    if total >= limit {
                        return Err(error(ctx, call, "vector size limit exceeded"));
                    }
                    total += 1;
                    let len = measure(&text[found.start..found.end], units);
                    char_count += measure(&text[last..found.start], units);
                    starts.push((char_count + 1) as i32);
                    lengths.push(len as i32);
                    captures.add_match(&found, text, char_count, units);
                    char_count += len;
                    last = found.end;
                    offset = if found.start == found.end {
                        next_unit(text, found.end, options.bytes)
                    } else {
                        found.end
                    };
                    if !all || offset >= text.len() {
                        break;
                    }
                }
            }
            if starts.len() == row_start {
                starts.push(-1);
                lengths.push(-1);
                captures.add_default(-1);
                total += 1;
            }
        } else {
            starts.push(NA_INTEGER);
            lengths.push(NA_INTEGER);
            captures.add_default(NA_INTEGER);
            total += 1;
        }
        if total > limit {
            return Err(error(ctx, call, "vector size limit exceeded"));
        }
        if all {
            let mark = text.is_some().then_some(units);
            let s = ctx.session_mut();
            let obj = match_object(s, &starts, &lengths, mark)?;
            if mark.is_some() {
                captures.attach(s, obj, starts.len())?;
            }
            ctx.protect(obj);
            objects.push(obj);
            starts.clear();
            lengths.clear();
            captures.clear();
        }
    }
    let s = ctx.session_mut();
    if all {
        Ok(list(s, &objects)?)
    } else {
        let obj = match_object(s, &starts, &lengths, Some(units))?;
        captures.attach(s, obj, starts.len())?;
        Ok(obj)
    }
}

struct Captures<'a> {
    names: &'a [&'a str],
    starts: Vec<Vec<i32>>,
    lengths: Vec<Vec<i32>>,
}

impl<'a> Captures<'a> {
    fn new(names: &'a [&'a str]) -> Self {
        Self {
            names,
            starts: vec![Vec::new(); names.len()],
            lengths: vec![Vec::new(); names.len()],
        }
    }

    fn add_match(&mut self, found: &Match, text: &[u8], start: usize, units: Units) {
        for (i, (starts, lengths)) in self.starts.iter_mut().zip(&mut self.lengths).enumerate() {
            let (start, len) =
                found
                    .groups
                    .get(i + 1)
                    .copied()
                    .flatten()
                    .map_or((0, 0), |(a, b)| {
                        let pos = if a < found.start {
                            start - measure(&text[a..found.start], units)
                        } else {
                            start + measure(&text[found.start..a], units)
                        };
                        (pos + 1, measure(&text[a..b], units))
                    });
            starts.push(start as i32);
            lengths.push(len as i32);
        }
    }

    fn add_default(&mut self, value: i32) {
        for column in self.starts.iter_mut().chain(self.lengths.iter_mut()) {
            column.push(value);
        }
    }

    fn attach(&self, s: &mut RSession, result: SexpId, count: usize) -> EvalResult<()> {
        if self.names.is_empty() {
            return Ok(());
        }
        let names = self
            .names
            .iter()
            .map(|n| s.mk_char(n.as_bytes(), Encoding::Utf8))
            .collect::<Result<Vec<_>, _>>()?;
        let names = s.alloc_str_from(&names)?;
        let dim = s.alloc_int_from(&[count as i32, self.names.len() as i32])?;
        let dimnames = list(s, &[SEXP_NIL, names])?;
        for (key, columns) in [
            ("capture.start", &self.starts),
            ("capture.length", &self.lengths),
        ] {
            let values: Vec<_> = columns.iter().flatten().copied().collect();
            let values = s.alloc_int_from(&values)?;
            set(s, values, "dim", dim)?;
            set(s, values, "dimnames", dimnames)?;
            set(s, result, key, values)?;
        }
        set(s, result, "capture.names", names)?;
        Ok(())
    }

    fn clear(&mut self) {
        for column in self.starts.iter_mut().chain(self.lengths.iter_mut()) {
            column.clear();
        }
    }
}

fn measure(text: &[u8], units: Units) -> usize {
    match units {
        Units::Unicode => text.iter().filter(|&&b| b & 0xc0 != 0x80).count(),
        Units::Bytes | Units::Ascii => text.len(),
    }
}

fn match_object(
    s: &mut RSession,
    starts: &[i32],
    lengths: &[i32],
    units: Option<Units>,
) -> EvalResult {
    let result = s.alloc_int_from(starts)?;
    let lengths = s.alloc_int_from(lengths)?;
    set(s, result, "match.length", lengths)?;
    let index = match units {
        Some(Units::Ascii) => "chars",
        Some(Units::Bytes) => "bytes",
        _ => return Ok(result),
    };
    let name = s.scalar_str(index)?;
    let enabled = s.scalar_lgl(RLogical::TRUE)?;
    set(s, result, "index.type", name)?;
    set(s, result, "useBytes", enabled)?;
    Ok(result)
}

pub(in crate::strings) fn regmatches(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, m, invert] = formals(ctx.session(), args, ["x", "m", "invert"]);
    let s = ctx.session_mut();
    let is_list = s.type_of(m)? == SexpType::Vec;
    let invert = if invert == SEXP_MISSING {
        Some(false)
    } else {
        let (value, _) = s.coerce(invert, SexpType::Lgl)?;
        s.lgl(value)?.iter().next().and_then(RLogical::get)
    };
    let chars = character(s, x)?;
    if chars.len() != s.xlength(m)? {
        return Err(error(ctx, call, "‘x’ and ‘m’ must have the same length"));
    }
    let rows = if is_list {
        s.list_slice(m)?.to_vec()
    } else {
        Vec::new()
    };
    let s = ctx.session_mut();
    let mut vectors = Vec::new();
    let mut values = Vec::new();
    let mut selected = Vec::new();
    for (i, id) in chars.into_iter().enumerate() {
        let object = if is_list { rows[i] } else { m };
        let read = match get_ranges(s, object, (!is_list).then_some(i)) {
            Ok(read) => read,
            Err(()) => return Err(error(ctx, call, "invalid substring arguments")),
        };
        let bytes = string(s, attr(s, object, "index.type")).as_deref() == Some("bytes");
        let options = Options {
            mode: Mode::Fixed,
            ignore_case: false,
            bytes,
        };
        let mut row = Vec::new();
        if invert != Some(false) && (id == CharId::NA || read.missing) {
            row.push(CharId::NA);
        } else if id != CharId::NA {
            let text = input(s, id, options)?.into_owned();
            let positions = if bytes || text.is_ascii() {
                None
            } else {
                let mut units: Vec<_> = text
                    .iter()
                    .enumerate()
                    .filter_map(|(i, b)| (b & 0xc0 != 0x80).then_some(i))
                    .collect();
                units.push(text.len());
                Some(units)
            };
            let to_byte = |at: usize| match &positions {
                Some(units) => units.get(at).copied().unwrap_or(text.len()),
                None => at.min(text.len()),
            };
            let mut end = 0;
            for r in read.ranges {
                let first = to_byte(r.start);
                let last = to_byte(r.end);
                if invert != Some(false) {
                    if first < end {
                        return Err(error(
                            ctx,
                            call,
                            format!(
                                "need non-overlapping matches for ‘invert = TRUE’ or ‘invert = NA’, but matches for element {} overlap",
                                i + 1
                            ),
                        ));
                    }
                    row.push(excerpt(s, id, &text[end..first], options)?);
                }
                if invert != Some(true) {
                    row.push(excerpt(s, id, &text[first..last], options)?);
                }
                end = last;
            }
            if invert != Some(false) {
                row.push(excerpt(s, id, &text[end..], options)?);
            }
        }
        if is_list || invert != Some(false) {
            vectors.push(s.alloc_str_from(&row)?);
        } else if let Some(value) = row.first() {
            values.push(*value);
            selected.push(i);
        }
    }
    vector_limit(
        ctx,
        call,
        vectors.len().max(values.len()),
        size_of::<SexpId>(),
    )?;
    let s = ctx.session_mut();
    if is_list || invert != Some(false) {
        let out = list(s, &vectors)?;
        copy_attrs(s, x, out, |n| n == "names")?;
        Ok(out)
    } else {
        let out = s.alloc_str_from(&values)?;
        let labels = names(s, x);
        if !labels.is_empty() {
            let labels: Vec<_> = selected
                .iter()
                .map(|&i| labels.get(i).copied().unwrap_or(CharId::NA))
                .collect();
            set_names(s, out, &labels)?;
        }
        Ok(out)
    }
}

fn excerpt(s: &mut RSession, id: CharId, bytes: &[u8], options: Options) -> EvalResult<CharId> {
    if options.bytes && !bytes.is_ascii() {
        return Ok(s.mk_char(bytes, Encoding::Bytes)?);
    }
    char_output(s, id, bytes, options)
}

struct Ranges {
    ranges: Vec<Range<usize>>,
    missing: bool,
}

fn get_ranges(s: &RSession, m: SexpId, at: Option<usize>) -> Result<Ranges, ()> {
    let starts = s.int(m).map_err(|_| ())?;
    let lengths = attr(s, m, "match.length");
    if starts.is_empty() && lengths == SEXP_NIL {
        return Ok(Ranges {
            ranges: Vec::new(),
            missing: false,
        });
    }
    let lengths = s.int(lengths).map_err(|_| ())?;
    if starts.len() != lengths.len() {
        return Err(());
    }
    let indices = at.map_or(0..starts.len(), |i| i..i + 1);
    let mut ranges = Vec::new();
    let mut missing = false;
    for i in indices {
        let start = starts.get(i);
        let len = lengths.get(i);
        match (start, len) {
            (NA_INTEGER, _) => missing = true,
            (-1, -1) => (),
            (start, len) if start > 0 && len >= 0 => {
                let start = (start - 1) as usize;
                ranges.push(start..start + len as usize);
            }
            _ => return Err(()),
        }
    }
    Ok(Ranges { ranges, missing })
}

use std::io::{self, BufRead};

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{Encoding, NA_INTEGER, SEXP_NIL, SexpType, ValueApi};

use super::connection::Access;
use super::{
    ByteEncoding, bytes, check_limit, connection, flag, integer, integers, strings, text,
    translated_bytes,
};

pub(super) fn read_lines(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let source = args.get(0);
    let n = integer(ctx, args.get(1))?;
    let ok = flag(ctx, call, args.get(2), "ok")?;
    let warn = flag(ctx, call, args.get(3), "warn")?;
    let encoding = match text(ctx, call, args.get(4), "encoding")?.as_str() {
        "UTF-8" => Encoding::Utf8,
        "latin1" => Encoding::Latin1,
        "bytes" => Encoding::Bytes,
        _ => Encoding::Native,
    };
    let skip_nul = flag(ctx, call, args.get(5), "skipNul")?;
    if n == NA_INTEGER {
        return Err(ctx.stop(call.err_call(), "invalid 'n' argument"));
    }
    let count = if n < 0 { usize::MAX } else { n as usize };
    let (lines, nul_lines, incomplete) =
        connection::read(ctx, call, source, Access::Lines, |input, limit| {
            let mut lines = Vec::new();
            let mut nul_lines = Vec::new();
            let mut incomplete = false;
            let mut remaining = limit;
            while lines.len() < count {
                let Some((mut line, terminated)) = read_line(input, remaining)? else {
                    break;
                };
                remaining = remaining
                    .checked_sub(line.len() + size_of::<Vec<u8>>())
                    .ok_or_else(limit_error)?;
                if skip_nul {
                    line.retain(|&b| b != 0);
                } else if let Some(pos) = line.iter().position(|&b| b == 0) {
                    if warn {
                        nul_lines.push(lines.len() + 1);
                    }
                    line.truncate(pos);
                }
                if lines.is_empty() && line.starts_with(b"\xef\xbb\xbf") {
                    line.drain(..3);
                }
                lines.push(line);
                incomplete = !terminated;
            }
            Ok((lines, nul_lines, incomplete))
        })?;
    if !ok && lines.len() < count && n >= 0 {
        return Err(ctx.stop(call.err_call(), "too few lines read in readLines"));
    }
    if warn && (!nul_lines.is_empty() || incomplete) {
        let label = connection::label(ctx, call, source)?;
        for line in nul_lines {
            ctx.warn(
                call.err_call(),
                format!("line {line} appears to contain an embedded nul"),
            );
        }
        if incomplete {
            ctx.warn_now(
                call.err_call(),
                format!("incomplete final line found on '{label}'"),
            )?;
        }
    }
    strings(ctx, lines.into_iter().map(Some), encoding)
}

fn limit_error() -> io::Error {
    io::Error::other("I/O result exceeds the vector size limit")
}

fn read_line(input: &mut dyn BufRead, limit: usize) -> io::Result<Option<(Vec<u8>, bool)>> {
    let mut line = Vec::new();
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return Ok((!line.is_empty()).then_some((line, false)));
        }
        let end = available.iter().position(|&b| matches!(b, b'\r' | b'\n'));
        let len = end.unwrap_or(available.len());
        if len > limit.saturating_sub(line.len()) {
            return Err(limit_error());
        }
        line.extend_from_slice(&available[..len]);
        if let Some(end) = end {
            let cr = available[end] == b'\r';
            input.consume(end + 1);
            if cr && input.fill_buf()?.first() == Some(&b'\n') {
                input.consume(1);
            }
            return Ok(Some((line, true)));
        }
        input.consume(len);
    }
}

pub(super) fn write_lines(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let use_bytes = flag(ctx, call, args.get(3), "useBytes")?;
    let encoding = if use_bytes {
        ByteEncoding::Bytes
    } else {
        ByteEncoding::native(ctx)
    };
    if ctx.session().type_of(args.get(0))? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "invalid 'text' argument"));
    }
    let values = translated_bytes(ctx, call, args.get(0), encoding)?;
    if ctx.session().type_of(args.get(2))? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "invalid 'sep' argument"));
    }
    let separator = translated_bytes(ctx, call, args.get(2), encoding)?;
    let [Some(sep)] = separator.as_slice() else {
        return Err(ctx.stop(call.err_call(), "invalid 'sep' argument"));
    };
    let mut buffer = Vec::new();
    for value in values {
        let value = value.as_deref().unwrap_or(b"NA");
        check_limit(
            ctx,
            call,
            buffer
                .len()
                .saturating_add(value.len())
                .saturating_add(sep.len()),
        )?;
        buffer.extend_from_slice(value);
        buffer.extend_from_slice(sep);
    }
    connection::write(ctx, call, args.get(1), Access::Lines, &buffer)?;
    Ok(SEXP_NIL)
}

pub(super) fn read_char(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let widths = integers(ctx, args.get(1))?;
    let use_bytes = flag(ctx, call, args.get(2), "useBytes")? || !ctx.host().locale().utf8;
    if widths.iter().any(|&n| n < 0) {
        return Err(ctx.stop(call.err_call(), "invalid 'nchars' argument"));
    }
    let mut nul = false;
    let values = connection::read(
        ctx,
        call,
        args.get(0),
        Access::Characters,
        |input, limit| {
            let mut values = Vec::new();
            let mut remaining = limit;
            for width in widths {
                let mut value = Vec::new();
                for _ in 0..width {
                    if !read_unit(input, use_bytes, &mut value, remaining)? {
                        break;
                    }
                }
                if width > 0 && value.is_empty() {
                    break;
                }
                remaining = remaining
                    .checked_sub(value.len() + size_of::<Vec<u8>>())
                    .ok_or_else(limit_error)?;
                if let Some(pos) = value.iter().position(|&b| b == 0) {
                    nul |= value[pos + 1..].iter().any(|&b| b != 0);
                    value.truncate(pos);
                }
                values.push(value);
            }
            Ok(values)
        },
    )?;
    if nul {
        ctx.warn(call.err_call(), "truncating string with embedded nuls");
    }
    strings(ctx, values.into_iter().map(Some), Encoding::Native)
}

fn read_unit(
    input: &mut dyn BufRead,
    use_bytes: bool,
    data: &mut Vec<u8>,
    limit: usize,
) -> io::Result<bool> {
    let Some(&first) = input.fill_buf()?.first() else {
        return Ok(false);
    };
    let width = if use_bytes { 1 } else { utf8_width(first) };
    if width > limit.saturating_sub(data.len()) {
        return Err(limit_error());
    }
    let mut unit = [0; 4];
    input.read_exact(&mut unit[..width])?;
    if !use_bytes && std::str::from_utf8(&unit[..width]).is_err() {
        return Err(io::Error::other("invalid UTF-8 input in readChar()"));
    }
    data.extend_from_slice(&unit[..width]);
    Ok(true)
}

fn utf8_width(first: u8) -> usize {
    match first {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 1,
    }
}

pub(super) fn write_char(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if ctx.session().type_of(args.get(0))? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "invalid 'object' argument"));
    }
    let values = bytes(ctx, call, args.get(0))?;
    let widths = if args.get(2) == SEXP_NIL {
        None
    } else {
        Some(integers(ctx, args.get(2))?)
    };
    if widths
        .as_ref()
        .is_some_and(|widths| widths.iter().any(|&n| n < 0))
        || (widths.is_none() && values.iter().any(Option::is_none))
    {
        return Err(ctx.stop(call.err_call(), "invalid 'nchars' argument"));
    }
    let eos = if args.get(3) == SEXP_NIL {
        None
    } else {
        Some(text(ctx, call, args.get(3), "eos")?)
    };
    let use_bytes = flag(ctx, call, args.get(4), "useBytes")? || !ctx.host().locale().utf8;
    let count = widths.as_ref().map_or(values.len(), Vec::len);
    if count > values.len() {
        return Err(ctx.stop(call.err_call(), "'object' is too short"));
    }
    let mut buffer = Vec::new();
    let mut padded = false;
    for (i, value) in values.into_iter().take(count).enumerate() {
        let value = value.as_deref().unwrap_or(b"NA");
        let requested = widths.as_ref().map(|widths| widths[i] as usize);
        let (len, used) = prefix(ctx, call, value, requested, use_bytes)?;
        let padding = requested.unwrap_or(used) - used;
        let extra = eos.as_ref().map_or(0, |end| end.len() + 1);
        check_limit(
            ctx,
            call,
            buffer
                .len()
                .saturating_add(len)
                .saturating_add(padding)
                .saturating_add(extra),
        )?;
        buffer.extend_from_slice(&value[..len]);
        buffer.resize(buffer.len() + padding, 0);
        padded |= padding > 0;
        if let Some(end) = &eos {
            buffer.extend_from_slice(end.as_bytes());
            buffer.push(0);
        }
    }
    if padded {
        ctx.warn(
            call.err_call(),
            "writeChar: more characters requested than are in the string - will zero-pad",
        );
    }
    let dest = args.get(1);
    if ctx.session().type_of(dest)? == SexpType::Raw {
        ctx.set_visible(true);
        return Ok(ctx.session_mut().alloc_raw_from(&buffer)?);
    }
    ctx.set_visible(false);
    connection::write(ctx, call, dest, Access::Characters, &buffer)?;
    Ok(SEXP_NIL)
}

fn prefix(
    ctx: &mut Ctx,
    call: &Call,
    value: &[u8],
    requested: Option<usize>,
    use_bytes: bool,
) -> EvalResult<(usize, usize)> {
    if use_bytes {
        let count = requested.unwrap_or(value.len()).min(value.len());
        return Ok((count, count));
    }
    let value = std::str::from_utf8(value)
        .map_err(|_| ctx.stop(call.err_call(), "invalid UTF-8 input in writeChar()"))?;
    let mut len = 0;
    let mut count = 0;
    for ch in value.chars().take(requested.unwrap_or(usize::MAX)) {
        len += ch.len_utf8();
        count += 1;
    }
    Ok((len, count))
}

//! Character vectors, encodings, and raw bytes.

use std::collections::HashMap;
use std::sync::LazyLock;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, NA_INTEGER, RLogical, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi,
};

use super::{character, recycle, text_attrs, utf8};
use crate::ops::attributes::has_class;
use crate::ops::common::{copy_attrs, error, flag, formals, tag, vector_limit};

pub(super) fn paste(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    paste_impl(ctx, call, args, false)
}

pub(super) fn paste0(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    paste_impl(ctx, call, args, true)
}

fn paste_impl(ctx: &mut Ctx, call: &Call, args: &Args, zero: bool) -> EvalResult {
    let sep = if zero {
        CharId::BLANK
    } else {
        let space = ctx.session_mut().mk_char(b" ", Encoding::Native)?;
        let sep = paste_separator(ctx, call, args, "sep", space)?;
        if ctx.session().char_encoding(sep)? == Encoding::Bytes {
            return Err(error(
                ctx,
                call,
                "translating strings with \"bytes\" encoding is not allowed",
            ));
        }
        sep
    };
    let collapse = match args.named(ctx.session(), "collapse") {
        None | Some(SEXP_NIL) => None,
        Some(_) => Some(paste_separator(ctx, call, args, "collapse", CharId::BLANK)?),
    };
    let recycle0 = flag(
        ctx,
        args.named(ctx.session(), "recycle0")
            .unwrap_or(SEXP_MISSING),
        false,
    )?;
    let values: Vec<_> = args
        .iter()
        .filter_map(|(name, value)| match tag(ctx.session(), name) {
            Some("collapse" | "recycle0") => None,
            Some("sep") if !zero => None,
            _ => Some(value),
        })
        .collect();
    let parts = values
        .into_iter()
        .map(|x| character(ctx.session_mut(), x))
        .collect::<EvalResult<Vec<_>>>()?;
    let len = if recycle0 && parts.iter().any(Vec::is_empty) {
        0
    } else {
        parts.iter().map(Vec::len).max().unwrap_or(0)
    };
    vector_limit(ctx, call, len, size_of::<CharId>())?;
    let limit = ctx.host().limits().max_vector_bytes;
    let mut ids = Vec::with_capacity(if collapse.is_some() { 1 } else { len });
    let mut joined = Vec::new();
    let mut total = 0usize;
    let mut joined_encoding = Encoding::Native;
    for i in 0..len {
        let elements = || {
            parts
                .iter()
                .map(|part| recycle(part, i).unwrap_or(CharId::BLANK))
        };
        let s = ctx.session();
        let mut encoding = if parts.len() > 1 {
            merge_encoding(Encoding::Native, s.char_encoding(sep)?)
        } else {
            Encoding::Native
        };
        for id in elements() {
            encoding = merge_encoding(encoding, s.char_encoding(id)?);
        }
        let raw = encoding == Encoding::Bytes;
        let mut row = Vec::new();
        for (j, id) in elements().enumerate() {
            if j != 0 {
                append_char(ctx, call, &mut row, sep, raw, limit)?;
            }
            append_char(ctx, call, &mut row, id, raw, limit)?;
        }
        if let Some(collapse) = collapse {
            if i != 0 {
                let mark = ctx.session().char_encoding(collapse)?;
                joined_encoding = merge_encoding(joined_encoding, mark);
                append_char(
                    ctx,
                    call,
                    &mut joined,
                    collapse,
                    mark == Encoding::Bytes,
                    limit,
                )?;
            }
            append(ctx, call, &mut joined, &row, limit)?;
            joined_encoding = merge_encoding(joined_encoding, encoding);
        } else {
            total = total.saturating_add(row.len());
            vector_limit(ctx, call, total, 1)?;
            ids.push(ctx.session_mut().mk_char(&row, encoding)?);
        }
    }
    let s = ctx.session_mut();
    if collapse.is_some() {
        ids.push(s.mk_char(&joined, joined_encoding)?);
    }
    Ok(s.alloc_str_from(&ids)?)
}

fn paste_separator(
    ctx: &mut Ctx,
    call: &Call,
    args: &Args,
    name: &str,
    default: CharId,
) -> EvalResult<CharId> {
    let s = ctx.session();
    let Some(x) = args.named(s, name) else {
        return Ok(default);
    };
    let id = s.str_slice(x).ok().and_then(|x| x.first()).copied();
    match id {
        Some(id) if id != CharId::NA => Ok(id),
        _ if name == "sep" => Err(error(ctx, call, "invalid separator")),
        _ => Err(error(ctx, call, "invalid 'collapse' argument")),
    }
}

fn merge_encoding(a: Encoding, b: Encoding) -> Encoding {
    match (a, b) {
        (Encoding::Bytes, _) | (_, Encoding::Bytes) => Encoding::Bytes,
        (Encoding::Native, Encoding::Native) => Encoding::Native,
        _ => Encoding::Utf8,
    }
}

fn append_char(
    ctx: &mut Ctx,
    call: &Call,
    out: &mut Vec<u8>,
    id: CharId,
    raw: bool,
    limit: usize,
) -> EvalResult<()> {
    let s = ctx.session();
    let bytes = if raw {
        std::borrow::Cow::Borrowed(s.char_bytes(id)?)
    } else {
        utf8(s, id)?
    };
    if bytes.len() > limit.saturating_sub(out.len()) {
        return Err(error(ctx, call, "vector size limit exceeded"));
    }
    out.extend_from_slice(&bytes);
    Ok(())
}

fn append(
    ctx: &mut Ctx,
    call: &Call,
    out: &mut Vec<u8>,
    bytes: &[u8],
    limit: usize,
) -> EvalResult<()> {
    if bytes.len() > limit.saturating_sub(out.len()) {
        return Err(error(ctx, call, "vector size limit exceeded"));
    }
    out.extend_from_slice(bytes);
    Ok(())
}

#[derive(PartialEq, Eq)]
enum Count {
    Chars,
    Bytes,
    Width,
}

pub(super) fn nchar(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, mode, allow_na, keep_na] =
        formals(ctx.session(), args, ["x", "type", "allowNA", "keepNA"]);
    let mode = if mode == SEXP_MISSING {
        Count::Chars
    } else {
        if ctx.session().xlength(mode)? != 1 {
            return Err(error(ctx, call, "invalid 'type' argument"));
        }
        let mode = crate::ops::common::string(ctx.session(), mode).unwrap_or_default();
        match mode.as_str() {
            "c" | "ch" | "cha" | "char" | "chars" => Count::Chars,
            "b" | "by" | "byt" | "byte" | "bytes" => Count::Bytes,
            "w" | "wi" | "wid" | "widt" | "width" => Count::Width,
            _ => return Err(error(ctx, call, "invalid 'type' argument")),
        }
    };
    if has_class(ctx.session(), x, "factor") {
        return Err(error(ctx, call, "'nchar()' requires a character vector"));
    }
    let allow_na = flag(ctx, allow_na, false)?;
    let keep_na = flag(ctx, keep_na, mode != Count::Width)?;
    let chars = character(ctx.session_mut(), x)?;
    let mut counts = Vec::with_capacity(chars.len());
    for id in chars {
        let s = ctx.session();
        let n = if id == CharId::NA {
            if keep_na { NA_INTEGER } else { 2 }
        } else if mode == Count::Bytes {
            size_to_int(s.char_bytes(id)?.len())
        } else if s.char_encoding(id)? == Encoding::Bytes {
            if !allow_na {
                let unit = if mode == Count::Width {
                    "width"
                } else {
                    "number of characters"
                };
                let i = counts.len() + 1;
                return Err(error(
                    ctx,
                    call,
                    format!("{unit} is not computable in \"bytes\" encoding, element {i}"),
                ));
            }
            NA_INTEGER
        } else {
            let bytes = utf8(s, id)?;
            match std::str::from_utf8(&bytes) {
                Ok(text) if mode == Count::Width => {
                    size_to_int(text.chars().map(display_width).sum())
                }
                Ok(text) => size_to_int(text.chars().count()),
                Err(_) if allow_na => NA_INTEGER,
                Err(_) => {
                    let i = counts.len() + 1;
                    return Err(error(
                        ctx,
                        call,
                        format!("invalid multibyte string, element {i}"),
                    ));
                }
            }
        };
        counts.push(n);
    }
    let s = ctx.session_mut();
    let out = s.alloc_int_from(&counts)?;
    copy_attrs(s, x, out, text_attrs)?;
    Ok(out)
}

fn size_to_int(size: usize) -> i32 {
    i32::try_from(size).unwrap_or(NA_INTEGER)
}

fn display_width(c: char) -> usize {
    static ZERO_WIDTH: LazyLock<Option<regex::Regex>> =
        LazyLock::new(|| regex::Regex::new(r"^[\p{Mn}\p{Me}\p{Cf}]$").ok());
    let mut unit = [0; 4];
    if c == '\u{ad}' {
        return 1;
    }
    if matches!(c, '\u{1160}'..='\u{11ff}')
        || c.is_control()
        || ZERO_WIDTH
            .as_ref()
            .is_some_and(|re| re.is_match(c.encode_utf8(&mut unit)))
    {
        return 0;
    }
    match c {
        '\u{1100}'..='\u{115f}'
        | '\u{231a}'..='\u{231b}'
        | '\u{2329}'..='\u{232a}'
        | '\u{23e9}'..='\u{23ec}'
        | '\u{23f0}'
        | '\u{23f3}'
        | '\u{25fd}'..='\u{25fe}'
        | '\u{2614}'..='\u{2615}'
        | '\u{2648}'..='\u{2653}'
        | '\u{267f}'
        | '\u{2693}'
        | '\u{26a1}'
        | '\u{26aa}'..='\u{26ab}'
        | '\u{26bd}'..='\u{26be}'
        | '\u{26c4}'..='\u{26c5}'
        | '\u{26ce}'
        | '\u{26d4}'
        | '\u{26ea}'
        | '\u{26f2}'..='\u{26f3}'
        | '\u{26f5}'
        | '\u{26fa}'
        | '\u{26fd}'
        | '\u{2705}'
        | '\u{270a}'..='\u{270b}'
        | '\u{2728}'
        | '\u{274c}'
        | '\u{274e}'
        | '\u{2753}'..='\u{2755}'
        | '\u{2757}'
        | '\u{2795}'..='\u{2797}'
        | '\u{27b0}'
        | '\u{27bf}'
        | '\u{2b1b}'..='\u{2b1c}'
        | '\u{2b50}'
        | '\u{2b55}'
        | '\u{2e80}'..='\u{303e}'
        | '\u{3041}'..='\u{a4cf}'
        | '\u{a960}'..='\u{a97c}'
        | '\u{ac00}'..='\u{d7a3}'
        | '\u{f900}'..='\u{faff}'
        | '\u{fe10}'..='\u{fe19}'
        | '\u{fe30}'..='\u{fe6b}'
        | '\u{ff01}'..='\u{ff60}'
        | '\u{ffe0}'..='\u{ffe6}'
        | '\u{16fe0}'..='\u{18dff}'
        | '\u{1b000}'..='\u{1b2ff}'
        | '\u{1f300}'..='\u{1faff}'
        | '\u{20000}'..='\u{3fffd}' => 2,
        _ => 1,
    }
}

pub(super) fn nzchar(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, keep_na] = formals(ctx.session(), args, ["x", "keepNA"]);
    if has_class(ctx.session(), x, "factor") {
        return Err(error(ctx, call, "'nzchar()' requires a character vector"));
    }
    let keep_na = flag(ctx, keep_na, false)?;
    let chars = character(ctx.session_mut(), x)?;
    let s = ctx.session_mut();
    let values = chars
        .iter()
        .map(|&id| {
            if id == CharId::NA && keep_na {
                return Ok(RLogical::NA);
            }
            Ok(RLogical::from_option(Some(!s.char_bytes(id)?.is_empty())))
        })
        .collect::<EvalResult<Vec<_>>>()?;
    Ok(s.alloc_lgl_from(&values)?)
}

pub(super) fn substr(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, first, last] = formals(ctx.session(), args, ["x", "start", "stop"]);
    slice(ctx, call, x, first, last, false)
}

pub(super) fn substring(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, first, last] = formals(ctx.session(), args, ["text", "first", "last"]);
    let last = if last == SEXP_MISSING {
        ctx.session_mut().scalar_int(1_000_000)?
    } else {
        last
    };
    slice(ctx, call, x, first, last, true)
}

fn slice(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    first: SexpId,
    last: SexpId,
    extend: bool,
) -> EvalResult {
    let s = ctx.session_mut();
    let chars = character(s, x)?;
    if chars.is_empty() {
        return Ok(s.alloc_str_from(&[])?);
    }
    let (first, _) = s.coerce(first, SexpType::Int)?;
    let (last, _) = s.coerce(last, SexpType::Int)?;
    let first = s.int(first)?.to_vec();
    let last = s.int(last)?.to_vec();
    if first.is_empty() || last.is_empty() {
        return Err(error(ctx, call, "invalid substring arguments"));
    }
    let len = if extend {
        chars.len().max(first.len()).max(last.len())
    } else {
        chars.len()
    };
    vector_limit(ctx, call, len, size_of::<CharId>())?;
    let mut result = Vec::with_capacity(len);
    let mut total = 0usize;
    for i in 0..len {
        let id = chars[i % chars.len()];
        let a = first[i % first.len()];
        let b = last[i % last.len()];
        if id == CharId::NA || a == NA_INTEGER || b == NA_INTEGER {
            result.push(CharId::NA);
            continue;
        }
        let s = ctx.session();
        let mark = s.char_encoding(id)?;
        let bytes = s.char_bytes(id)?;
        let start = (a.max(1) - 1) as usize;
        let end = b.max(0) as usize;
        let range = if start >= end {
            0..0
        } else if matches!(mark, Encoding::Latin1 | Encoding::Bytes) || bytes.is_ascii() {
            start.min(bytes.len())..end.min(bytes.len())
        } else if let Ok(text) = std::str::from_utf8(bytes) {
            let mut indices = text.char_indices();
            let start_byte = indices.nth(start).map_or(text.len(), |(i, _)| i);
            let end_byte = indices.nth(end - start - 1).map_or(text.len(), |(i, _)| i);
            start_byte..end_byte
        } else {
            return Err(error(
                ctx,
                call,
                format!("invalid multibyte string, element {}", i + 1),
            ));
        };
        if range.len() == bytes.len() {
            result.push(id);
            continue;
        }
        let bytes = bytes[range].to_vec();
        total = total.saturating_add(bytes.len());
        vector_limit(ctx, call, total, 1)?;
        result.push(ctx.session_mut().mk_char(&bytes, mark)?);
    }
    let s = ctx.session_mut();
    let out = s.alloc_str_from(&result)?;
    if chars.len() == len {
        copy_attrs(s, x, out, text_attrs)?;
    }
    Ok(out)
}

pub(super) fn starts_with(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, prefix] = formals(ctx.session(), args, ["x", "prefix"]);
    compare_edge(ctx, call, x, prefix, <[u8]>::starts_with)
}

pub(super) fn ends_with(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, suffix] = formals(ctx.session(), args, ["x", "suffix"]);
    compare_edge(ctx, call, x, suffix, <[u8]>::ends_with)
}

fn compare_edge(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    edge: SexpId,
    check: fn(&[u8], &[u8]) -> bool,
) -> EvalResult {
    let s = ctx.session();
    let (Ok(x), Ok(edge)) = (s.str_slice(x), s.str_slice(edge)) else {
        return Err(error(ctx, call, "non-character object(s)"));
    };
    let mut values = Vec::new();
    if !x.is_empty() && !edge.is_empty() {
        for i in 0..x.len().max(edge.len()) {
            let (a, b) = (x[i % x.len()], edge[i % edge.len()]);
            let v = if a == CharId::NA || b == CharId::NA {
                RLogical::NA
            } else {
                RLogical::from_option(Some(check(&utf8(s, a)?, &utf8(s, b)?)))
            };
            values.push(v);
        }
    }
    Ok(ctx.session_mut().alloc_lgl_from(&values)?)
}

pub(super) fn tolower(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    map_characters(ctx, call, args, |c| c.to_lowercase().next().unwrap_or(c))
}

pub(super) fn toupper(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    map_characters(ctx, call, args, |c| {
        let mut upper = c.to_uppercase();
        match (upper.next(), upper.next()) {
            (Some(upper), None) => upper,
            _ => c,
        }
    })
}

pub(super) fn chartr(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [old, new, x] = formals(ctx.session(), args, ["old", "new", "x"]);
    let old = translation_set(ctx, call, old, "old")?;
    let new = translation_set(ctx, call, new, "new")?;
    if old.len() > new.len() {
        return Err(error(ctx, call, "'old' is longer than 'new'"));
    }
    let map: HashMap<_, _> = old.into_iter().zip(new).collect();
    transform(ctx, call, x, |c| map.get(&c).copied().unwrap_or(c))
}

fn translation_set(ctx: &mut Ctx, call: &Call, x: SexpId, name: &str) -> EvalResult<Vec<char>> {
    let ids = character(ctx.session_mut(), x)?;
    let Some(&id) = ids.first() else {
        return Err(error(ctx, call, format!("invalid '{name}' argument")));
    };
    if id == CharId::NA {
        return Err(error(ctx, call, format!("invalid '{name}' argument")));
    }
    if ids.len() > 1 {
        ctx.warn_now(
            call.err_call(),
            format!("argument '{name}' has length > 1 and only the first element will be used"),
        )?;
    }
    let limit = ctx.host().limits().max_vector_bytes / size_of::<char>();
    let bytes = utf8(ctx.session(), id)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(first) = chars.next() {
        let mut tail = chars.clone();
        if let (Some('-'), Some(last)) = (tail.next(), tail.next()) {
            if first > last {
                let message = format!("decreasing range specification ('{first}-{last}')");
                return Err(error(ctx, call, message));
            }
            let count = (u32::from(last) - u32::from(first)) as usize + 1;
            if count > limit.saturating_sub(out.len()) {
                return Err(error(ctx, call, "vector size limit exceeded"));
            }
            out.extend(first..=last);
            chars = tail;
        } else {
            out.push(first);
        }
    }
    Ok(out)
}

fn map_characters(
    ctx: &mut Ctx,
    call: &Call,
    args: &Args,
    map: impl Fn(char) -> char,
) -> EvalResult {
    let [x] = formals(ctx.session(), args, ["x"]);
    transform(ctx, call, x, map)
}

fn transform(ctx: &mut Ctx, call: &Call, x: SexpId, map: impl Fn(char) -> char) -> EvalResult {
    let ids = character(ctx.session_mut(), x)?;
    let mut out = Vec::with_capacity(ids.len());
    let mut total = 0usize;
    for (i, &id) in ids.iter().enumerate() {
        if id == CharId::NA {
            out.push(id);
            continue;
        }
        let s = ctx.session();
        let mark = s.char_encoding(id)?;
        let bytes = utf8(s, id)?;
        let (bytes, encoding) = if mark == Encoding::Bytes {
            let bytes = bytes
                .iter()
                .map(|&b| {
                    if b.is_ascii() {
                        u8::try_from(map(char::from(b))).unwrap_or(b)
                    } else {
                        b
                    }
                })
                .collect();
            (bytes, mark)
        } else {
            let Ok(text) = std::str::from_utf8(&bytes) else {
                return Err(error(ctx, call, format!("invalid input string {}", i + 1)));
            };
            let mapped: String = text.chars().map(&map).collect();
            if mark == Encoding::Latin1 && mapped.chars().all(|c| u32::from(c) <= 255) {
                (mapped.chars().map(|c| c as u8).collect(), mark)
            } else {
                (mapped.into_bytes(), mark)
            }
        };
        total = total.saturating_add(bytes.len());
        vector_limit(ctx, call, total, 1)?;
        out.push(ctx.session_mut().mk_char(&bytes, encoding)?);
    }
    let s = ctx.session_mut();
    let out = s.alloc_str_from(&out)?;
    copy_attrs(s, x, out, text_attrs)?;
    Ok(out)
}

pub(super) fn encoding(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x] = formals(ctx.session(), args, ["x"]);
    let s = ctx.session_mut();
    let Ok(ids) = s.str_slice(x) else {
        return Err(error(ctx, call, "a character vector argument expected"));
    };
    let labels = ids
        .iter()
        .map(|&id| s.char_encoding(id).map(Encoding::as_r_str))
        .collect::<Result<Vec<_>, _>>()?;
    let ids = labels
        .into_iter()
        .map(|v| s.mk_char(v.as_bytes(), Encoding::Native))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(s.alloc_str_from(&ids)?)
}

pub(super) fn set_encoding(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, value] = formals(ctx.session(), args, ["x", "value"]);
    let s = ctx.session_mut();
    let (Ok(ids), Ok(marks)) = (s.str_slice(x), s.str_slice(value)) else {
        return Err(error(ctx, call, "a character vector argument expected"));
    };
    if marks.is_empty() {
        return Err(error(ctx, call, "'value' must be of positive length"));
    }
    let marks = marks
        .iter()
        .map(|&id| {
            Ok(match s.char_bytes(id)? {
                b"UTF-8" => Encoding::Utf8,
                b"latin1" => Encoding::Latin1,
                b"bytes" => Encoding::Bytes,
                _ => Encoding::Native,
            })
        })
        .collect::<EvalResult<Vec<_>>>()?;
    let ids = ids.to_vec();
    let mut out = Vec::with_capacity(ids.len());
    for (i, id) in ids.into_iter().enumerate() {
        let bytes = s.char_bytes(id)?;
        if id == CharId::NA || bytes.is_ascii() {
            out.push(id);
        } else {
            let bytes = bytes.to_vec();
            out.push(s.mk_char(&bytes, marks[i % marks.len()])?);
        }
    }
    let out = s.alloc_str_from(&out)?;
    copy_attrs(s, x, out, |_| true)?;
    Ok(out)
}

pub(super) fn enc2utf8(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    transcode(ctx, call, args)
}

pub(super) fn enc2native(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    transcode(ctx, call, args)
}

fn transcode(ctx: &mut Ctx, call: &Call, args: &Args) -> EvalResult {
    let [x] = formals(ctx.session(), args, ["x"]);
    let s = ctx.session_mut();
    let Ok(ids) = s.str_slice(x) else {
        return Err(error(ctx, call, "argument is not a character vector"));
    };
    let ids = ids.to_vec();
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let mark = s.char_encoding(id)?;
        if id == CharId::NA || mark == Encoding::Bytes || mark == Encoding::Utf8 {
            out.push(id);
        } else {
            let bytes = utf8(s, id)?.into_owned();
            if std::str::from_utf8(&bytes).is_ok() {
                out.push(s.mk_char(&bytes, Encoding::Utf8)?);
            } else {
                out.push(id);
            }
        }
    }
    let out = s.alloc_str_from(&out)?;
    copy_attrs(s, x, out, |_| true)?;
    Ok(out)
}

pub(super) fn char_to_raw(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x] = formals(ctx.session(), args, ["x"]);
    let ids = ctx.session().str_slice(x).unwrap_or_default();
    let Some(&id) = ids.first() else {
        return Err(error(
            ctx,
            call,
            "argument must be a character vector of length 1",
        ));
    };
    if ids.len() > 1 {
        ctx.warn_now(call.err_call(), "argument should be a character vector of length 1\nall but the first element will be ignored")?;
    }
    let s = ctx.session_mut();
    let bytes = s.char_bytes(id)?.to_vec();
    Ok(s.alloc_raw_from(&bytes)?)
}

pub(super) fn raw_to_char(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, multiple] = formals(ctx.session(), args, ["x", "multiple"]);
    let multiple = flag(ctx, multiple, false)?;
    let s = ctx.session_mut();
    let Ok(bytes) = s.raw(x) else {
        return Err(error(ctx, call, "argument 'x' must be a raw vector"));
    };
    let bytes = bytes.to_vec();
    if multiple {
        vector_limit(ctx, call, bytes.len(), size_of::<CharId>())?;
        let s = ctx.session_mut();
        let ids = bytes
            .into_iter()
            .map(|b| {
                if b == 0 {
                    Ok(CharId::BLANK)
                } else {
                    s.mk_char(&[b], Encoding::Native)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(s.alloc_str_from(&ids)?);
    }
    let len = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    let bytes = &bytes[..len];
    if bytes.contains(&0) {
        let repr = String::from_utf8_lossy(bytes).replace('\0', "\\0");
        return Err(error(
            ctx,
            call,
            format!("embedded nul in string: '{repr}'"),
        ));
    }
    let id = s.mk_char(bytes, Encoding::Native)?;
    Ok(s.alloc_str_from(&[id])?)
}

//! Search, splitting, and substitution with fixed, POSIX, and Perl patterns.

#[path = "pattern_engine.rs"]
mod engine;
#[path = "pattern_positions.rs"]
mod positions;

use std::collections::HashMap;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, RLogical, RSession, SEXP_MISSING, ValueApi};

use super::{character, recycle, utf8};
use crate::ops::common::{
    copy_attrs, error, flag, formals, list, names, set_names, string, vector_limit,
};

use engine::{Match, Matcher, next_unit};
pub(super) use positions::{gregexpr, regexpr, regmatches};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Fixed,
    Posix,
    Perl,
}

#[derive(Clone, Copy)]
struct Options {
    mode: Mode,
    ignore_case: bool,
    bytes: bool,
}

impl Options {
    fn read(
        ctx: &mut Ctx,
        call: &Call,
        ignore: SexpId,
        perl: SexpId,
        fixed: SexpId,
        bytes: SexpId,
    ) -> EvalResult<Self> {
        let ignore_case = flag(ctx, ignore, false)?;
        let perl = flag(ctx, perl, false)?;
        let fixed = flag(ctx, fixed, false)?;
        let bytes = flag(ctx, bytes, false)?;
        let mode = if fixed {
            if ignore_case {
                ctx.warn_now(
                    call.err_call(),
                    "argument 'ignore.case = TRUE' will be ignored",
                )?;
            }
            if perl {
                ctx.warn_now(call.err_call(), "argument 'perl = TRUE' will be ignored")?;
            }
            Mode::Fixed
        } else if perl {
            Mode::Perl
        } else {
            Mode::Posix
        };
        Ok(Self {
            mode,
            ignore_case: ignore_case && !fixed,
            bytes,
        })
    }

    fn with_byte_marks(mut self, s: &RSession, ids: &[CharId]) -> EvalResult<Self> {
        for &id in ids {
            self.bytes |= s.char_encoding(id)? == Encoding::Bytes;
        }
        Ok(self)
    }
}

fn single_pattern(ctx: &mut Ctx, call: &Call, id: SexpId, name: &str) -> EvalResult<CharId> {
    let ids = character(ctx.session_mut(), id)?;
    let Some(&first) = ids.first() else {
        return Err(error(ctx, call, format!("invalid '{name}' argument")));
    };
    if ids.len() > 1 {
        ctx.protect(first.as_sexp());
        ctx.warn_now(
            call.err_call(),
            format!("argument '{name}' has length > 1 and only the first element will be used"),
        )?;
    }
    Ok(first)
}

fn input(s: &RSession, id: CharId, options: Options) -> EvalResult<std::borrow::Cow<'_, [u8]>> {
    if options.bytes {
        Ok(std::borrow::Cow::Borrowed(s.char_bytes(id)?))
    } else {
        Ok(utf8(s, id)?)
    }
}

fn input_bytes(
    ctx: &mut Ctx,
    call: &Call,
    id: CharId,
    options: Options,
    i: usize,
) -> EvalResult<Option<Vec<u8>>> {
    let bytes = input(ctx.session(), id, options)?.into_owned();
    if !options.bytes && std::str::from_utf8(&bytes).is_err() {
        ctx.warn_now(
            call.err_call(),
            format!("input string {} is invalid in this locale", i + 1),
        )?;
        return Ok(None);
    }
    Ok(Some(bytes))
}

fn char_output(s: &mut RSession, id: CharId, bytes: &[u8], options: Options) -> EvalResult<CharId> {
    let mark = s.char_encoding(id)?;
    if options.bytes {
        let encoding = if !bytes.is_ascii() && mark == Encoding::Bytes {
            Encoding::Bytes
        } else {
            Encoding::Native
        };
        Ok(s.mk_char(bytes, encoding)?)
    } else if mark == Encoding::Latin1
        && let Ok(value) = std::str::from_utf8(bytes)
        && value.chars().all(|c| u32::from(c) <= 255)
    {
        let translated: Vec<_> = value.chars().map(|c| c as u8).collect();
        Ok(s.mk_char(&translated, mark)?)
    } else {
        let encoding = if mark == Encoding::Native {
            mark
        } else {
            Encoding::Utf8
        };
        Ok(s.mk_char(bytes, encoding)?)
    }
}

pub(super) fn grep(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    search(ctx, call, args, Some(false))
}

pub(super) fn grepv(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    search(ctx, call, args, Some(true))
}

pub(super) fn grepl(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    search(ctx, call, args, None)
}

fn search(ctx: &mut Ctx, call: &Call, args: &Args, default: Option<bool>) -> EvalResult {
    let (pattern, x, options, value, invert) = if let Some(default) = default {
        let [pattern, x, ignore, perl, value, fixed, bytes, invert] = formals(
            ctx.session(),
            args,
            [
                "pattern",
                "x",
                "ignore.case",
                "perl",
                "value",
                "fixed",
                "useBytes",
                "invert",
            ],
        );
        let options = Options::read(ctx, call, ignore, perl, fixed, bytes)?;
        (
            pattern,
            x,
            options,
            flag(ctx, value, default)?,
            flag(ctx, invert, false)?,
        )
    } else {
        let [pattern, x, ignore, perl, fixed, bytes] = formals(
            ctx.session(),
            args,
            ["pattern", "x", "ignore.case", "perl", "fixed", "useBytes"],
        );
        (
            pattern,
            x,
            Options::read(ctx, call, ignore, perl, fixed, bytes)?,
            false,
            false,
        )
    };
    let pattern = single_pattern(ctx, call, pattern, "pattern")?;
    let ids = character(ctx.session_mut(), x)?;
    for &id in &ids {
        ctx.protect(id.as_sexp());
    }
    if pattern == CharId::NA {
        let s = ctx.session_mut();
        if default.is_none() {
            return Ok(s.alloc_lgl_from(&vec![RLogical::NA; ids.len()])?);
        }
        if value {
            let out = s.alloc_str_from(&vec![CharId::NA; ids.len()])?;
            copy_attrs(s, x, out, |n| n == "names")?;
            return Ok(out);
        }
        return Ok(s.alloc_int_from(&vec![roxide_value::NA_INTEGER; ids.len()])?);
    }
    let options = options
        .with_byte_marks(ctx.session(), &[pattern])?
        .with_byte_marks(ctx.session(), &ids)?;
    let matcher = compile(ctx, call, pattern, options)?;
    let mut selected = Vec::new();
    let mut mask = Vec::new();
    for (i, &id) in ids.iter().enumerate() {
        let mut matched = false;
        if id != CharId::NA
            && let Some(bytes) = input_bytes(ctx, call, id, options, i)?
        {
            matched = matcher
                .exists(&bytes)
                .map_err(|msg| error(ctx, call, msg))?;
        }
        if default.is_none() {
            mask.push(RLogical::from_option(Some(matched)));
        } else if matched != invert {
            selected.push(i);
        }
    }
    let s = ctx.session_mut();
    if default.is_none() {
        Ok(s.alloc_lgl_from(&mask)?)
    } else if value {
        let values: Vec<_> = selected.iter().map(|&i| ids[i]).collect();
        let out = s.alloc_str_from(&values)?;
        let source = names(s, x);
        if !source.is_empty() {
            let labels: Vec<_> = selected
                .iter()
                .map(|&i| source.get(i).copied().unwrap_or(CharId::NA))
                .collect();
            set_names(s, out, &labels)?;
        }
        Ok(out)
    } else {
        let indices: Vec<_> = selected.into_iter().map(|i| (i + 1) as i32).collect();
        Ok(s.alloc_int_from(&indices)?)
    }
}

fn compile(ctx: &mut Ctx, call: &Call, id: CharId, options: Options) -> EvalResult<Matcher> {
    let pattern = input(ctx.session(), id, options)?;
    Matcher::compile(&pattern, options).map_err(|msg| error(ctx, call, msg))
}

pub(super) fn strsplit(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, split, fixed, perl, bytes] = formals(
        ctx.session(),
        args,
        ["x", "split", "fixed", "perl", "useBytes"],
    );
    let options = Options::read(ctx, call, SEXP_MISSING, perl, fixed, bytes)?;
    let Ok(ids) = ctx.session().str_slice(x) else {
        return Err(error(ctx, call, "non-character argument"));
    };
    let ids = ids.to_vec();
    let splits = character(ctx.session_mut(), split)?;
    for &id in &splits {
        ctx.protect(id.as_sexp());
    }
    let mut compiled = HashMap::new();
    let mut result = Vec::with_capacity(ids.len());
    let limit = ctx.host().limits().max_vector_bytes / size_of::<CharId>();
    let mut pieces = 0;
    for (i, id) in ids.into_iter().enumerate() {
        let split = recycle(&splits, i).unwrap_or(CharId::BLANK);
        let row = if id == CharId::NA || split == CharId::NA {
            vec![id]
        } else {
            let options = options.with_byte_marks(ctx.session(), &[id, split])?;
            let Some(text) = input_bytes(ctx, call, id, options, i)? else {
                let s = ctx.session_mut();
                let out = s.alloc_str_from(&[CharId::NA])?;
                ctx.protect(out);
                result.push(out);
                continue;
            };
            if compiled.len() >= 32 && !compiled.contains_key(&(split, options.bytes)) {
                compiled.clear();
            }
            let matcher = match compiled.entry((split, options.bytes)) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let empty = ctx.session().char_bytes(split)?.is_empty();
                    entry.insert(if empty {
                        None
                    } else {
                        Some(compile(ctx, call, split, options)?)
                    })
                }
            };
            let mut row = Vec::new();
            let mut remaining = text.as_slice();
            while !remaining.is_empty() {
                if row.len() >= limit.saturating_sub(pieces) {
                    return Err(error(ctx, call, "vector size limit exceeded"));
                }
                let found = if let Some(matcher) = matcher {
                    matcher
                        .find(remaining, 0, false)
                        .map_err(|msg| error(ctx, call, msg))?
                } else {
                    Some(Match::new(0, 0))
                };
                let (end, consumed) = match found {
                    None => (remaining.len(), remaining.len()),
                    Some(found) if found.start == 0 && found.end == 0 => {
                        let end = next_unit(remaining, 0, options.bytes);
                        (end, end)
                    }
                    Some(found) => (found.start, found.end),
                };
                row.push(char_output(
                    ctx.session_mut(),
                    id,
                    &remaining[..end],
                    options,
                )?);
                remaining = &remaining[consumed..];
            }
            row
        };
        pieces += row.len();
        let out = ctx.session_mut().alloc_str_from(&row)?;
        ctx.protect(out);
        result.push(out);
    }
    let s = ctx.session_mut();
    let out = list(s, &result)?;
    copy_attrs(s, x, out, |n| n == "names")?;
    Ok(out)
}

pub(super) fn sub(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    substitute(ctx, call, args, false)
}

pub(super) fn gsub(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    substitute(ctx, call, args, true)
}

fn substitute(ctx: &mut Ctx, call: &Call, args: &Args, global: bool) -> EvalResult {
    let [pattern, replacement, x, ignore, perl, fixed, bytes] = formals(
        ctx.session(),
        args,
        [
            "pattern",
            "replacement",
            "x",
            "ignore.case",
            "perl",
            "fixed",
            "useBytes",
        ],
    );
    let options = Options::read(ctx, call, ignore, perl, fixed, bytes)?;
    let pattern = single_pattern(ctx, call, pattern, "pattern")?;
    ctx.protect(pattern.as_sexp());
    let replacement = single_pattern(ctx, call, replacement, "replacement")?;
    let ids = character(ctx.session_mut(), x)?;
    for &id in &ids {
        ctx.protect(id.as_sexp());
    }
    let mut out = Vec::with_capacity(ids.len());
    if pattern == CharId::NA {
        out.resize(ids.len(), CharId::NA);
    } else {
        let options = options
            .with_byte_marks(ctx.session(), &[pattern, replacement])?
            .with_byte_marks(ctx.session(), &ids)?;
        if options.mode == Mode::Fixed && ctx.session().char_bytes(pattern)?.is_empty() {
            return Err(error(ctx, call, "zero-length pattern"));
        }
        let matcher = compile(ctx, call, pattern, options)?;
        let replacement_bytes = input(ctx.session(), replacement, options)?.into_owned();
        let limit = ctx.host().limits().max_vector_bytes;
        let mut total = 0usize;
        for (i, id) in ids.into_iter().enumerate() {
            if id == CharId::NA {
                out.push(id);
                continue;
            }
            let Some(bytes) = input_bytes(ctx, call, id, options, i)? else {
                out.push(id);
                continue;
            };
            let replacement = (replacement != CharId::NA).then_some(replacement_bytes.as_slice());
            let result = replace(
                &matcher,
                &bytes,
                replacement,
                options,
                global,
                limit.saturating_sub(total),
            )
            .map_err(|msg| error(ctx, call, msg))?;
            let id = match result {
                Replaced::Unchanged => id,
                Replaced::Missing => CharId::NA,
                Replaced::Value(value) => {
                    total = total.saturating_add(value.len());
                    let id = char_output(ctx.session_mut(), id, &value, options)?;
                    ctx.protect(id.as_sexp());
                    id
                }
            };
            out.push(id);
        }
    }
    let s = ctx.session_mut();
    let result = s.alloc_str_from(&out)?;
    copy_attrs(s, x, result, |_| true)?;
    Ok(result)
}

enum Replaced {
    Unchanged,
    Missing,
    Value(Vec<u8>),
}

fn replace(
    matcher: &Matcher,
    text: &[u8],
    replacement: Option<&[u8]>,
    options: Options,
    global: bool,
    limit: usize,
) -> Result<Replaced, String> {
    let captures = options.mode != Mode::Fixed && replacement.is_some_and(|r| r.contains(&b'\\'));
    let mut matches = matcher.matches(text, options, captures);
    let Some(found) = matches.next().transpose()? else {
        return Ok(Replaced::Unchanged);
    };
    let Some(replacement) = replacement else {
        return Ok(Replaced::Missing);
    };
    let mut out = Vec::new();
    let mut end = 0;
    for found in std::iter::once(Ok(found))
        .chain(matches)
        .take(if global { usize::MAX } else { 1 })
    {
        let found = found?;
        extend(&mut out, &text[end..found.start], limit)?;
        if captures {
            expand(&mut out, replacement, &found, text, options.mode, limit)?;
        } else {
            extend(&mut out, replacement, limit)?;
        }
        end = found.end;
    }
    extend(&mut out, &text[end..], limit)?;
    Ok(Replaced::Value(out))
}

fn extend(out: &mut Vec<u8>, next: &[u8], limit: usize) -> Result<(), String> {
    if next.len() > limit.saturating_sub(out.len()) {
        return Err("vector size limit exceeded".into());
    }
    out.extend_from_slice(next);
    Ok(())
}

fn expand(
    out: &mut Vec<u8>,
    replacement: &[u8],
    found: &Match,
    text: &[u8],
    mode: Mode,
    limit: usize,
) -> Result<(), String> {
    let mut bytes = replacement.iter().copied();
    let mut case = None;
    while let Some(b) = bytes.next() {
        if b != b'\\' {
            extend(out, &[b], limit)?;
            continue;
        }
        match bytes.next() {
            None => break,
            Some(b'U') if mode == Mode::Perl => case = Some(true),
            Some(b'L') if mode == Mode::Perl => case = Some(false),
            Some(b'E') if mode == Mode::Perl => case = None,
            Some(n @ b'1'..=b'9') => {
                if let Some((start, end)) =
                    found.groups.get(usize::from(n - b'0')).copied().flatten()
                {
                    let matched = &text[start..end];
                    match case {
                        Some(true) => extend(
                            out,
                            String::from_utf8_lossy(matched).to_uppercase().as_bytes(),
                            limit,
                        )?,
                        Some(false) => extend(
                            out,
                            String::from_utf8_lossy(matched).to_lowercase().as_bytes(),
                            limit,
                        )?,
                        None => extend(out, matched, limit)?,
                    }
                }
            }
            Some(b) => extend(out, &[b], limit)?,
        }
    }
    Ok(())
}

pub(super) fn trimws(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, which, whitespace] = formals(ctx.session(), args, ["x", "which", "whitespace"]);
    let which = if which == SEXP_MISSING {
        "both".to_owned()
    } else {
        string(ctx.session(), which).unwrap_or_default()
    };
    let (left, right) = match which.as_str() {
        "b" | "bo" | "bot" | "both" => (true, true),
        "l" | "le" | "lef" | "left" => (true, false),
        "r" | "ri" | "rig" | "righ" | "right" => (false, true),
        _ => {
            return Err(error(
                ctx,
                call,
                "'arg' should be one of “both”, “left”, “right”",
            ));
        }
    };
    let space = if whitespace == SEXP_MISSING {
        "[ \t\r\n]".to_owned()
    } else {
        string(ctx.session(), whitespace)
            .ok_or_else(|| error(ctx, call, "invalid 'whitespace' argument"))?
    };
    let options = Options {
        mode: Mode::Perl,
        ignore_case: false,
        bytes: false,
    };
    let left = left
        .then(|| Matcher::compile(format!("^(?:{space})+").as_bytes(), options))
        .transpose()
        .map_err(|msg| error(ctx, call, msg))?;
    let right = right
        .then(|| Matcher::compile(format!("(?:{space})+$").as_bytes(), options))
        .transpose()
        .map_err(|msg| error(ctx, call, msg))?;
    let ids = character(ctx.session_mut(), x)?;
    let mut out = Vec::with_capacity(ids.len());
    for (i, id) in ids.into_iter().enumerate() {
        if id == CharId::NA {
            out.push(id);
            continue;
        }
        let Some(bytes) = input_bytes(ctx, call, id, options, i)? else {
            out.push(id);
            continue;
        };
        let mut value = bytes.as_slice();
        if let Some(matcher) = &left
            && let Some(found) = matcher
                .find(value, 0, false)
                .map_err(|msg| error(ctx, call, msg))?
        {
            value = &value[found.end..];
        }
        if let Some(matcher) = &right
            && let Some(found) = matcher
                .find(value, 0, false)
                .map_err(|msg| error(ctx, call, msg))?
        {
            let mut trimmed = value[..found.start].to_vec();
            trimmed.extend_from_slice(&value[found.end..]);
            let value = char_output(ctx.session_mut(), id, &trimmed, options)?;
            ctx.protect(value.as_sexp());
            out.push(value);
            continue;
        }
        let value = char_output(ctx.session_mut(), id, value, options)?;
        ctx.protect(value.as_sexp());
        out.push(value);
    }
    vector_limit(ctx, call, out.len(), size_of::<CharId>())?;
    let s = ctx.session_mut();
    let result = s.alloc_str_from(&out)?;
    if s.type_of(x)? == roxide_value::SexpType::Str {
        copy_attrs(s, x, result, |_| true)?;
    }
    Ok(result)
}

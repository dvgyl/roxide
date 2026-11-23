use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{BASE_ENV, CharId, Encoding, RLogical, ValueApi};
use std::borrow::Cow;
use std::fmt::Write;

use super::tree;

use super::{char_text, flag};

fn extension(path: &str) -> Option<usize> {
    let base = path.rsplit('/').next()?;
    let (stem, ext) = base.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty() && ext.chars().all(char::is_alphanumeric))
        .then_some(path.len() - ext.len() - 1)
}

fn decimal_extension(ctx: &mut Ctx, ext: &str) -> EvalResult<bool> {
    if ext
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c.is_alphabetic())
    {
        return Ok(true);
    }
    ctx.scoped(|ctx| {
        let pattern = ctx.session_mut().scalar_str(r"^[\p{L}\p{Nd}]+$")?;
        ctx.protect(pattern);
        let value = ctx.session_mut().scalar_str(ext)?;
        ctx.protect(value);
        let name = ctx.session_mut().intern("perl")?;
        let perl = ctx.session_mut().scalar_lgl(RLogical::TRUE)?;
        let matched = ctx.call_function(
            "grepl",
            &[(None, pattern), (None, value), (Some(name), perl)],
            BASE_ENV,
        )?;
        Ok(ctx.session().lgl(matched)?.iter().next() == Some(RLogical::TRUE))
    })
}

pub(super) fn extensions(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = args.get(0);
    let compression = flag(ctx, call, args.get(1))?;
    let ext_only = flag(ctx, call, args.get(2))?;
    let count = ctx.session().str_slice(input)?.len();
    let mut result = Vec::with_capacity(count);
    let mut output = String::new();
    for i in 0..count {
        let id = ctx.session().str_slice(input)?[i];
        if id == CharId::NA && !ext_only {
            result.push(id);
            continue;
        }
        let path = char_text(ctx, id)?.unwrap_or(Cow::Borrowed(""));
        let full = path.len();
        let trimmed = if ext_only {
            path.trim_end_matches('/')
        } else {
            &path
        };
        let stem = if compression {
            extension(trimmed)
                .filter(|&dot| matches!(&trimmed[dot + 1..], "gz" | "bz2" | "xz"))
                .map_or(trimmed, |dot| &trimmed[..dot])
        } else {
            trimmed
        };
        let stem_len = stem.len();
        let dot = extension(stem);
        let mut special = String::new();
        if let Some(dot) = dot {
            let ext = &stem[dot + 1..];
            if ext
                .chars()
                .any(|c| !c.is_ascii() && c.is_numeric() && !c.is_alphabetic())
            {
                special.push_str(ext);
            }
        }
        let valid = special.is_empty() || decimal_extension(ctx, &special)?;
        let path = char_text(ctx, id)?.unwrap_or(Cow::Borrowed(""));
        let output_slice = if ext_only {
            match dot.filter(|_| valid) {
                Some(_) if path.ends_with('/') => &*path,
                Some(dot) => &path[dot + 1..],
                None => "",
            }
        } else {
            &path[..dot.filter(|_| valid).unwrap_or(stem_len)]
        };
        if id != CharId::NA && output_slice.len() == full {
            result.push(id);
        } else {
            output.clear();
            output.push_str(output_slice);
            result.push(
                ctx.session_mut()
                    .mk_char(output.as_bytes(), Encoding::Utf8)?,
            );
        }
    }
    Ok(ctx.session_mut().alloc_str_from(&result)?)
}

pub(super) fn non_ascii(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let source = args.get(0);
    let count = ctx.session().str_slice(source)?.len();
    let mut remaining = ctx.host().limits().max_vector_bytes;
    tree::charge(
        ctx,
        call,
        &mut remaining,
        count.saturating_mul(size_of::<RLogical>()),
    )?;
    let mut selected = Vec::with_capacity(count);
    for i in 0..count {
        let id = ctx.session().str_slice(source)?[i];
        if id == CharId::NA || ctx.session().char_bytes(id)?.is_ascii() {
            selected.push(RLogical::FALSE);
            continue;
        }
        selected.push(RLogical::TRUE);
        let bytes = ctx.session().char_bytes(id)?;
        let mut line = format!("{}: ", i + 1);
        let payload = bytes
            .iter()
            .map(|b| if b.is_ascii() { 1 } else { 4 })
            .sum::<usize>();
        tree::charge(
            ctx,
            call,
            &mut remaining,
            line.len().saturating_add(payload).saturating_add(1),
        )?;
        for &byte in ctx.session().char_bytes(id)? {
            if byte.is_ascii() {
                line.push(char::from(byte));
            } else {
                let _ = write!(line, "<{byte:02x}>");
            }
        }
        line.push('\n');
        if let Err(error) = ctx.host().write_err(line.as_bytes()) {
            return Err(ctx.stop(call.err_call(), error.to_string()));
        }
    }
    Ok(ctx.session_mut().alloc_lgl_from(&selected)?)
}

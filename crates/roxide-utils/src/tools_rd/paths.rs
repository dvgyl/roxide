use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{CharId, RLogical, ValueApi};
use std::fmt::Write;

use super::tree;

use super::{flag, strings};

fn extension(path: &str) -> Option<usize> {
    let base = path.rsplit('/').next()?;
    let (stem, ext) = base.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty() && ext.chars().all(char::is_alphanumeric))
        .then_some(path.len() - ext.len() - 1)
}

pub(super) fn extensions(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let paths = strings(ctx, args.get(0))?;
    let compression = flag(ctx, call, args.get(1))?;
    let ext_only = flag(ctx, call, args.get(2))?;
    let mut result = Vec::with_capacity(paths.len());
    for path in &paths {
        let output = match path {
            Some(path) if ext_only => {
                if extension(path.trim_end_matches('/')).is_none() {
                    Some("")
                } else {
                    Some(extension(path).map_or(path.as_str(), |dot| &path[dot + 1..]))
                }
            }
            Some(path) => {
                let mut stem = path.as_str();
                if compression
                    && let Some(dot) = extension(stem)
                    && matches!(&stem[dot + 1..], "gz" | "bz2" | "xz")
                {
                    stem = &stem[..dot];
                }
                Some(extension(stem).map_or(stem, |dot| &stem[..dot]))
            }
            None if ext_only => Some(""),
            None => None,
        };
        result.push(match output {
            Some(value) => ctx
                .session_mut()
                .mk_char(value.as_bytes(), roxide_value::Encoding::Utf8)?,
            None => CharId::NA,
        });
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

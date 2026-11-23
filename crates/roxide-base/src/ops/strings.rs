use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{CharId, NA_INTEGER, SEXP_MISSING, SexpType, ValueApi};

use super::attributes::has_class;
use super::common::{self, ch, chars, error, flag, formals, string, tag, text, vector_limit};
use super::types::convert;

pub(super) fn paste(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    paste_impl(ctx, call, args, false)
}
pub(super) fn paste0(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    paste_impl(ctx, call, args, true)
}

fn paste_impl(ctx: &mut Ctx, call: &Call, args: &Args, zero: bool) -> EvalResult {
    let s = ctx.session();
    let sep = if zero {
        String::new()
    } else {
        args.named(s, "sep")
            .and_then(|x| string(s, x))
            .unwrap_or_else(|| " ".into())
    };
    let collapse = args.named(s, "collapse").and_then(|x| string(s, x));
    let recycle0 = args.named(s, "recycle0").unwrap_or(SEXP_MISSING);
    let inputs: std::vec::Vec<_> = args
        .iter()
        .filter(|(n, _)| {
            !matches!(tag(s, *n), Some("collapse" | "recycle0"))
                && (zero || tag(s, *n) != Some("sep"))
        })
        .map(|(_, v)| v)
        .collect();
    let recycle0 = flag(ctx, recycle0, false)?;
    let mut parts = std::vec::Vec::new();
    for x in inputs {
        let x = convert(ctx, call, x, SexpType::Str)?;
        let s = ctx.session();
        parts.push(
            chars(s, x)
                .iter()
                .map(|&v| text(s, v))
                .collect::<std::vec::Vec<_>>(),
        );
    }
    let len = if recycle0 && parts.iter().any(std::vec::Vec::is_empty) {
        0
    } else {
        parts.iter().map(std::vec::Vec::len).max().unwrap_or(0)
    };
    vector_limit(ctx, call, len, size_of::<CharId>())?;
    let values: std::vec::Vec<_> = (0..len)
        .map(|i| {
            parts
                .iter()
                .map(|v| if v.is_empty() { "" } else { &v[i % v.len()] })
                .collect::<std::vec::Vec<_>>()
                .join(&sep)
        })
        .collect();
    let s = ctx.session_mut();
    if let Some(sep) = collapse {
        return Ok(s.scalar_str(&values.join(&sep))?);
    }
    let ids = values
        .iter()
        .map(|v| ch(s, v))
        .collect::<Result<std::vec::Vec<_>, _>>()?;
    Ok(s.alloc_str_from(&ids)?)
}

pub(super) fn nchar(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, mode, _, keep_na] = formals(ctx.session(), args, ["x", "type", "allowNA", "keepNA"]);
    let mode = string(ctx.session(), mode).unwrap_or_else(|| "chars".into());
    let keep_na = flag(ctx, keep_na, mode != "width")?;
    if has_class(ctx.session(), x, "factor") {
        return Err(error(ctx, call, "'nchar()' requires a character vector"));
    }
    let vec = convert(ctx, call, x, SexpType::Str)?;
    let s = ctx.session_mut();
    let values: std::vec::Vec<_> = s
        .str_slice(vec)?
        .iter()
        .map(|&id| {
            if id == CharId::NA {
                return if keep_na { NA_INTEGER } else { 2 };
            }
            let v = text(s, id);
            if mode == "bytes" {
                v.len() as i32
            } else {
                v.chars().count() as i32
            }
        })
        .collect();
    let out = s.alloc_int_from(&values)?;
    common::copy_attrs(s, x, out, |n| matches!(n, "dim" | "dimnames" | "names"))?;
    Ok(out)
}

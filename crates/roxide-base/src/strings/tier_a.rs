use std::collections::{HashMap, HashSet};

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind,
    Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, NA_INTEGER, SEXP_NIL, SexpType, ValueApi};

use crate::ops::common::{error, flag, number, set, string, vector_limit};

pub(crate) const MODULE: Module = Module {
    prims: &[
        internal("rox_utf8_to_int", utf8_to_int, 1),
        internal("rox_int_to_utf8", int_to_utf8, 3),
        internal("rox_iconv", iconv, 6),
        internal("rox_make_unique", make_unique, 2),
        internal("rox_strtoi", strtoi, 2),
        internal("rox_encode_string", encode, 2),
    ],
    prelude: include_str!("tier_a.R"),
};

const fn internal(name: &'static str, fun: PrimFn, n: usize) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(n),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
    }
}

fn utf8_to_int(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let s = ctx.session();
    let Some(&id) = s
        .str_slice(args.get(0))
        .ok()
        .and_then(|values| values.first())
    else {
        return Err(error(
            ctx,
            call,
            "argument must be a character vector of length 1",
        ));
    };
    let values = if id == CharId::NA {
        vec![NA_INTEGER]
    } else {
        let data = s.char_bytes(id)?;
        std::str::from_utf8(data).map_or_else(
            |_| vec![NA_INTEGER],
            |text| text.chars().map(|c| c as i32).collect(),
        )
    };
    Ok(ctx.session_mut().alloc_int_from(&values)?)
}

fn int_to_utf8(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let multiple = flag(ctx, args.get(1), false)?;
    let allow_surrogate = flag(ctx, args.get(2), false)?;
    if allow_surrogate && multiple {
        ctx.warn_now(
            call.err_call(),
            "allow_surrogate_pairs = TRUE is incompatible with multiple = TRUE and will be ignored",
        )?;
    }
    let (x, _) = ctx.session_mut().coerce(args.get(0), SexpType::Int)?;
    let values = ctx.session().int(x)?.to_vec();
    vector_limit(ctx, call, values.len(), 4)?;
    let s = ctx.session_mut();
    let mut out = Vec::new();
    let mut bytes = Vec::new();
    let mut values = values.into_iter().peekable();
    while let Some(mut value) = values.next() {
        if value == 0 && !multiple {
            continue;
        }
        if allow_surrogate
            && !multiple
            && (0xd800..=0xdbff).contains(&value)
            && let Some(low) = values.next_if(|low| (0xdc00..=0xdfff).contains(low))
        {
            value = 0x10000 + ((value - 0xd800) << 10) + low - 0xdc00;
        }
        if value != 0 {
            if let Some(c) = char::from_u32(value as u32) {
                bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            } else if multiple {
                out.push(CharId::NA);
                continue;
            } else {
                return Ok(s.alloc_str_from(&[CharId::NA])?);
            }
        }
        if multiple {
            out.push(s.mk_char(&bytes, Encoding::Utf8)?);
            bytes.clear();
        }
    }
    if !multiple {
        out.push(s.mk_char(&bytes, Encoding::Utf8)?);
    }
    Ok(s.alloc_str_from(&out)?)
}

#[derive(Clone, Copy)]
enum Codec {
    Native,
    Utf8,
    Latin1,
    Ascii,
}

fn codec(s: &roxide_value::RSession, x: SexpId) -> Option<Codec> {
    Some(match string(s, x)?.to_ascii_lowercase().as_str() {
        "" => Codec::Native,
        "utf8" | "utf-8" => Codec::Utf8,
        "latin1" | "iso-8859-1" | "iso8859-1" => Codec::Latin1,
        "ascii" | "us-ascii" => Codec::Ascii,
        _ => return None,
    })
}

fn iconv(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    if x == SEXP_NIL {
        let s = ctx.session_mut();
        let ids = ["ASCII", "ISO-8859-1", "UTF-8"].map(|name| crate::ops::common::ch(s, name));
        let ids = ids.into_iter().collect::<Result<Vec<_>, _>>()?;
        return Ok(s.alloc_str_from(&ids)?);
    }
    let from = codec(ctx.session(), args.get(1));
    let to = codec(ctx.session(), args.get(2));
    let (Some(from), Some(to)) = (from, to) else {
        return Err(error(ctx, call, "unsupported conversion"));
    };
    let sub = string(ctx.session(), args.get(3));
    let mark = flag(ctx, args.get(4), true)?;
    let raw = flag(ctx, args.get(5), false)?;
    let (x, _) = ctx.session_mut().coerce(x, SexpType::Str)?;
    ctx.protect(x);
    let ids = ctx.session().str_slice(x)?.to_vec();
    let mut output = Vec::new();
    let mut strings = Vec::new();
    let mut total = 0usize;
    for id in ids {
        let result = if id == CharId::NA {
            None
        } else {
            let s = ctx.session();
            let from = if matches!(from, Codec::Native) {
                match s.char_encoding(id)? {
                    Encoding::Latin1 => Codec::Latin1,
                    _ => Codec::Utf8,
                }
            } else {
                from
            };
            convert_bytes(s.char_bytes(id)?, from, to, sub.as_deref())
        };
        if let Some(bytes) = &result {
            total = total.saturating_add(bytes.len());
            vector_limit(ctx, call, total, 1)?;
        }
        let s = ctx.session_mut();
        if raw {
            output.push(result.map_or(Ok(SEXP_NIL), |bytes| s.alloc_raw_from(&bytes))?);
        } else if let Some(bytes) = result {
            let encoding = if mark {
                match to {
                    Codec::Utf8 => Encoding::Utf8,
                    Codec::Latin1 => Encoding::Latin1,
                    _ => Encoding::Native,
                }
            } else {
                Encoding::Native
            };
            strings.push(s.mk_char(&bytes, encoding)?);
        } else {
            strings.push(CharId::NA);
        }
    }
    let s = ctx.session_mut();
    if raw {
        Ok(crate::ops::common::list(s, &output)?)
    } else {
        let result = s.alloc_str_from(&strings)?;
        crate::ops::common::copy_attrs(s, x, result, |name| {
            matches!(name, "names" | "dim" | "dimnames")
        })?;
        Ok(result)
    }
}

fn convert_bytes(mut bytes: &[u8], from: Codec, to: Codec, sub: Option<&str>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    while let Some(&first) = bytes.first() {
        let (code, n) = match from {
            Codec::Latin1 => (Some(char::from(first)), 1),
            Codec::Ascii => (first.is_ascii().then_some(char::from(first)), 1),
            _ => {
                let n = match first {
                    0..=0x7f => 1,
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf4 => 4,
                    _ => 1,
                };
                bytes
                    .get(..n)
                    .and_then(|data| std::str::from_utf8(data).ok())
                    .and_then(|s| s.chars().next())
                    .map_or((None, 1), |ch| (Some(ch), n))
            }
        };
        match (code, to) {
            (Some(c), Codec::Utf8 | Codec::Native) => {
                out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            }
            (Some(c), Codec::Ascii) if c.is_ascii() => out.push(c as u8),
            (Some(c), Codec::Latin1) if u32::from(c) <= 255 => out.push(c as u8),
            _ => match sub? {
                "byte" => {
                    for byte in &bytes[..n] {
                        out.extend_from_slice(format!("<{byte:02x}>").as_bytes());
                    }
                }
                text => {
                    for _ in 0..n {
                        out.extend_from_slice(text.as_bytes());
                    }
                }
            },
        }
        bytes = &bytes[n..];
    }
    Some(out)
}

fn make_unique(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sep = string(ctx.session(), args.get(1))
        .ok_or_else(|| error(ctx, call, "invalid 'sep' argument"))?;
    let s = ctx.session();
    let ids = s.str_slice(args.get(0))?.to_vec();
    let mut used: HashSet<String> = ids
        .iter()
        .map(|&id| crate::ops::common::text(s, id))
        .collect();
    let mut times = HashMap::new();
    let mut output = Vec::with_capacity(ids.len());
    let mut bytes = 0usize;
    for id in ids {
        let name = if id == CharId::NA {
            "NA".to_owned()
        } else {
            crate::ops::common::text(ctx.session(), id)
        };
        let count = times.entry(name.clone()).or_insert(0usize);
        if *count == 0 {
            *count = 1;
            output.push(id);
            continue;
        }
        loop {
            let label = format!("{name}{sep}{count}");
            *count += 1;
            if used.insert(label.clone()) {
                bytes = bytes.saturating_add(label.len());
                vector_limit(ctx, call, bytes, 1)?;
                output.push(crate::ops::common::ch(ctx.session_mut(), &label)?);
                break;
            }
        }
    }
    Ok(ctx.session_mut().alloc_str_from(&output)?)
}

fn strtoi(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let base = number(ctx.session_mut(), args.get(1))?;
    if base != 0.0 && !(2.0..=36.0).contains(&base) {
        return Err(error(ctx, call, "invalid 'base' argument"));
    }
    let (x, _) = ctx.session_mut().coerce(args.get(0), SexpType::Str)?;
    let s = ctx.session_mut();
    let values = s
        .str_slice(x)?
        .iter()
        .map(|&id| {
            let text = crate::ops::common::text(s, id);
            parse_integer(&text, base as u32).unwrap_or(NA_INTEGER)
        })
        .collect::<Vec<_>>();
    Ok(s.alloc_int_from(&values)?)
}

fn parse_integer(text: &str, mut base: u32) -> Option<i32> {
    let text = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let (sign, text) = if let Some(s) = text.strip_prefix('-') {
        (-1, s)
    } else {
        (1, text.strip_prefix('+').unwrap_or(text))
    };
    let hex = || text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"));
    if base == 0 {
        base = if hex().is_some() {
            16
        } else if text.starts_with('0') {
            8
        } else {
            10
        };
    }
    let text = if base == 16 {
        hex().unwrap_or(text)
    } else {
        text
    };
    i32::from_str_radix(text, base)
        .ok()
        .and_then(|n| if n < 0 { None } else { n.checked_mul(sign) })
}

fn encode(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let quote = string(ctx.session(), args.get(1))
        .ok_or_else(|| error(ctx, call, "invalid 'quote' argument"))?;
    let x = args.get(0);
    let ids = ctx.session().str_slice(x)?.to_vec();
    let mut output = Vec::with_capacity(ids.len());
    let mut bytes = 0usize;
    for id in ids {
        if id == CharId::NA {
            output.push(id);
        } else {
            let encoded = roxide_value::encode_string(ctx.session().char_bytes(id)?);
            let data = &encoded[1..encoded.len() - 1];
            let field = if quote == "\"" {
                encoded
            } else {
                let data = data.replace("\\\"", "\"");
                if quote.is_empty() {
                    data
                } else {
                    format!(
                        "{quote}{}{quote}",
                        data.replace(&quote, &format!("\\{quote}"))
                    )
                }
            };
            bytes = bytes.saturating_add(field.len());
            vector_limit(ctx, call, bytes, 1)?;
            output.push(crate::ops::common::ch(ctx.session_mut(), &field)?);
        }
    }
    let result = ctx.session_mut().alloc_str_from(&output)?;
    let names = crate::ops::common::attr(ctx.session(), x, "names");
    set(ctx.session_mut(), result, "names", names)?;
    Ok(result)
}

use std::ops::Range;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{BASE_ENV, FALSE_SCALAR, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use super::{attr, flag, set, strings};
use roxide_value::deparse::deparse_bounded;

pub(super) fn ends(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let n = counts(ctx, call, args.get(1))?;
    let tail = flag(ctx, call, args.get(2))?;
    let keep = flag(ctx, call, args.get(3))?;
    let ty = ctx.session().type_of(x)?;
    if matches!(ty, SexpType::Clo | SexpType::Builtin | SexpType::Special) {
        return function(ctx, call, x, &n, tail);
    }

    let s = ctx.session_mut();
    let class = attr(s, x, "class")?;
    let frame = class != SEXP_NIL
        && s.str_slice(class)?
            .iter()
            .any(|&id| s.char_bytes(id).ok() == Some(b"data.frame"));
    let dim = attr(s, x, "dim")?;
    let sizes = if frame {
        let rows = s.attr_row_names(x)?;
        vec![s.xlength(rows)?, s.xlength(x)?]
    } else if dim != SEXP_NIL {
        s.int(dim)?
            .iter()
            .map(|size| size.max(0) as usize)
            .collect()
    } else {
        validate(ctx, call, &n, None)?;
        let caller = ctx.closure_caller(call.env);
        let length = ctx.call_function("length", &[(None, x)], caller)?;
        let len = ctx.session_mut().coerce(length, SexpType::Real)?.0;
        let len = ctx.session().real(len)?.iter().next().unwrap_or(0.0);
        let len = len as usize;
        let sub = indices(ctx, call, selection(len, n[0], tail))?;
        return subset(ctx, call, x, &[sub], false);
    };

    validate(ctx, call, &n, Some(sizes.len()))?;
    if !frame && sizes.len() == 1 {
        return one_dimension(ctx, call, x, selection(sizes[0], n[0], tail));
    }
    let classless_array = dim != SEXP_NIL && !frame;
    let mut subs = Vec::new();
    for (i, &size) in sizes.iter().enumerate() {
        let count = n.get(i).copied().filter(|n| !n.is_nan());
        let sub = match count {
            Some(count) => indices(ctx, call, selection(size, count, tail))?,
            None => SEXP_MISSING,
        };
        ctx.protect(sub);
        subs.push(sub);
    }
    let result = subset(ctx, call, x, &subs, true)?;
    ctx.protect(result);
    if tail && keep && classless_array {
        add_names(ctx, call, result, &sizes, &n)?;
    }
    Ok(result)
}

fn one_dimension(ctx: &mut Ctx, call: &Call, x: SexpId, range: Range<usize>) -> EvalResult {
    let count = range.len();
    let sub = indices(ctx, call, range)?;
    ctx.protect(sub);
    let result = subset(ctx, call, x, &[sub], false)?;
    ctx.protect(result);
    let dim_names = attr(ctx.session_mut(), x, "dimnames")?;
    let dim = ctx.session_mut().scalar_int(count as i32)?;
    set(ctx.session_mut(), result, "dim", dim)?;
    if dim_names == SEXP_NIL {
        return Ok(result);
    }
    let old = ctx.session().list_get(dim_names, 0)?;
    let new = if old == SEXP_NIL {
        SEXP_NIL
    } else {
        subset(ctx, call, old, &[sub], false)?
    };
    let s = ctx.session_mut();
    let names = s.alloc_list(1)?;
    s.list_set(names, 0, new)?;
    let labels = attr(s, dim_names, "names")?;
    set(s, names, "names", labels)?;
    set(s, result, "dimnames", names)?;
    Ok(result)
}

fn counts(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Vec<f64>> {
    let ty = ctx.session().type_of(x)?;
    let len = ctx.session().xlength(x)?;
    if len == 0 {
        return Ok(Vec::new());
    }
    if !matches!(ty, SexpType::Lgl | SexpType::Int | SexpType::Real) {
        return Err(ctx.stop(
            call.err_call(),
            "invalid 'n' - must be numeric, possibly NA.",
        ));
    }
    if len > ctx.host().limits().max_vector_bytes / size_of::<f64>() {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    let x = ctx.session_mut().coerce(x, SexpType::Real)?.0;
    Ok(ctx.session().real(x)?.to_vec())
}

fn validate(ctx: &mut Ctx, call: &Call, n: &[f64], rank: Option<usize>) -> EvalResult<()> {
    let error = if n.iter().all(|n| n.is_nan()) {
        "invalid 'n' - must contain at least one non-missing element, got none.".into()
    } else if rank.is_none() && n.len() != 1 {
        format!(
            "invalid 'n' - must have length one when dim(x) is NULL, got {}",
            n.len()
        )
    } else if let Some(rank) = rank.filter(|&rank| n.len() > rank) {
        format!(
            "invalid 'n' - length(n) must be <= length(dim(x)), got {} > {rank}",
            n.len()
        )
    } else {
        return Ok(());
    };
    Err(ctx.stop(call.err_call(), error))
}

fn selection(size: usize, count: f64, tail: bool) -> Range<usize> {
    let len = if count < 0.0 {
        size as f64 + count
    } else {
        count
    };
    let len = len.clamp(0.0, size as f64);
    if tail {
        size - len.ceil() as usize..size
    } else {
        0..len as usize
    }
}

fn indices(ctx: &mut Ctx, call: &Call, range: Range<usize>) -> EvalResult {
    if range.end > i32::MAX as usize
        || range.len() > ctx.host().limits().max_vector_bytes / size_of::<i32>()
    {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    let values: Vec<_> = range.map(|i| i as i32 + 1).collect();
    Ok(ctx.session_mut().alloc_int_from(&values)?)
}

fn subset(ctx: &mut Ctx, call: &Call, x: SexpId, subs: &[SexpId], dimensions: bool) -> EvalResult {
    ctx.scoped(|ctx| {
        let mut args = vec![(None, x)];
        for &sub in subs {
            ctx.protect(sub);
            args.push((None, sub));
        }
        if dimensions {
            let drop = ctx.session_mut().intern("drop")?;
            args.push((Some(drop), FALSE_SCALAR));
        }
        let op = ctx.session_mut().intern("[")?;
        let fun = ctx.find_fun(op, BASE_ENV)?;
        ctx.apply(fun, &args, call.err_call(), call.env)
    })
}

fn add_names(
    ctx: &mut Ctx,
    call: &Call,
    result: SexpId,
    sizes: &[usize],
    counts: &[f64],
) -> EvalResult<()> {
    let s = ctx.session_mut();
    let existing = attr(s, result, "dimnames")?;
    let names = s.alloc_list(sizes.len())?;
    ctx.protect(names);
    if existing != SEXP_NIL {
        let labels = attr(ctx.session_mut(), existing, "names")?;
        set(ctx.session_mut(), names, "names", labels)?;
    }
    for (i, &size) in sizes.iter().enumerate() {
        let s = ctx.session();
        let old = if existing == SEXP_NIL {
            SEXP_NIL
        } else {
            s.list_get(existing, i)?
        };
        let new = if old != SEXP_NIL {
            old
        } else if let Some(count) = counts.get(i).filter(|n| !n.is_nan()) {
            let range = selection(size, *count, true);
            if range.len().saturating_mul(32) > ctx.host().limits().max_vector_bytes {
                return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
            }
            if range.is_empty() {
                SEXP_NIL
            } else {
                strings(
                    ctx.session_mut(),
                    range.map(|n| {
                        let index = n + 1;
                        match i {
                            0 => format!("[{index},]"),
                            1 => format!("[,{index}]"),
                            _ => index.to_string(),
                        }
                    }),
                )?
            }
        } else {
            SEXP_NIL
        };
        ctx.session_mut().list_set(names, i, new)?;
    }
    set(ctx.session_mut(), result, "dimnames", names)
}

fn function(ctx: &mut Ctx, call: &Call, x: SexpId, n: &[f64], tail: bool) -> EvalResult {
    validate(ctx, call, n, None)?;
    let lines = if let Some(name) = ctx.primitive_name(x) {
        vec![format!(".Primitive(\"{name}\")")]
    } else {
        let byte_limit = ctx.host().limits().max_vector_bytes.min(1024 * 1024);
        let line_limit = if !tail && n[0] >= 0.0 {
            n[0] as usize
        } else {
            usize::MAX
        };
        let (lines, used_budget) =
            deparse_bounded(ctx.session(), x, Default::default(), line_limit, byte_limit)?;
        if used_budget && (tail || n[0] < 0.0) {
            return Err(ctx.stop(call.err_call(), "function deparse exceeds the size limit"));
        }
        lines
    };
    let range = selection(lines.len(), n[0], tail);
    let rows = range.len() as i32;
    let s = ctx.session_mut();
    let result = strings(s, lines[range.clone()].iter())?;
    let dim = s.alloc_int_from(&[rows, 1])?;
    let names = s.alloc_list(2)?;
    if rows > 0 {
        let rownames = strings(s, range.map(|i| (i + 1).to_string()))?;
        s.list_set(names, 0, rownames)?;
    }
    let column = s.scalar_str("")?;
    s.list_set(names, 1, column)?;
    let class = s.scalar_str("noquote")?;
    set(s, result, "dim", dim)?;
    set(s, result, "dimnames", names)?;
    set(s, result, "class", class)?;
    Ok(result)
}

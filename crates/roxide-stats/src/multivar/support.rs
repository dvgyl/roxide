use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{Encoding, SEXP_NIL, ValueApi};

use crate::distn::support::{attr, limit, number, numbers, set_attr, set_dim, set_dimnames};

pub(super) fn area<T>(ctx: &mut Ctx, call: &Call, rows: usize, cols: usize) -> EvalResult<usize> {
    let size = rows
        .checked_mul(cols)
        .ok_or_else(|| ctx.stop(call.err_call(), "vector size limit exceeded"))?;
    limit::<T>(ctx, call, size)?;
    Ok(size)
}

pub(super) fn size(ctx: &mut Ctx, call: &Call, d: SexpId) -> EvalResult<usize> {
    let size = attr(ctx, d, "Size")?;
    if size == SEXP_NIL {
        return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
    }
    let size = number(ctx, call, size)?;
    if size < 0.0 {
        return Err(ctx.stop(call.err_call(), "invalid 'nrow' value (< 0)"));
    }
    if !size.is_finite() || size > 65536.0 {
        return Err(ctx.stop(call.err_call(), "size cannot be NA nor exceed 65536"));
    }
    Ok(size as usize)
}

pub(super) fn pair_count(ctx: &mut Ctx, call: &Call, n: usize) -> EvalResult<usize> {
    let len = n
        .checked_mul(n.saturating_sub(1))
        .ok_or_else(|| ctx.stop(call.err_call(), "vector size limit exceeded"))?
        / 2;
    limit::<f64>(ctx, call, len)?;
    Ok(len)
}

pub(super) struct Matrix {
    pub rows: usize,
    pub cols: usize,
    pub values: Vec<f64>,
}

impl Matrix {
    pub fn read(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Self> {
        let (rows, cols) = dimensions(ctx, call, x)?;
        let values = numbers(ctx, call, x)?;
        if rows.checked_mul(cols) != Some(values.len()) {
            return Err(ctx.stop(call.err_call(), "invalid matrix dimensions"));
        }
        Ok(Self { rows, cols, values })
    }

    pub fn at(&self, row: usize, col: usize) -> f64 {
        self.values[col * self.rows + row]
    }
}

pub(super) fn dimensions(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<(usize, usize)> {
    let dim = attr(ctx, x, "dim")?;
    let dims = ctx.session().int(dim).map(|ints| ints.to_vec());
    match dims.as_deref() {
        Ok([r, c]) if *r >= 0 && *c >= 0 => Ok((*r as usize, *c as usize)),
        _ => Err(ctx.stop(call.err_call(), "invalid matrix dimensions")),
    }
}

pub(super) fn dimnames(ctx: &mut Ctx, x: SexpId) -> EvalResult<(SexpId, SexpId)> {
    let names = attr(ctx, x, "dimnames")?;
    if names == SEXP_NIL {
        return Ok((SEXP_NIL, SEXP_NIL));
    }
    Ok((
        ctx.session().list_get(names, 0)?,
        ctx.session().list_get(names, 1)?,
    ))
}

pub(super) fn matrix(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    shape: (usize, usize),
    names: (SexpId, SexpId),
) -> EvalResult {
    ctx.protect(x);
    set_dim(ctx, call, x, shape.0, shape.1)?;
    set_dimnames(ctx, x, names.0, names.1)?;
    Ok(x)
}

pub(super) fn strings(
    ctx: &mut Ctx,
    values: impl IntoIterator<Item = impl AsRef<str>>,
) -> EvalResult {
    let s = ctx.session_mut();
    let ids = values
        .into_iter()
        .map(|value| s.mk_char(value.as_ref().as_bytes(), Encoding::Utf8))
        .collect::<Result<Vec<_>, _>>()?;
    let result = s.alloc_str_from(&ids)?;
    ctx.protect(result);
    Ok(result)
}

pub(super) fn real(ctx: &mut Ctx, values: &[f64]) -> EvalResult {
    let id = ctx.session_mut().alloc_real_from(values)?;
    ctx.protect(id);
    Ok(id)
}

pub(super) fn ints(ctx: &mut Ctx, values: &[i32]) -> EvalResult {
    let id = ctx.session_mut().alloc_int_from(values)?;
    ctx.protect(id);
    Ok(id)
}

pub(super) fn list(ctx: &mut Ctx, fields: &[(&str, SexpId)], class: Option<&str>) -> EvalResult {
    let id = ctx.session_mut().alloc_list(fields.len())?;
    ctx.protect(id);
    for (i, &(_, value)) in fields.iter().enumerate() {
        ctx.session_mut().list_set(id, i, value)?;
    }
    let names = strings(ctx, fields.iter().map(|&(name, _)| name))?;
    set_attr(ctx, id, "names", names)?;
    if let Some(class) = class {
        let class = strings(ctx, [class])?;
        set_attr(ctx, id, "class", class)?;
    }
    Ok(id)
}

pub(super) fn choice(ctx: &Ctx, id: SexpId) -> Option<&str> {
    let id = *ctx.session().str_slice(id).ok()?.first()?;
    std::str::from_utf8(ctx.session().char_bytes(id).ok()?).ok()
}

pub(super) fn match_choice(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = args.get(0);
    let choices = ctx.session().str_slice(args.get(1))?;
    let names: Vec<_> = choices
        .iter()
        .filter_map(|&id| std::str::from_utf8(ctx.session().char_bytes(id).ok()?).ok())
        .collect();
    let arg = choice(ctx, input);
    let arg = if choice(ctx, args.get(2)) == Some("dist")
        && arg.is_some_and(|arg| !arg.is_empty() && "euclidian".starts_with(arg))
    {
        Some("euclidean")
    } else {
        arg
    };
    let generic = choice(ctx, args.get(2));
    let default = (input == SEXP_NIL && !matches!(generic, Some("dist" | "hclust")))
        || roxide_value::identical(ctx.session(), input, args.get(1), Default::default())?;
    let matched = if default {
        choices.first()
    } else {
        let mut partial = names
            .iter()
            .zip(choices)
            .filter(|&(name, _)| arg.is_some_and(|arg| !arg.is_empty() && name.starts_with(arg)));
        names
            .iter()
            .zip(choices)
            .find_map(|(&name, id)| (Some(name) == arg).then_some(id))
            .or_else(|| {
                let first = partial.next()?;
                partial.next().is_none().then_some(first.1)
            })
    };
    if !default && ctx.session().xlength(input)? != 1 {
        let message = if matches!(generic, Some("dist" | "hclust")) {
            if ctx.session().xlength(input)? == 0 {
                "argument is of length zero"
            } else {
                "the condition has length > 1"
            }
        } else {
            "'arg' must be of length 1"
        };
        return Err(ctx.stop(call.err_call(), message));
    }
    let Some(&matched) = matched else {
        let message = match choice(ctx, args.get(2)) {
            Some("dist") => "invalid distance method".to_owned(),
            Some("hclust") => format!("invalid clustering method {}", arg.unwrap_or("NA")),
            _ => format!(
                "'arg' should be one of {}",
                names
                    .iter()
                    .map(|x| format!("“{x}”"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        return Err(ctx.stop(call.err_call(), message));
    };
    Ok(ctx.session_mut().alloc_str_from(&[matched])?)
}

pub(super) fn explicit_rows(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let rows = attr(ctx, args.get(0), "row.names")?;
    let automatic = ctx.session().int(rows).is_ok_and(|v| {
        let mut v = v.iter();
        v.next() == Some(roxide_value::NA_INTEGER) && v.next().is_some_and(|n| n < 0)
    });
    Ok(ctx
        .session_mut()
        .scalar_lgl(roxide_value::RLogical::from_option(Some(!automatic)))?)
}

pub(super) fn replace_call(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let (_, old_tail, _) = ctx.session().cons_parts(args.get(0))?;
    let name = choice(ctx, args.get(1)).unwrap_or_default().to_owned();
    let head = ctx.session_mut().intern(&name)?.as_sexp();
    let tail = if name == "model.frame" {
        let inputs: Vec<_> = ctx
            .session()
            .pairlist_iter(old_tail)
            .filter(|&(tag, _)| {
                tag.is_some_and(|tag| {
                    matches!(
                        ctx.session().sym_name_of(tag.as_sexp()).ok(),
                        Some("formula" | "data" | "subset" | "na.action")
                    )
                })
            })
            .collect();
        let mut tail = SEXP_NIL;
        for (tag, value) in inputs.into_iter().rev() {
            tail = ctx
                .session_mut()
                .cons(value, tail, tag.map_or(SEXP_NIL, |x| x.as_sexp()))?;
            ctx.protect(tail);
        }
        tail
    } else {
        old_tail
    };
    Ok(ctx.session_mut().lcons(head, tail, SEXP_NIL)?)
}

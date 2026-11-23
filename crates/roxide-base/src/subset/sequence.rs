use std::cmp::Ordering;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, NA_INTEGER, RLogical, RSession, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi,
};

use crate::ops::attributes::{has_class, row_count};
use crate::ops::common::{atomic, attr, count, error, flag, formals, number, tag, vector_limit};
use crate::ops::types::is_missing;

use super::index::selected;

pub(super) fn colon(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let (a, b) = (args.get(0), args.get(1));
    for x in [a, b] {
        let n = ctx.session().xlength(x)?;
        if n > 1 {
            ctx.warn_now(
                call.err_call(),
                format!("numerical expression has {n} elements: only the first used"),
            )?;
        }
        if n == 0 {
            return Err(error(ctx, call, "argument of length 0"));
        }
    }
    let x = number(ctx.session_mut(), a)?;
    let y = number(ctx.session_mut(), b)?;
    from_to(ctx, call, x, y)
}

fn from_to(ctx: &mut Ctx, call: &Call, x: f64, y: f64) -> EvalResult {
    if !x.is_finite() || !y.is_finite() {
        return Err(error(ctx, call, "NA/NaN argument"));
    }
    let n = count(ctx, call, (x - y).abs() + 1.0 + 1e-7)?;
    unit_sequence(ctx, call, x, n, if x <= y { 1 } else { -1 })
}

fn unit_sequence(ctx: &mut Ctx, call: &Call, from: f64, len: usize, step: i8) -> EvalResult {
    let last = from + len.saturating_sub(1) as f64 * f64::from(step);
    let is_int = from.fract() == 0.0
        && from > f64::from(i32::MIN)
        && from <= f64::from(i32::MAX)
        && last > f64::from(i32::MIN)
        && last <= f64::from(i32::MAX);
    vector_limit(ctx, call, len, if is_int { 4 } else { 8 })?;
    let s = ctx.session_mut();
    Ok(if len == 0 {
        s.alloc_int_from(&[])?
    } else if is_int {
        s.alloc_compact_intseq(from as i32, len, step)?
    } else {
        s.alloc_compact_realseq(from, len, f64::from(step))?
    })
}

pub(super) fn seq_along(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let n = ctx.session().xlength(args.get(0))?;
    unit_sequence(ctx, call, 1.0, n, 1)
}

pub(super) fn seq_len(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let n = number(ctx.session_mut(), args.get(0))?;
    let n = count(ctx, call, n)?;
    unit_sequence(ctx, call, 1.0, n, 1)
}

pub(super) fn seq(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [from, to, by, length, along] = formals(
        ctx.session(),
        args,
        ["from", "to", "by", "length.out", "along.with"],
    );
    let n = if along != SEXP_MISSING {
        Some(ctx.session().xlength(along)?)
    } else if length != SEXP_MISSING && length != SEXP_NIL {
        let n = number(ctx.session_mut(), length)?;
        Some(count(ctx, call, n.ceil())?)
    } else {
        None
    };
    if n == Some(0) {
        return unit_sequence(ctx, call, 1.0, 0, 1);
    }
    if from != SEXP_MISSING && to == SEXP_MISSING && by == SEXP_MISSING && n.is_none() {
        let s = ctx.session();
        if s.xlength(from)? != 1 || !matches!(s.type_of(from)?, SexpType::Int | SexpType::Real) {
            let n = s.xlength(from)?;
            return unit_sequence(ctx, call, 1.0, n, 1);
        }
        let to = number(ctx.session_mut(), from)?;
        return from_to(ctx, call, 1.0, to);
    }
    let s = ctx.session_mut();
    let integers = [from, to, by]
        .into_iter()
        .all(|v| v == SEXP_MISSING || s.type_of(v).ok() == Some(SexpType::Int));
    let a = if from == SEXP_MISSING {
        None
    } else {
        Some(number(s, from)?)
    };
    let b = if to == SEXP_MISSING {
        None
    } else {
        Some(number(s, to)?)
    };
    let step = if by == SEXP_MISSING {
        None
    } else {
        Some(number(s, by)?)
    };
    if a.into_iter().chain(b).chain(step).any(|v| !v.is_finite()) {
        return Err(error(
            ctx,
            call,
            "'from', 'to' and 'by' must be finite numbers",
        ));
    }
    if let Some(n) = n {
        if let Some(step) = step {
            if a.is_some() && b.is_some() {
                return Err(error(ctx, call, "too many arguments"));
            }
            let from = a.unwrap_or_else(|| b.unwrap_or(1.0) - n.saturating_sub(1) as f64 * step);
            return stepped(ctx, call, from, step, n, None, integers);
        }
        return match (a, b) {
            (None, None) => unit_sequence(ctx, call, 1.0, n, 1),
            (Some(a), None) => unit_sequence(ctx, call, a, n, 1),
            (None, Some(b)) => unit_sequence(ctx, call, b - (n - 1) as f64, n, 1),
            (Some(a), Some(b)) => stepped(
                ctx,
                call,
                a,
                (b - a) / n.saturating_sub(1).max(1) as f64,
                n,
                (n > 1).then_some(b),
                false,
            ),
        };
    }
    let a = a.unwrap_or(1.0);
    let b = b.unwrap_or(1.0);
    let Some(step) = step else {
        return from_to(ctx, call, a, b);
    };
    if a == b {
        return Ok(if from != SEXP_MISSING {
            from
        } else if to != SEXP_MISSING {
            to
        } else {
            ctx.session_mut().scalar_real(a)?
        });
    }
    let d = (b - a) / step;
    if step == 0.0 || d < 0.0 {
        return Err(error(ctx, call, "wrong sign in 'by' argument"));
    }
    let n = count(ctx, call, (d + 1e-10).floor() + 1.0)?;
    stepped(ctx, call, a, step, n, None, integers)
}

fn stepped(
    ctx: &mut Ctx,
    call: &Call,
    a: f64,
    by: f64,
    n: usize,
    last: Option<f64>,
    integers: bool,
) -> EvalResult {
    vector_limit(ctx, call, n, 8)?;
    let mut data: Vec<_> = (0..n)
        .map(|i| if i == 0 { a } else { a + i as f64 * by })
        .collect();
    if let (Some(end), Some(v)) = (last, data.last_mut()) {
        *v = end;
    }
    let s = ctx.session_mut();
    if integers
        && data
            .iter()
            .all(|&v| v.fract() == 0.0 && v > f64::from(i32::MIN) && v <= f64::from(i32::MAX))
    {
        let values: Vec<_> = data.into_iter().map(|v| v as i32).collect();
        return Ok(s.alloc_int_from(&values)?);
    }
    Ok(s.alloc_real_from(&data)?)
}

pub(super) fn rep(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, times, length, each] =
        formals(ctx.session(), args, ["x", "times", "length.out", "each"]);
    if x == SEXP_MISSING {
        return Err(error(ctx, call, "argument 'x' is missing, with no default"));
    }
    if x == SEXP_NIL {
        return Ok(x);
    }
    let s = ctx.session_mut();
    let n = s.xlength(x)?;
    let each = if each == SEXP_MISSING {
        1.0
    } else {
        number(s, each)?
    };
    let length = if length == SEXP_MISSING {
        f64::NAN
    } else {
        number(s, length)?
    };
    let each = count(ctx, call, if each.is_nan() { 1.0 } else { each })?;
    let base = n.saturating_mul(each);
    let positions = if !length.is_nan() {
        let len = count(ctx, call, length)?;
        vector_limit(ctx, call, len, size_of::<Option<usize>>())?;
        if base == 0 && n != 0 && len != 0 {
            return Err(error(ctx, call, "invalid 'each' argument"));
        }
        (0..len)
            .map(|i| (base != 0).then(|| (i % base) / each))
            .collect()
    } else {
        let s = ctx.session_mut();
        let repeats = if times == SEXP_MISSING {
            vec![1.0]
        } else {
            let (v, _) = s.coerce(times, SexpType::Real)?;
            s.real(v)?.to_vec()
        };
        let mul = if repeats.len() == 1 {
            count(ctx, call, repeats[0])?
        } else {
            1
        };
        if repeats.len() != 1 && repeats.len() != base {
            return Err(error(ctx, call, "invalid 'times' argument"));
        }
        if repeats.len() == 1 {
            let len = base.saturating_mul(mul);
            vector_limit(ctx, call, len, size_of::<Option<usize>>())?;
            (0..len).map(|i| Some((i % base) / each)).collect()
        } else {
            let mut out = Vec::new();
            for (i, r) in repeats.into_iter().enumerate() {
                let repeats = count(ctx, call, r)?;
                vector_limit(
                    ctx,
                    call,
                    out.len().saturating_add(repeats),
                    size_of::<Option<usize>>(),
                )?;
                out.extend(std::iter::repeat_n(Some(i / each), repeats));
            }
            out
        }
    };
    selected(ctx, x, &positions)
}

pub(super) fn rev(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let n = ctx.session().xlength(x)?;
    vector_limit(ctx, call, n, size_of::<Option<usize>>())?;
    selected(ctx, x, &(0..n).rev().map(Some).collect::<Vec<_>>())
}

fn end(ctx: &mut Ctx, call: &Call, args: &Args, tail: bool) -> EvalResult {
    let [x, n] = formals(ctx.session(), args, ["x", "n"]);
    let s = ctx.session_mut();
    let dim = attr(s, x, "dim");
    let array = dim != SEXP_NIL;
    let frame = has_class(s, x, "data.frame");
    let total = if frame {
        row_count(s, x)?
    } else if array {
        s.int(dim)?.iter().next().unwrap_or(0).max(0) as usize
    } else {
        s.xlength(x)?
    };
    let n = if n == SEXP_MISSING {
        6.0
    } else {
        number(s, n)?
    };
    if !n.is_finite() {
        return Err(error(
            ctx,
            call,
            "invalid 'n' - must contain at least one non-missing element",
        ));
    }
    let count = if n >= 0.0 {
        (n as usize).min(total)
    } else {
        total.saturating_sub((-n) as usize)
    };
    let start = if tail { total - count } else { 0 };
    vector_limit(ctx, call, count, size_of::<Option<usize>>())?;
    if array || frame {
        let s = ctx.session_mut();
        let rows = s.alloc_compact_intseq((start + 1) as i32, count, 1)?;
        let dims = if frame { 2 } else { s.xlength(dim)? };
        let mut args = vec![(None, x), (None, rows)];
        args.extend(std::iter::repeat_n((None, SEXP_MISSING), dims - 1));
        let drop = s.intern("drop")?;
        args.push((Some(drop), roxide_value::FALSE_SCALAR));
        return super::index::subset(ctx, call, &mut Args::from_pairs(args));
    }
    selected(
        ctx,
        x,
        &(start..start + count).map(Some).collect::<Vec<_>>(),
    )
}

pub(super) fn head(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    end(ctx, call, args, false)
}
pub(super) fn tail(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    end(ctx, call, args, true)
}

pub(super) fn sort(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, decreasing, na, method] = formals(
        ctx.session(),
        args,
        ["x", "decreasing", "na.last", "method"],
    );
    if x == SEXP_NIL {
        return Ok(x);
    }
    if ctx.session().type_of(x)? == SexpType::Raw {
        return Err(error(ctx, call, "raw vectors cannot be sorted"));
    }
    if ctx.session().xlength(x)? > 1 && method != SEXP_MISSING {
        let method = crate::ops::common::string(ctx.session(), method).unwrap_or_default();
        let methods = ["auto", "shell", "quick", "radix"];
        if method.is_empty() || methods.iter().filter(|m| m.starts_with(&method)).count() != 1 {
            return Err(error(
                ctx,
                call,
                "'arg' should be one of “auto”, “shell”, “quick”, “radix”",
            ));
        }
    }
    let desc = flag(ctx, decreasing, false)?;
    let last = na_position(ctx, na, None)?;
    let n = ctx.session().xlength(x)?;
    let order = ordering(ctx, call, &[x], n, last, &[desc])?;
    selected(ctx, x, &order.into_iter().map(Some).collect::<Vec<_>>())
}

pub(super) fn order(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let s = ctx.session();
    let na = args.named(s, "na.last").unwrap_or(SEXP_MISSING);
    let decreasing = args.named(s, "decreasing").unwrap_or(SEXP_MISSING);
    let inputs: Vec<_> = args
        .iter()
        .filter(|(n, _)| !matches!(tag(s, *n), Some("na.last" | "decreasing" | "method")))
        .map(|(_, v)| v)
        .collect();
    let Some(&first) = inputs.first() else {
        return Ok(SEXP_NIL);
    };
    let n = s.xlength(first)?;
    if inputs
        .iter()
        .any(|&v| ctx.session().xlength(v).ok() != Some(n))
    {
        return Err(error(ctx, call, "argument lengths differ"));
    }
    let last = na_position(ctx, na, Some(true))?;
    let desc = if decreasing == SEXP_MISSING {
        vec![false]
    } else {
        let s = ctx.session_mut();
        let (v, _) = s.coerce(decreasing, SexpType::Lgl)?;
        let d: Vec<_> = s.lgl(v)?.iter().map(|v| v.get() == Some(true)).collect();
        if d.len() != 1 && d.len() != inputs.len() {
            return Err(error(
                ctx,
                call,
                "'decreasing' must be of length 1 or the number of columns",
            ));
        }
        d
    };
    let result = ordering(ctx, call, &inputs, n, last, &desc)?;
    let vals: Vec<_> = result.into_iter().map(|i| (i + 1) as i32).collect();
    Ok(ctx.session_mut().alloc_int_from(&vals)?)
}

fn na_position(ctx: &mut Ctx, x: SexpId, default: Option<bool>) -> EvalResult<Option<bool>> {
    if x == SEXP_MISSING {
        return Ok(default);
    }
    let s = ctx.session_mut();
    let (x, _) = s.coerce(x, SexpType::Lgl)?;
    Ok(s.lgl(x)?.iter().next().and_then(RLogical::get))
}

fn ordering(
    ctx: &mut Ctx,
    call: &Call,
    values: &[SexpId],
    n: usize,
    na: Option<bool>,
    decreasing: &[bool],
) -> EvalResult<Vec<usize>> {
    vector_limit(ctx, call, n, size_of::<usize>())?;
    let s = ctx.session();
    let mut columns = Vec::new();
    for &x in values {
        let ty = s.type_of(x)?;
        if (!atomic(ty) && ty != SexpType::Nil) || ty == SexpType::Raw {
            return Err(error(
                ctx,
                call,
                format!(
                    "unimplemented type '{}' in 'orderVector1'",
                    ty.type_name(false)
                ),
            ));
        }
        columns.push((x, ty));
    }
    let mut ix: Vec<_> = (0..n)
        .filter(|&i| na.is_some() || values.iter().all(|&x| !is_missing(s, x, i)))
        .collect();
    ix.sort_by(|&i, &j| {
        for (col, &(x, ty)) in columns.iter().enumerate() {
            let a = is_missing(s, x, i);
            let b = is_missing(s, x, j);
            let cmp = if a || b {
                if na.unwrap_or(true) {
                    a.cmp(&b)
                } else {
                    b.cmp(&a)
                }
            } else {
                let cmp = compare(s, x, ty, i, j);
                if decreasing[col % decreasing.len()] {
                    cmp.reverse()
                } else {
                    cmp
                }
            };
            if cmp != Ordering::Equal {
                return cmp;
            }
        }
        Ordering::Equal
    });
    Ok(ix)
}

pub(super) fn compare(s: &RSession, x: SexpId, ty: SexpType, i: usize, j: usize) -> Ordering {
    match ty {
        SexpType::Int => s
            .int(x)
            .map(|v| {
                let (a, b) = (v.get(i), v.get(j));
                if a == NA_INTEGER || b == NA_INTEGER {
                    Ordering::Equal
                } else {
                    a.cmp(&b)
                }
            })
            .unwrap_or(Ordering::Equal),
        SexpType::Lgl => s
            .lgl(x)
            .map(|v| v.get(i).raw().cmp(&v.get(j).raw()))
            .unwrap_or(Ordering::Equal),
        SexpType::Real => s
            .real(x)
            .ok()
            .and_then(|v| v.get(i).partial_cmp(&v.get(j)))
            .unwrap_or(Ordering::Equal),
        SexpType::Cplx => s
            .cplx(x)
            .ok()
            .and_then(|v| {
                let (a, b) = (v.get(i), v.get(j));
                a.re.partial_cmp(&b.re)
                    .zip(a.im.partial_cmp(&b.im))
                    .map(|(re, im)| re.then(im))
            })
            .unwrap_or(Ordering::Equal),
        SexpType::Raw => s
            .raw(x)
            .map(|v| v.get(i).cmp(&v.get(j)))
            .unwrap_or(Ordering::Equal),
        SexpType::Str => {
            let id = |i| {
                s.str_slice(x)
                    .ok()
                    .and_then(|v| v.get(i))
                    .copied()
                    .unwrap_or(CharId::NA)
            };
            s.char_bytes(id(i)).ok().cmp(&s.char_bytes(id(j)).ok())
        }
        _ => Ordering::Equal,
    }
}

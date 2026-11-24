use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, NA_INTEGER, RLogical, RSession, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi,
};

use crate::ops::attributes::{has_class, row_count};
use crate::ops::common::{
    attr, ch, chars, copy_attrs, element, error, flag, gather, list, names, set, set_names, string,
    tag, text, vector_limit,
};

#[derive(Clone, Copy)]
pub(super) struct Index {
    pub(super) pos: Option<usize>,
    pub(super) name: Option<CharId>,
}

impl Index {
    pub(super) fn position(pos: Option<usize>) -> Self {
        Self { pos, name: None }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum IndexPolicy {
    PadMissing,
    Extend,
    Strict,
}

pub(super) fn indices(
    ctx: &mut Ctx,
    call: &Call,
    sub: SexpId,
    n: usize,
    labels: &[CharId],
    policy: IndexPolicy,
) -> EvalResult<Vec<Index>> {
    use SexpType::*;
    if sub == SEXP_MISSING {
        vector_limit(ctx, call, n, size_of::<Index>())?;
        return Ok((0..n).map(|i| Index::position(Some(i))).collect());
    }
    let s = ctx.session();
    let ty = s.type_of(sub)?;
    let len = s.xlength(sub)?;
    vector_limit(ctx, call, len.max(n), size_of::<Index>())?;
    let s = ctx.session_mut();
    let index = match ty {
        Nil => vec![],
        Lgl => {
            let v = s.lgl(sub)?;
            if policy == IndexPolicy::Strict && v.len() > n {
                return Err(error(ctx, call, "(subscript) logical subscript too long"));
            }
            if v.is_empty() {
                return Ok(vec![]);
            }
            (0..n.max(v.len()))
                .filter_map(|i| match v.get(i % v.len()).get() {
                    Some(false) => None,
                    Some(true) => Some(Index::position(
                        (policy == IndexPolicy::Extend || i < n).then_some(i),
                    )),
                    None => Some(Index::position(None)),
                })
                .collect()
        }
        Int | Real => {
            let (v, _) = s.coerce(sub, Real)?;
            let v = s.real(v)?;
            let negative = v.iter().any(|v| v.trunc() < 0.0);
            if negative {
                if v.iter().any(|v| v.is_nan() || v.trunc() > 0.0) {
                    return Err(error(
                        ctx,
                        call,
                        "only 0's may be mixed with negative subscripts",
                    ));
                }
                let mut excluded = vec![false; n];
                for v in v
                    .iter()
                    .filter(|v| v.trunc() < 0.0 && -v < (n as f64 + 1.0))
                {
                    excluded[(-v as usize) - 1] = true;
                }
                excluded
                    .into_iter()
                    .enumerate()
                    .filter(|(_, v)| !v)
                    .map(|(i, _)| Index::position(Some(i)))
                    .collect()
            } else {
                v.iter()
                    .filter(|v| v.is_nan() || v.trunc() != 0.0)
                    .map(|v| {
                        let pos = (v.is_finite() && v >= 1.0 && v < usize::MAX as f64)
                            .then(|| (v as usize) - 1);
                        Index::position(match policy {
                            IndexPolicy::PadMissing => pos.filter(|&i| i < n),
                            IndexPolicy::Extend | IndexPolicy::Strict => pos,
                        })
                    })
                    .collect()
            }
        }
        Str => s
            .str_slice(sub)?
            .iter()
            .map(|&id| {
                let pos = if id == CharId::NA || id == CharId::BLANK {
                    None
                } else {
                    labels.iter().position(|&l| {
                        l != CharId::NA && s.char_bytes(l).ok() == s.char_bytes(id).ok()
                    })
                };
                Index {
                    pos,
                    name: (id != CharId::NA).then_some(id),
                }
            })
            .collect(),
        _ => {
            return Err(error(
                ctx,
                call,
                format!("invalid subscript type '{}'", ty.type_name(false)),
            ));
        }
    };
    if policy == IndexPolicy::Strict
        && index
            .iter()
            .any(|i| i.pos.is_some_and(|p| p >= n) || (i.pos.is_none() && i.name.is_some()))
    {
        return Err(error(ctx, call, "subscript out of bounds"));
    }
    Ok(index)
}

fn argument_indices(ctx: &Ctx, args: &Args, skip: &[&str]) -> Vec<SexpId> {
    args.iter()
        .skip(1)
        .filter(|(n, _)| tag(ctx.session(), *n).is_none_or(|name| !skip.contains(&name)))
        .map(|(_, v)| v)
        .collect()
}

pub(super) fn index_labels(s: &mut roxide_value::RSession, x: SexpId) -> EvalResult<Vec<CharId>> {
    if matches!(s.type_of(x)?, SexpType::Lang | SexpType::List) {
        let tags: Vec<_> = s.pairlist_iter(x).map(|(tag, _)| tag).collect();
        return tags
            .into_iter()
            .map(|sym| {
                let key = tag(s, sym).unwrap_or("").to_owned();
                Ok(ch(s, &key)?)
            })
            .collect();
    }
    Ok(names(s, x).to_vec())
}

pub(super) fn subset(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    subset_impl(ctx, call, args, args.get(0))
}

pub(super) fn subset_default(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut x = args.get(0);
    let s = ctx.session_mut();
    if s.type_of(x)? != SexpType::Env && (attr(s, x, "class") != SEXP_NIL || s.is_s4(x)?) {
        x = s.shallow_duplicate(x)?;
        set(s, x, "class", SEXP_NIL)?;
        s.set_s4_flag(x, false)?;
        ctx.protect(x);
    }
    subset_impl(ctx, call, args, x)
}

fn subset_impl(ctx: &mut Ctx, call: &Call, args: &Args, x: SexpId) -> EvalResult {
    if x == SEXP_NIL {
        return Ok(x);
    }
    let ty = ctx.session().type_of(x)?;
    let drop_arg = args.named(ctx.session(), "drop").unwrap_or(SEXP_MISSING);
    let subs = argument_indices(ctx, args, &["drop"]);
    let drop = flag(
        ctx,
        drop_arg,
        !has_class(ctx.session(), x, "data.frame") || subs.len() > 1,
    )?;
    if ty != SexpType::Env && has_class(ctx.session(), x, "data.frame") {
        return subset_frame(ctx, call, x, &subs, drop);
    }
    let s = ctx.session();
    let dim = attr(s, x, "dim");
    if subs.len() > 1 {
        if dim == SEXP_NIL {
            return Err(error(ctx, call, "incorrect number of dimensions"));
        }
        return subset_array(ctx, call, x, &subs, drop);
    }
    let sub = subs.first().copied().unwrap_or(SEXP_MISSING);
    if matches!(
        ty,
        SexpType::Clo | SexpType::Builtin | SexpType::Special | SexpType::Env
    ) {
        let name = ty.type_name(false);
        return Err(error(
            ctx,
            call,
            format!("object of type '{name}' is not subsettable"),
        ));
    }
    if sub == SEXP_MISSING && ty != SexpType::List {
        return Ok(x);
    }
    if dim != SEXP_NIL
        && attr(s, sub, "dim") != SEXP_NIL
        && matches!(
            s.type_of(sub)?,
            SexpType::Int | SexpType::Real | SexpType::Str
        )
    {
        let dims = s.int(dim)?.to_vec();
        if let Some(index) = coordinates(ctx, call, x, sub, &dims)? {
            return selected(ctx, x, &index);
        }
    }
    let s = ctx.session();
    let n = s.xlength(x)?;
    let labels = index_labels(ctx.session_mut(), x)?;
    let index = indices(ctx, call, sub, n, &labels, IndexPolicy::PadMissing)?;
    let pos: Vec<_> = index.iter().map(|i| i.pos).collect();
    if ty == SexpType::List {
        pairlist_as_list(ctx.session_mut(), x, &pos)
    } else {
        selected(ctx, x, &pos)
    }
}

pub(super) fn pairlist_as_list(s: &mut RSession, x: SexpId, pos: &[Option<usize>]) -> EvalResult {
    let entries: Vec<_> = s.pairlist_iter(x).collect();
    let selected: Vec<_> = pos
        .iter()
        .map(|i| i.and_then(|i| entries.get(i)).copied())
        .collect();
    let values: Vec<_> = selected
        .iter()
        .map(|entry| entry.map_or(SEXP_NIL, |(_, value)| value))
        .collect();
    let out = list(s, &values)?;
    if entries.iter().any(|(name, _)| name.is_some()) {
        let labels = selected
            .into_iter()
            .map(|entry| match entry {
                Some((name, _)) => {
                    let label = tag(s, name).unwrap_or("").to_owned();
                    ch(s, &label)
                }
                None => Ok(CharId::NA),
            })
            .collect::<Result<Vec<_>, _>>()?;
        set_names(s, out, &labels)?;
    }
    Ok(out)
}

pub(super) fn selected(ctx: &mut Ctx, x: SexpId, pos: &[Option<usize>]) -> EvalResult {
    let s = ctx.session_mut();
    let out = gather(s, x, pos)?;
    let labels = names(s, x);
    if attr(s, x, "names") != SEXP_NIL {
        let values: Vec<_> = pos
            .iter()
            .map(|i| i.and_then(|i| labels.get(i)).copied().unwrap_or(CharId::NA))
            .collect();
        set_names(s, out, &values)?;
    }
    if has_class(s, x, "factor") {
        copy_attrs(s, x, out, |n| matches!(n, "levels" | "class"))?;
        s.set_s4_flag(out, false)?;
    }
    Ok(out)
}

pub(super) fn coordinates(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    sub: SexpId,
    dims: &[i32],
) -> EvalResult<Option<Vec<Option<usize>>>> {
    let s = ctx.session_mut();
    let subdim = s.int(attr(s, sub, "dim"))?;
    if subdim.len() != 2 || subdim.get(1) as usize != dims.len() {
        return Ok(None);
    }
    let n = subdim.get(0) as usize;
    let is_str = s.type_of(sub)? == SexpType::Str;
    let sub = if is_str {
        sub
    } else {
        s.coerce(sub, SexpType::Real)?.0
    };
    let dn = attr(s, x, "dimnames");
    let on_axis: Vec<_> = (0..dims.len())
        .map(|i| s.list_get(dn, i).unwrap_or(SEXP_NIL))
        .collect();
    let mut out = Vec::new();
    for row in 0..n {
        let mut stride = 1;
        let mut position = Some(0);
        let mut skip = false;
        for (axis, &dim) in dims.iter().enumerate() {
            let index = row + axis * n;
            let v = if is_str {
                let id = s.str_slice(sub)?[index];
                if id == CharId::NA {
                    f64::NAN
                } else {
                    let found = chars(s, on_axis[axis]).iter().position(|&i| i == id);
                    let Some(found) = found else {
                        return Err(error(ctx, call, "subscript out of bounds"));
                    };
                    (found + 1) as f64
                }
            } else {
                s.real(sub)?.get(index).trunc()
            };
            if v == 0.0 {
                skip = true;
                break;
            }
            if v < 0.0 {
                return Err(error(
                    ctx,
                    call,
                    "negative values are not allowed in a matrix subscript",
                ));
            }
            if v > f64::from(dim) {
                return Err(error(ctx, call, "subscript out of bounds"));
            }
            if v.is_nan() {
                position = None;
                break;
            }
            position = position.map(|p| p + (v as usize - 1) * stride);
            stride *= dim as usize;
        }
        if !skip {
            out.push(position);
        }
    }
    Ok(Some(out))
}

pub(super) struct ArraySelection {
    pub(super) positions: Vec<Option<usize>>,
    axes: Vec<Vec<Option<usize>>>,
    labels: Vec<SexpId>,
}

pub(super) fn array_indices(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    subs: &[SexpId],
) -> EvalResult<ArraySelection> {
    let s = ctx.session();
    let dims = s.int(attr(s, x, "dim"))?.to_vec();
    if dims.len() != subs.len() {
        return Err(error(ctx, call, "incorrect number of dimensions"));
    }
    let dn = attr(s, x, "dimnames");
    let dn: Vec<_> = (0..dims.len())
        .map(|i| s.list_get(dn, i).unwrap_or(SEXP_NIL))
        .collect();
    let mut axes = Vec::with_capacity(dims.len());
    for ((&d, &sub), &labels) in dims.iter().zip(subs).zip(&dn) {
        let labels = chars(ctx.session(), labels).to_vec();
        let positions = indices(ctx, call, sub, d as usize, &labels, IndexPolicy::Strict)?;
        axes.push(positions.into_iter().map(|i| i.pos).collect::<Vec<_>>());
    }
    let count = axes
        .iter()
        .fold(1usize, |n, axis| n.saturating_mul(axis.len()));
    vector_limit(ctx, call, count, size_of::<Index>())?;
    let positions = (0..count)
        .map(|mut i| {
            let mut stride = 1;
            let mut result = 0;
            for (axis, &dim) in axes.iter().zip(&dims) {
                result += axis[i % axis.len()]? * stride;
                i /= axis.len();
                stride *= dim as usize;
            }
            Some(result)
        })
        .collect();
    Ok(ArraySelection {
        positions,
        axes,
        labels: dn,
    })
}

fn subset_array(ctx: &mut Ctx, call: &Call, x: SexpId, subs: &[SexpId], drop: bool) -> EvalResult {
    let selection = array_indices(ctx, call, x, subs)?;
    let out = selected(ctx, x, &selection.positions)?;
    let mut axes: Vec<_> = selection
        .axes
        .iter()
        .enumerate()
        .filter(|(_, v)| !drop || v.len() != 1)
        .map(|(i, _)| i)
        .collect();
    let s = ctx.session_mut();
    if drop && axes.is_empty() {
        let named: Vec<_> = selection
            .labels
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != SEXP_NIL)
            .map(|(i, _)| i)
            .collect();
        if named.len() == 1 {
            axes = named;
        }
    }
    if drop && axes.len() < 2 {
        if let Some(&i) = axes.first()
            && selection.labels[i] != SEXP_NIL
        {
            let n = gather(s, selection.labels[i], &selection.axes[i])?;
            set(s, out, "names", n)?;
        }
        return Ok(out);
    }
    let dims: Vec<_> = axes
        .iter()
        .map(|&i| selection.axes[i].len() as i32)
        .collect();
    let dim = s.alloc_int_from(&dims)?;
    set(s, out, "dim", dim)?;
    if axes.iter().any(|&i| selection.labels[i] != SEXP_NIL) {
        let mut labels = Vec::new();
        for i in axes {
            labels.push(gather(s, selection.labels[i], &selection.axes[i])?);
        }
        let value = list(s, &labels)?;
        set(s, out, "dimnames", value)?;
    }
    Ok(out)
}

fn subset_frame(ctx: &mut Ctx, call: &Call, x: SexpId, subs: &[SexpId], drop: bool) -> EvalResult {
    let (rows, cols) = match subs {
        [] => (None, SEXP_MISSING),
        [cols] => (None, *cols),
        [rows, cols] => (Some(*rows), *cols),
        _ => return Err(error(ctx, call, "incorrect number of dimensions")),
    };
    let s = ctx.session();
    let labels = names(s, x).to_vec();
    let n = s.xlength(x)?;
    let ix = indices(ctx, call, cols, n, &labels, IndexPolicy::PadMissing)?;
    let Some(positions) = ix.iter().map(|i| i.pos).collect::<Option<Vec<_>>>() else {
        return Err(error(ctx, call, "undefined columns selected"));
    };
    let s = ctx.session_mut();
    let actual_rows = s.attr_row_names(x)?;
    let nrow = row_count(s, x)?;
    let row_labels = s.coerce(actual_rows, SexpType::Str)?.0;
    let mut row_ids = None;
    if let Some(rows) = rows.filter(|&v| v != SEXP_MISSING) {
        let labels = chars(ctx.session(), row_labels).to_vec();
        let idx = indices(ctx, call, rows, nrow, &labels, IndexPolicy::PadMissing)?;
        row_ids = Some(idx.iter().map(|i| i.pos).collect::<Vec<_>>());
    }
    let mut out = Vec::new();
    for &i in &positions {
        let col = ctx.session().list_get(x, i)?;
        out.push(if let Some(ref ids) = row_ids {
            selected(ctx, col, ids)?
        } else {
            col
        });
    }
    if drop && out.len() == 1 {
        return Ok(out[0]);
    }
    let s = ctx.session_mut();
    let result = list(s, &out)?;
    let new_names = positions
        .iter()
        .map(|&i| labels.get(i).copied().unwrap_or(CharId::BLANK))
        .collect::<Vec<_>>();
    set_names(s, result, &new_names)?;
    set(s, result, "class", attr(s, x, "class"))?;
    let row_names = if let Some(ref ids) = row_ids {
        let value = gather(s, actual_rows, ids)?;
        if ids.iter().any(Option::is_none) || duplicates(ids) {
            let (value, _) = s.coerce(value, SexpType::Str)?;
            let labels = chars(s, value)
                .iter()
                .map(|&id| text(s, id))
                .collect::<Vec<_>>();
            let labels = super::containers::unique_names(labels);
            let labels = labels
                .iter()
                .map(|v| ch(s, v))
                .collect::<Result<Vec<_>, _>>()?;
            s.alloc_str_from(&labels)?
        } else {
            value
        }
    } else {
        attr(s, x, "row.names")
    };
    set(s, result, "row.names", row_names)?;
    Ok(result)
}

fn duplicates<T: Copy + std::hash::Hash + Eq>(values: &[T]) -> bool {
    let mut seen = std::collections::HashSet::new();
    values.iter().any(|&v| !seen.insert(v))
}

pub(super) fn lookup(
    s: &RSession,
    names: &[CharId],
    key: &str,
    partial: bool,
) -> (Option<usize>, Option<String>) {
    if key.is_empty() {
        return (None, None);
    }
    if let Some(i) = names
        .iter()
        .position(|&v| v != CharId::NA && s.char_bytes(v).ok() == Some(key.as_bytes()))
    {
        return (Some(i), None);
    }
    if partial {
        let mut candidates = names.iter().enumerate().filter(|(_, v)| {
            **v != CharId::NA
                && s.char_bytes(**v)
                    .is_ok_and(|v| v.starts_with(key.as_bytes()))
        });
        if let Some((i, &v)) = candidates.next()
            && candidates.next().is_none()
        {
            return (Some(i), Some(text(s, v)));
        }
    }
    (None, None)
}

pub(super) fn single(
    ctx: &mut Ctx,
    call: &Call,
    sub: SexpId,
    i: usize,
    exact: RLogical,
    labels: &[CharId],
    n: usize,
) -> EvalResult<Option<usize>> {
    let s = ctx.session();
    match s.type_of(sub)? {
        SexpType::Str => {
            let id = s.str_slice(sub)?[i];
            if id == CharId::NA {
                return Ok(None);
            }
            let key = text(s, id);
            let (pos, matched) = lookup(s, labels, &key, exact != RLogical::TRUE);
            if let Some(matched) = matched
                && exact == RLogical::NA
            {
                ctx.warn_now(
                    call.err_call(),
                    format!("partial match of '{key}' to '{matched}'"),
                )?;
            }
            Ok(pos)
        }
        SexpType::Int | SexpType::Lgl | SexpType::Real => {
            let v = match s.type_of(sub)? {
                SexpType::Real => s.real(sub)?.get(i),
                SexpType::Lgl => s
                    .lgl(sub)?
                    .get(i)
                    .get()
                    .map_or(f64::NAN, |v| f64::from(u8::from(v))),
                _ => {
                    let v = s.int(sub)?.get(i);
                    if v == NA_INTEGER {
                        f64::NAN
                    } else {
                        f64::from(v)
                    }
                }
            };
            if v.is_nan() {
                return Ok(None);
            }
            let v = v.trunc();
            if v == 0.0 || (v < 0.0 && n < 2) {
                return Err(error(
                    ctx,
                    call,
                    "attempt to select less than one element in get1index",
                ));
            }
            if v < 0.0 {
                if n == 2 && v >= -2.0 {
                    return Ok(Some(2 - (-v as usize)));
                }
                return Err(error(
                    ctx,
                    call,
                    "attempt to select more than one element in get1index",
                ));
            }
            if v > n as f64 {
                return Err(error(ctx, call, "subscript out of bounds"));
            }
            Ok(Some(v as usize - 1))
        }
        ty => Err(error(
            ctx,
            call,
            format!("invalid subscript type '{}'", ty.type_name(false)),
        )),
    }
}

pub(super) fn extract(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    extract_impl(ctx, call, args, true)
}

pub(super) fn extract_default(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    extract_impl(ctx, call, args, false)
}

fn extract_impl(ctx: &mut Ctx, call: &Call, args: &Args, frame_methods: bool) -> EvalResult {
    let mut x = args.get(0);
    if x == SEXP_NIL {
        return Ok(x);
    }
    let subs = argument_indices(ctx, args, &["exact", "drop"]);
    let exact = args.named(ctx.session(), "exact").unwrap_or(SEXP_MISSING);
    let exact = if exact == SEXP_MISSING {
        RLogical::TRUE
    } else {
        let s = ctx.session_mut();
        let (v, _) = s.coerce(exact, SexpType::Lgl)?;
        s.lgl(v)?.iter().next().unwrap_or(RLogical::NA)
    };
    let Some(&sub) = subs.first() else {
        return Err(error(ctx, call, "no index specified"));
    };
    if sub == SEXP_MISSING {
        return Err(error(ctx, call, "subscript is missing"));
    }
    if ctx.session().type_of(x)? == SexpType::Env && subs.len() == 1 {
        let s = ctx.session();
        if let Ok([key]) = s.str_slice(sub) {
            return env_lookup(ctx, x, &text(s, *key));
        }
        return Err(error(
            ctx,
            call,
            "wrong arguments for subsetting an environment",
        ));
    }
    if frame_methods && subs.len() == 2 && has_class(ctx.session(), x, "data.frame") {
        let value = subset_frame(ctx, call, x, &subs, true)?;
        if ctx.session().xlength(value)? != 1 {
            return Err(error(ctx, call, "subscript out of bounds"));
        }
        return Ok(element(ctx.session_mut(), value, 0)?);
    }
    if subs.len() > 1 {
        if attr(ctx.session(), x, "dim") == SEXP_NIL {
            return Err(error(ctx, call, "incorrect number of subscripts"));
        }
        if subs
            .iter()
            .any(|&v| ctx.session().xlength(v).ok() != Some(1))
        {
            return Err(error(
                ctx,
                call,
                "attempt to select more than one element in get1index",
            ));
        }
        let selection = array_indices(ctx, call, x, &subs)?;
        if let [Some(i)] = selection.positions.as_slice() {
            return Ok(element(ctx.session_mut(), x, *i)?);
        }
        return Err(error(ctx, call, "subscript out of bounds"));
    }
    let len = ctx.session().xlength(sub)?;
    if len == 0 {
        return Err(error(
            ctx,
            call,
            "attempt to select less than one element in get1index",
        ));
    }
    for i in 0..len {
        let s = ctx.session();
        let n = s.xlength(x)?;
        let ty = s.type_of(x)?;
        let labels = index_labels(ctx.session_mut(), x)?;
        let pos = single(ctx, call, sub, i, exact, &labels, n)?;
        let Some(pos) = pos else {
            if matches!(
                ty,
                SexpType::Vec | SexpType::List | SexpType::Lang | SexpType::Expr
            ) && i + 1 == len
            {
                return Ok(SEXP_NIL);
            }
            return Err(error(ctx, call, "subscript out of bounds"));
        };
        if i + 1 < len
            && !matches!(
                ty,
                SexpType::Vec | SexpType::List | SexpType::Lang | SexpType::Expr
            )
        {
            return Err(error(
                ctx,
                call,
                "attempt to select more than one element in get1index",
            ));
        }
        x = element(ctx.session_mut(), x, pos)?;
    }
    Ok(x)
}

pub(super) fn dollar_name(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<String> {
    let s = ctx.session();
    if let Ok(sym) = s.sym_name_of(x) {
        return Ok(sym.to_owned());
    }
    string(s, x).ok_or_else(|| error(ctx, call, "invalid subscript type"))
}

fn env_lookup(ctx: &mut Ctx, env: SexpId, key: &str) -> EvalResult {
    let sym = ctx.session_mut().intern(key)?;
    Ok(ctx.get_local(sym, env)?.unwrap_or(SEXP_NIL))
}

pub(super) fn dollar(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let key = dollar_name(ctx, call, args.get(1))?;
    let s = ctx.session();
    match s.type_of(x)? {
        SexpType::Nil => Ok(SEXP_NIL),
        SexpType::Env => env_lookup(ctx, x, &key),
        SexpType::Vec | SexpType::List | SexpType::Lang => {
            let labels = index_labels(ctx.session_mut(), x)?;
            let (pos, matched) = lookup(ctx.session(), &labels, &key, true);
            if let Some(matched) = matched
                && ctx.opts().warn_partial_match_dollar
            {
                ctx.warn_now(
                    call.err_call(),
                    format!("partial match of '{key}' to '{matched}'"),
                )?;
            }
            if let Some(i) = pos {
                Ok(element(ctx.session_mut(), x, i)?)
            } else {
                Ok(SEXP_NIL)
            }
        }
        _ => Err(error(ctx, call, "$ operator is invalid for atomic vectors")),
    }
}

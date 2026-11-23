use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, NA_INTEGER, RLogical, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use crate::ops::attributes::{has_class, row_count};
use crate::ops::common::{
    atomic, attr, ch, chars, concatenate, copy_attrs, element, error, gather, names, rank,
    set_names, string, tag, text, vector_limit,
};

use super::index::{
    Index, array_indices, coordinates, dollar_name, indices, lookup, selected, single,
};

fn replace_one_index(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    sub: SexpId,
    i: usize,
) -> EvalResult<Index> {
    let s = ctx.session();
    let n = s.xlength(x)?;
    if s.type_of(sub)? == SexpType::Str {
        let id = s.str_slice(sub)?[i];
        let key = text(s, id);
        return Ok(Index {
            pos: if id == CharId::NA {
                None
            } else {
                lookup(s, names(s, x), &key, false).0
            },
            name: (id != CharId::NA).then_some(id),
        });
    }
    let (idx, _) = ctx.session_mut().coerce(sub, SexpType::Real)?;
    let v = ctx.session().real(idx)?.get(i);
    if v >= 1.0 && v.is_finite() && v < usize::MAX as f64 {
        return Ok(Index::position(Some(v as usize - 1)));
    }
    let labels = names(ctx.session(), x).to_vec();
    Ok(Index::position(single(
        ctx,
        call,
        sub,
        i,
        RLogical::TRUE,
        &labels,
        n,
    )?))
}

pub(super) fn replace(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let x = ctx.session_mut().unshare(x)?;
    let (subs, value) = replacement_args(ctx, args);
    if has_class(ctx.session(), x, "data.frame") {
        return replace_frame(ctx, call, x, &subs, value);
    }
    let s = ctx.session();
    let n = s.xlength(x)?;
    let labels = names(s, x).to_vec();
    let matrix = attr(s, x, "dim") != SEXP_NIL;
    let ix = if subs.len() > 1 {
        if !matrix {
            return Err(error(ctx, call, "incorrect number of subscripts"));
        }
        array_indices(ctx, call, x, &subs)?
            .positions
            .into_iter()
            .map(Index::position)
            .collect()
    } else {
        let sub = subs.first().copied().unwrap_or(SEXP_MISSING);
        let coord = if matrix && attr(ctx.session(), sub, "dim") != SEXP_NIL {
            let s = ctx.session();
            let dims = s.int(attr(s, x, "dim"))?.to_vec();
            coordinates(ctx, call, x, sub, &dims)?
        } else {
            None
        };
        if let Some(pos) = coord {
            pos.into_iter().map(Index::position).collect()
        } else {
            indices(ctx, call, sub, n, &labels, !matrix, matrix)?
        }
    };
    assign(ctx, call, x, ix, value, false)
}

fn replacement_args(ctx: &Ctx, args: &Args) -> (Vec<SexpId>, SexpId) {
    let i = args
        .iter()
        .position(|(n, _)| tag(ctx.session(), n) == Some("value"))
        .unwrap_or(args.len().saturating_sub(1));
    let subs = args
        .values()
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(j, _)| *j != i)
        .map(|(_, &v)| v)
        .collect();
    (subs, args.get(i))
}

pub(super) fn replace_one(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = ctx.session_mut().unshare(args.get(0))?;
    let (subs, value) = replacement_args(ctx, args);
    let Some(&sub) = subs.first() else {
        return Err(error(ctx, call, "no index specified"));
    };
    if ctx.session().type_of(x)? == SexpType::Env {
        let Some(key) = string(ctx.session(), sub) else {
            return Err(error(ctx, call, "wrong args for environment subassignment"));
        };
        let sym = ctx.session_mut().intern(&key)?;
        ctx.assign(sym, value, x)?;
        return Ok(x);
    }
    if subs.len() == 2 && has_class(ctx.session(), x, "data.frame") {
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
        return replace_frame(ctx, call, x, &subs, value);
    }
    if subs.len() > 1 {
        if attr(ctx.session(), x, "dim") == SEXP_NIL {
            return Err(error(ctx, call, "incorrect number of subscripts"));
        }
        let selection = array_indices(ctx, call, x, &subs)?;
        if let [Some(i)] = selection.positions.as_slice() {
            return assign(ctx, call, x, vec![Index::position(Some(*i))], value, true);
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
    let mut target = x;
    let mut parents = Vec::new();
    for i in 0..len - 1 {
        let idx = replace_one_index(ctx, call, target, sub, i)?;
        let n = ctx.session().xlength(target)?;
        let Some(pos) = idx
            .pos
            .filter(|&p| p < n && ctx.session().type_of(target).ok() == Some(SexpType::Vec))
        else {
            return Err(error(
                ctx,
                call,
                format!("no such index at level {}", i + 1),
            ));
        };
        let next = ctx.session().list_get(target, pos)?;
        parents.push((target, idx));
        target = ctx.session_mut().unshare(next)?;
    }
    let index = replace_one_index(ctx, call, target, sub, len - 1)?;
    let mut out = replace_single_column_or_item(ctx, call, target, index, value)?;
    for (parent, idx) in parents.into_iter().rev() {
        out = assign(ctx, call, parent, vec![idx], out, true)?;
    }
    Ok(out)
}

pub(super) fn replace_dollar(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let x = ctx.session_mut().unshare(x)?;
    ctx.protect(x);
    let key = dollar_name(ctx, call, args.get(1))?;
    let value = args.get(2);
    let s = ctx.session_mut();
    let ty = s.type_of(x)?;
    if ty == SexpType::Env {
        let sym = s.intern(&key)?;
        ctx.assign(sym, value, x)?;
        return Ok(x);
    }
    let x = if atomic(ty) {
        ctx.warn_now(call.err_call(), "Coercing LHS to a list")?;
        ctx.session_mut().coerce(x, SexpType::Vec)?.0
    } else {
        x
    };
    let s = ctx.session_mut();
    let id = ch(s, &key)?;
    let pos = lookup(s, names(s, x), &key, false).0;
    replace_single_column_or_item(
        ctx,
        call,
        x,
        Index {
            pos,
            name: Some(id),
        },
        value,
    )
}

fn replace_single_column_or_item(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    index: Index,
    value: SexpId,
) -> EvalResult {
    let value = if value != SEXP_NIL && has_class(ctx.session(), x, "data.frame") {
        let rows = row_count(ctx.session_mut(), x)?;
        recycle_column(ctx, call, value, rows)?
    } else {
        value
    };
    assign(ctx, call, x, vec![index], value, true)
}

fn recycle_column(ctx: &mut Ctx, call: &Call, value: SexpId, rows: usize) -> EvalResult {
    let n = ctx.session().xlength(value)?;
    if n == rows {
        return Ok(value);
    }
    if n == 0 || n > rows || !rows.is_multiple_of(n) {
        return Err(error(
            ctx,
            call,
            format!("replacement has {n} rows, data has {rows}"),
        ));
    }
    vector_limit(ctx, call, rows, size_of::<Option<usize>>())?;
    selected(
        ctx,
        value,
        &(0..rows).map(|i| Some(i % n)).collect::<Vec<_>>(),
    )
}

fn replace_frame(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    subs: &[SexpId],
    value: SexpId,
) -> EvalResult {
    let (row_sub, col_sub) = match subs {
        [] => (SEXP_MISSING, SEXP_MISSING),
        [col] => (SEXP_MISSING, *col),
        [row, col] => (*row, *col),
        _ => return Err(error(ctx, call, "incorrect number of subscripts")),
    };
    let s = ctx.session_mut();
    let nrow = row_count(s, x)?;
    let ncol = s.xlength(x)?;
    let column_names = names(s, x).to_vec();
    let column_indices = indices(ctx, call, col_sub, ncol, &column_names, true, false)?;
    if column_indices.is_empty() {
        return Ok(x);
    }
    if column_indices
        .iter()
        .any(|v| v.pos.is_none() && v.name.is_none())
    {
        return Err(error(
            ctx,
            call,
            "missing values are not allowed in subscripted assignments of data frames",
        ));
    }
    if value == SEXP_NIL && row_sub == SEXP_MISSING {
        return assign(ctx, call, x, column_indices, value, false);
    }
    let row_indices = if row_sub == SEXP_MISSING {
        None
    } else {
        let s = ctx.session_mut();
        let labels = s.attr_row_names(x)?;
        let (labels, _) = s.coerce(labels, SexpType::Str)?;
        let labels = chars(s, labels).to_vec();
        Some(indices(ctx, call, row_sub, nrow, &labels, false, true)?)
    };
    let s = ctx.session_mut();
    let list_value = matches!(s.type_of(value)?, SexpType::Vec | SexpType::List);
    let size = s.xlength(value)?;
    if size == 0 {
        return Err(error(ctx, call, "replacement has length zero"));
    }
    let rows = row_indices.as_ref().map_or(nrow, Vec::len);
    let mut out = x;
    for (j, col) in column_indices.into_iter().enumerate() {
        if col
            .pos
            .is_some_and(|i| ctx.session().xlength(out).is_ok_and(|n| i > n))
        {
            return Err(error(
                ctx,
                call,
                "new columns would leave holes after existing columns",
            ));
        }
        let v = if list_value {
            element(ctx.session_mut(), value, j % size)?
        } else if rows == 0 || (size <= rows && rows.is_multiple_of(size)) {
            value
        } else {
            if rows == 0 || !size.is_multiple_of(rows) {
                return Err(error(
                    ctx,
                    call,
                    format!("replacement has {size} items for {rows} rows"),
                ));
            }
            let start = j % (size / rows) * rows;
            gather(
                ctx.session_mut(),
                value,
                &(start..start + rows).map(Some).collect::<Vec<_>>(),
            )?
        };
        let col_value = if let Some(row_indices) = &row_indices {
            let current = if let Some(i) = col.pos.filter(|&i| i < ncol) {
                ctx.session().list_get(x, i)?
            } else {
                gather(ctx.session_mut(), v, &vec![None; nrow])?
            };
            assign(ctx, call, current, row_indices.clone(), v, false)?
        } else if v == SEXP_NIL {
            v
        } else {
            recycle_column(ctx, call, v, nrow)?
        };
        out = assign(ctx, call, out, vec![col], col_value, true)?;
        ctx.protect(out);
    }
    Ok(out)
}

fn assign(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    mut ix: Vec<Index>,
    value: SexpId,
    one: bool,
) -> EvalResult {
    ctx.protect(x);
    ctx.protect(value);
    let factor = has_class(ctx.session(), x, "factor");
    let value = if factor && value != SEXP_NIL {
        factor_codes(ctx, call, x, value)?
    } else {
        value
    };
    ctx.protect(value);
    let s = ctx.session();
    let tx = s.type_of(x)?;
    let tv = s.type_of(value)?;
    if ix.is_empty() {
        return Ok(x);
    }
    let n = s.xlength(x)?;
    let size = s.xlength(value)?;
    if value == SEXP_NIL {
        if tx == SexpType::Nil {
            return Ok(x);
        }
        if matches!(tx, SexpType::Vec | SexpType::List) {
            let removed: std::collections::HashSet<_> = ix.iter().filter_map(|i| i.pos).collect();
            let keep: Vec<_> = (0..n).filter(|i| !removed.contains(i)).map(Some).collect();
            let out = selected(ctx, x, &keep)?;
            let s = ctx.session_mut();
            copy_attrs(s, x, out, |name| {
                !matches!(name, "names" | "dim" | "dimnames")
            })?;
            return Ok(out);
        }
    }
    let value_len = if one
        && (matches!(tx, SexpType::Vec | SexpType::List | SexpType::Expr)
            || !atomic(tv) && tv != SexpType::Nil)
    {
        1
    } else {
        size
    };
    if value_len == 0 {
        return Err(error(ctx, call, "replacement has length zero"));
    }
    if one && value_len > 1 {
        return Err(error(
            ctx,
            call,
            "more elements supplied than there are to replace",
        ));
    }
    if value_len > 1 && ix.iter().any(|v| v.pos.is_none() && v.name.is_none()) {
        return Err(error(
            ctx,
            call,
            "NAs are not allowed in subscripted assignments",
        ));
    }
    if one && ix.iter().any(|v| v.pos.is_none() && v.name.is_none()) {
        return Err(error(ctx, call, "subscript out of bounds"));
    }
    if !one && !ix.len().is_multiple_of(value_len) {
        if attr(ctx.session(), x, "dim") != SEXP_NIL {
            return Err(error(
                ctx,
                call,
                "number of items to replace is not a multiple of replacement length",
            ));
        }
        ctx.warn_now(
            call.err_call(),
            "number of items to replace is not a multiple of replacement length",
        )?;
    }
    let mut end = n;
    let mut added = std::collections::HashMap::new();
    for i in &mut ix {
        if i.pos.is_none()
            && let Some(name) = i.name
        {
            i.pos = Some(*added.entry(name).or_insert_with(|| {
                let i = end;
                end += 1;
                i
            }));
        }
        if let Some(i) = i.pos {
            end = end.max(i.saturating_add(1));
        }
    }
    if end != n && attr(ctx.session(), x, "dim") != SEXP_NIL {
        return Err(error(ctx, call, "subscript out of bounds"));
    }
    vector_limit(ctx, call, end, size_of::<SexpId>() * 2)?;
    let can_overwrite = ctx.can_overwrite(call);
    let s = ctx.session_mut();
    let target_ty = if rank(tx) >= rank(tv) { tx } else { tv };
    let ty = if atomic(target_ty) || target_ty == SexpType::Expr {
        target_ty
    } else {
        SexpType::Vec
    };
    if can_overwrite
        && end == n
        && ty == tx
        && value != SEXP_NIL
        && value != x
        && let Some(out) = overwrite(s, x, value, ty, &ix, value_len, one)?
    {
        return Ok(out);
    }
    let labels = names(s, x);
    let named = attr(s, x, "names") != SEXP_NIL || !added.is_empty();
    let mut labels = if named { labels.to_vec() } else { vec![] };
    if named {
        labels.resize(end, CharId::BLANK);
    }
    for index in &ix {
        if let (Some(pos), Some(id)) = (index.pos, index.name)
            && pos >= n
        {
            labels[pos] = id;
        }
    }

    let out = if atomic(ty) {
        let source = concatenate(s, &[x, value], ty)?;
        let mut positions: Vec<_> = (0..end).map(|i| (i < n).then_some(i)).collect();
        for (i, index) in ix.iter().enumerate() {
            if let Some(pos) = index.pos {
                positions[pos] = Some(n + i % value_len);
            }
        }
        gather(s, source, &positions)?
    } else {
        let mut values = Vec::with_capacity(end);
        for i in 0..end {
            values.push(if i < n { element(s, x, i)? } else { SEXP_NIL });
        }
        for (i, index) in ix.into_iter().enumerate() {
            if let Some(pos) = index.pos {
                values[pos] = if one {
                    value
                } else {
                    element(s, value, i % value_len)?
                };
            }
        }
        concatenate(s, &values, ty)?
    };
    copy_attrs(s, x, out, |n| n != "names")?;
    if named {
        set_names(s, out, &labels)?;
    }
    Ok(out)
}

fn overwrite(
    s: &mut roxide_value::RSession,
    target: SexpId,
    value: SexpId,
    ty: SexpType,
    indices: &[Index],
    value_len: usize,
    one: bool,
) -> Result<Option<SexpId>, roxide_value::ValueError> {
    let target = s.unshare(target)?;
    macro_rules! overwrite_atomic {
        ($read:ident, $write:ident) => {{
            let (value, _) = s.coerce(value, ty)?;
            let values = s.$read(value)?.to_vec();
            let dst = s.$write(target)?;
            for (i, index) in indices.iter().enumerate() {
                if let Some(pos) = index.pos {
                    dst[pos] = values[i % value_len];
                }
            }
        }};
    }
    match ty {
        SexpType::Lgl => overwrite_atomic!(lgl, lgl_mut),
        SexpType::Int => overwrite_atomic!(int, int_mut),
        SexpType::Real => overwrite_atomic!(real, real_mut),
        SexpType::Vec | SexpType::Expr if one => {
            for index in indices {
                if let Some(pos) = index.pos {
                    s.list_set(target, pos, value)?;
                }
            }
        }
        _ => return Ok(None),
    }
    Ok(Some(target))
}

fn factor_codes(ctx: &mut Ctx, call: &Call, x: SexpId, value: SexpId) -> EvalResult {
    let s = ctx.session_mut();
    let v = if has_class(s, value, "factor") {
        super::containers::factor_values(s, value)?
    } else {
        s.coerce(value, SexpType::Str)?.0
    };
    let levels = chars(s, attr(s, x, "levels"));
    let map: std::collections::HashMap<_, _> = levels
        .iter()
        .enumerate()
        .map(|(i, &v)| (v, (i + 1) as i32))
        .collect();
    let mut warn = false;
    let values: Vec<_> = chars(s, v)
        .iter()
        .map(|id| {
            if let Some(&code) = map.get(id) {
                code
            } else {
                warn |= *id != CharId::NA;
                NA_INTEGER
            }
        })
        .collect();
    if warn {
        ctx.warn_now(call.err_call(), "invalid factor level, NA generated")?;
    }
    Ok(ctx.session_mut().alloc_int_from(&values)?)
}

#[cfg(test)]
mod tests {
    use roxide_eval::{Ctx, EvalResult, Unwind};
    use roxide_gc::GcPolicy;
    use roxide_host::PureHost;
    use roxide_value::{GLOBAL_ENV, ValueApi};

    #[test]
    fn repeated_replacement_updates_an_unshared_vector_in_place() -> EvalResult<()> {
        for policy in [GcPolicy::default(), GcPolicy::Torture] {
            let mut ctx = Ctx::new(Box::new(PureHost::new()), policy).map_err(Unwind::Internal)?;
            crate::register_all(&mut ctx).map_err(Unwind::Internal)?;
            ctx.run_script("<test>", "x <- integer(20L)", GLOBAL_ENV, false)?;
            let name = ctx.session_mut().intern("x")?;
            let Some(original) = ctx.get_var(name, GLOBAL_ENV)? else {
                return Err(Unwind::TopLevel);
            };
            ctx.run_script("<test>", "for (i in 1:20) x[i] <- i", GLOBAL_ENV, false)?;
            assert_eq!(ctx.get_var(name, GLOBAL_ENV)?, Some(original));
            assert_eq!(ctx.session().heap.refcnt(original), 1);
            assert_eq!(
                ctx.session().int(original)?.to_vec(),
                (1..=20).collect::<Vec<_>>()
            );
        }
        Ok(())
    }
}

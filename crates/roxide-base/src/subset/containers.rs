use std::collections::{HashMap, HashSet};

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, NA_INTEGER, RSession, SEXP_MISSING, SEXP_NIL, SexpType, Sym, ValueApi};

use crate::ops::attributes::{has_class, row_count};
use crate::ops::common::{
    self, atomic, attr, ch, chars, concatenate, count, element, error, flag, formals, gather,
    names, number, rank, set, set_names, tag, text, vector_limit,
};
use crate::ops::types::is_missing;

use super::{index::selected, sequence::compare};

pub(super) fn list(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    vector_limit(ctx, call, args.len(), size_of::<SexpId>())?;
    let s = ctx.session_mut();
    let out = common::list(s, args.values())?;
    if args.iter().any(|(n, _)| n.is_some()) {
        let ns = args
            .iter()
            .map(|(n, _)| {
                let name = tag(s, n).unwrap_or("").to_owned();
                ch(s, &name)
            })
            .collect::<Result<Vec<_>, _>>()?;
        set_names(s, out, &ns)?;
    }
    Ok(out)
}

pub(super) fn pairlist(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let s = ctx.session_mut();
    let entries: Vec<_> = args.iter().collect();
    let mut out = SEXP_NIL;
    for (name, value) in entries.into_iter().rev() {
        out = s.cons(value, out, name.map_or(SEXP_NIL, Sym::as_sexp))?;
    }
    Ok(out)
}

pub(super) fn matrix(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [mut data, nr, nc, byrow, dn] = formals(
        ctx.session(),
        args,
        ["data", "nrow", "ncol", "byrow", "dimnames"],
    );
    let byrow = flag(ctx, byrow, false)?;
    if data == SEXP_MISSING {
        data = roxide_value::NA_LGL_SCALAR;
    }
    let s = ctx.session();
    let ty = s.type_of(data)?;
    if !(atomic(ty) || matches!(ty, SexpType::Vec | SexpType::Expr)) {
        return Err(error(
            ctx,
            call,
            format!(
                "'data' must be of a vector type, was '{}'",
                ty.type_name(false)
            ),
        ));
    }
    let n = s.xlength(data)?;
    let mut dim = [None; 2];
    for (i, v) in [nr, nc].into_iter().enumerate() {
        if v != SEXP_MISSING {
            let value = number(ctx.session_mut(), v)?;
            let length = count(ctx, call, value)?;
            if length > i32::MAX as usize {
                return Err(error(ctx, call, "invalid matrix extent"));
            }
            dim[i] = Some(length);
        }
    }
    let (nr, nc) = match dim {
        [None, None] => (n, 1),
        [Some(r), None] => (r, if r == 0 { 0 } else { n.div_ceil(r) }),
        [None, Some(c)] => (if c == 0 { 0 } else { n.div_ceil(c) }, c),
        [Some(r), Some(c)] => (r, c),
    };
    let total = nr.saturating_mul(nc);
    vector_limit(ctx, call, total, size_of::<Option<usize>>())?;
    if n > 0 && total == 0 {
        ctx.warn_now(call.err_call(), "non-empty data for zero-extent matrix")?;
    } else if n > 1 && !total.is_multiple_of(n) {
        let (name, axis) = if byrow { ("columns", nc) } else { ("rows", nr) };
        if axis != 0 && !n.is_multiple_of(axis) && !axis.is_multiple_of(n) {
            ctx.warn_now(call.err_call(), format!(
                "data length [{n}] is not a sub-multiple or multiple of the number of {name} [{axis}]"
            ))?;
        } else {
            let (name, axis) = if byrow { ("rows", nr) } else { ("columns", nc) };
            if axis != 0 && !n.is_multiple_of(axis) && !axis.is_multiple_of(n) {
                ctx.warn_now(call.err_call(), format!(
                    "data length [{n}] is not a sub-multiple or multiple of the number of {name} [{axis}]"
                ))?;
            } else {
                ctx.warn_now(
                    call.err_call(),
                    format!("data length differs from size of matrix: [{n} != {nr} x {nc}]"),
                )?;
            }
        }
    }
    let positions: Vec<_> = (0..total)
        .map(|i| {
            (n != 0).then(|| {
                if byrow {
                    ((i % nr) * nc + i / nr) % n
                } else {
                    i % n
                }
            })
        })
        .collect();
    let s = ctx.session_mut();
    let out = gather(s, data, &positions)?;
    attach_matrix(ctx, call, out, nr, nc, dn)
}

fn attach_matrix(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    nr: usize,
    nc: usize,
    dn: SexpId,
) -> EvalResult {
    if nr > i32::MAX as usize || nc > i32::MAX as usize {
        return Err(error(ctx, call, "invalid matrix extent"));
    }
    let s = ctx.session_mut();
    let dim = s.alloc_int_from(&[nr as i32, nc as i32])?;
    set(s, x, "dim", dim)?;
    if dn != SEXP_NIL && dn != SEXP_MISSING {
        if s.type_of(dn)? != SexpType::Vec || s.xlength(dn)? > 2 {
            return Err(error(
                ctx,
                call,
                "length of 'dimnames' must match that of 'dims'",
            ));
        }
        let mut labels = [SEXP_NIL; 2];
        let dims = [nr, nc];
        for i in 0..2 {
            let v = s.list_get(dn, i).unwrap_or(SEXP_NIL);
            if v == SEXP_NIL || s.xlength(v)? == 0 {
                continue;
            }
            if s.xlength(v)? != dims[i] {
                return Err(error(
                    ctx,
                    call,
                    format!("length of 'dimnames' [{}] not equal to array extent", i + 1),
                ));
            }
            labels[i] = s.coerce(v, SexpType::Str)?.0;
        }
        let val = common::list(s, &labels)?;
        set(s, val, "names", attr(s, dn, "names"))?;
        set(s, x, "dimnames", val)?;
    }
    Ok(x)
}

pub(super) fn rbind(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    bind(ctx, call, args, true)
}
pub(super) fn cbind(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    bind(ctx, call, args, false)
}

struct Block {
    x: SexpId,
    dim: Option<(usize, usize)>,
    name: Option<CharId>,
    size: usize,
}

pub(super) fn matrix_dims(
    s: &RSession,
    x: SexpId,
) -> Result<Option<(usize, usize)>, roxide_value::ValueError> {
    let dim = attr(s, x, "dim");
    if dim == SEXP_NIL {
        return Ok(None);
    }
    let v = s.int(dim)?;
    Ok((v.len() == 2).then(|| (v.get(0) as usize, v.get(1) as usize)))
}

fn bind(ctx: &mut Ctx, call: &Call, args: &Args, rows: bool) -> EvalResult {
    let s = ctx.session_mut();
    let mut ty = SexpType::Nil;
    let mut blocks = Vec::new();
    let mut fixed = None;
    let level = args
        .named(s, "deparse.level")
        .map_or(Ok(1.0), |x| number(s, x))?;
    let exprs = s.pairlist_iter(call.expr).skip(1).collect::<Vec<_>>();
    for (arg_index, (n, x)) in args.iter().enumerate() {
        if tag(s, n) == Some("deparse.level") || x == SEXP_NIL {
            continue;
        }
        let tx = s.type_of(x)?;
        let tx = if atomic(tx) { tx } else { SexpType::Vec };
        if rank(tx) > rank(ty) {
            ty = tx;
        }
        let dim = matrix_dims(s, x)?;
        if let Some((nr, nc)) = dim {
            let inner = if rows { nc } else { nr };
            if fixed.is_some_and(|v| v != inner) {
                return Err(error(
                    ctx,
                    call,
                    "number of rows or columns of matrices must match",
                ));
            }
            fixed = Some(inner);
        }
        let size = s.xlength(x)?;
        let name = if let Some(name) = tag(s, n) {
            Some(name.to_owned())
        } else if level > 0.0 && dim.is_none() {
            let expr = exprs.get(arg_index).map_or(SEXP_NIL, |(_, expr)| *expr);
            if let Ok(name) = s.sym_name_of(expr) {
                Some(name.to_owned())
            } else if level >= 2.0 && expr != SEXP_NIL {
                Some(roxide_value::deparse(s, expr, Default::default())?.join(" "))
            } else {
                None
            }
        } else {
            None
        };
        let name = name.map(|name| ch(s, &name)).transpose()?;
        blocks.push(Block { x, dim, name, size });
    }
    if ty == SexpType::Nil {
        return Ok(SEXP_NIL);
    }
    let inner = fixed.unwrap_or_else(|| blocks.iter().map(|v| v.size).max().unwrap_or(0));
    blocks.retain(|v| inner == 0 || v.size != 0 || v.dim.is_some());
    let width = |v: &Block| v.dim.map_or(1, |(r, c)| if rows { r } else { c });
    let outer = blocks.iter().map(width).sum::<usize>();
    vector_limit(
        ctx,
        call,
        inner.saturating_mul(outer),
        size_of::<SexpId>() * 2,
    )?;
    for (i, v) in blocks.iter().enumerate() {
        if v.dim.is_none() && v.size != 0 && !inner.is_multiple_of(v.size) {
            let side = if rows { "columns" } else { "rows" };
            ctx.warn_now(
                call.err_call(),
                format!(
                    "number of {side} of result is not a multiple of vector length (arg {})",
                    i + 1
                ),
            )?;
        }
    }
    let s = ctx.session_mut();
    let (nr, nc) = if rows { (outer, inner) } else { (inner, outer) };
    let mut data = vec![SEXP_NIL; nr * nc];
    let mut outer_names = Vec::new();
    let mut inner_names = SEXP_NIL;
    let mut named = false;
    let mut outer_start = 0;
    for block in &blocks {
        let outer_width = width(block);
        let dn = attr(s, block.x, "dimnames");
        let outer_axis = usize::from(!rows);
        let outer_labels = s.list_get(dn, outer_axis).unwrap_or(SEXP_NIL);
        if outer_labels != SEXP_NIL || block.name.is_some() {
            named = true;
        }
        if outer_labels != SEXP_NIL {
            outer_names.extend_from_slice(chars(s, outer_labels));
        } else {
            outer_names.extend(std::iter::repeat_n(
                block.name.unwrap_or(CharId::BLANK),
                outer_width,
            ));
        }
        if inner_names == SEXP_NIL {
            if block.dim.is_some() {
                inner_names = s.list_get(dn, 1 - outer_axis).unwrap_or(SEXP_NIL);
            } else if block.size == inner {
                inner_names = attr(s, block.x, "names");
            }
        }
        let block_data: Vec<_> = (0..block.size)
            .map(|i| element(s, block.x, i))
            .collect::<Result<_, _>>()?;
        let (block_rows, block_cols, row_start, col_start) = if rows {
            (outer_width, inner, outer_start, 0)
        } else {
            (inner, outer_width, 0, outer_start)
        };
        for col in 0..block_cols {
            let start = (col_start + col) * nr + row_start;
            for row in 0..block_rows {
                let i = if block.dim.is_some() {
                    col * block_rows + row
                } else {
                    (if rows { col } else { row }) % block.size
                };
                data[start + row] = block_data[i];
            }
        }
        outer_start += outer_width;
    }
    let x = concatenate(s, &data, ty)?;
    let outer_names = if named {
        s.alloc_str_from(&outer_names)?
    } else {
        SEXP_NIL
    };
    let axes = if rows {
        [outer_names, inner_names]
    } else {
        [inner_names, outer_names]
    };
    let dn = if axes == [SEXP_NIL; 2] {
        SEXP_NIL
    } else {
        common::list(s, &axes)?
    };
    attach_matrix(ctx, call, x, nr, nc, dn)
}

fn axis_count(ctx: &mut Ctx, x: SexpId, axis: usize) -> EvalResult {
    let s = ctx.session_mut();
    if has_class(s, x, "data.frame") {
        let n = if axis == 0 {
            row_count(s, x)?
        } else {
            s.xlength(x)?
        };
        return Ok(s.scalar_int(n as i32)?);
    }
    let dim = attr(s, x, "dim");
    if dim == SEXP_NIL {
        return Ok(SEXP_NIL);
    }
    let d = s.int(dim)?;
    if axis >= d.len() {
        return Ok(SEXP_NIL);
    }
    let n = d.get(axis);
    Ok(s.scalar_int(n)?)
}

pub(super) fn nrow(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    axis_count(ctx, args.get(0), 0)
}
pub(super) fn ncol(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    axis_count(ctx, args.get(0), 1)
}

pub(super) fn factor(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    make_factor(ctx, call, args, false)
}
pub(super) fn ordered(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    make_factor(ctx, call, args, true)
}

fn make_factor(ctx: &mut Ctx, call: &Call, args: &Args, ordered: bool) -> EvalResult {
    let [x, levels, labels, exclude, order, _] = formals(
        ctx.session(),
        args,
        ["x", "levels", "labels", "exclude", "ordered", "nmax"],
    );
    let x = if x == SEXP_MISSING { SEXP_NIL } else { x };
    let ordered = ordered || flag(ctx, order, has_class(ctx.session(), x, "ordered"))?;
    let s = ctx.session_mut();
    let source = if has_class(s, x, "factor") {
        factor_values(s, x)?
    } else {
        s.coerce(x, SexpType::Str)?.0
    };
    let source = s.str_slice(source)?.to_vec();
    let mut excluded = HashSet::new();
    if exclude == SEXP_MISSING {
        excluded.insert(CharId::NA);
    } else {
        let v = s.coerce(exclude, SexpType::Str)?.0;
        excluded.extend(chars(s, v));
    }
    let mut levels = if levels == SEXP_MISSING {
        let mut seen = HashSet::new();
        let mut distinct: Vec<_> = source
            .iter()
            .copied()
            .filter(|id| seen.insert(*id))
            .collect();
        if matches!(
            s.type_of(x)?,
            SexpType::Int | SexpType::Real | SexpType::Lgl
        ) && !has_class(s, x, "factor")
        {
            let original: HashMap<_, _> = source
                .iter()
                .copied()
                .enumerate()
                .map(|(i, v)| (v, i))
                .collect();
            let ty = s.type_of(x)?;
            distinct.sort_by(|a, b| match (*a == CharId::NA, *b == CharId::NA) {
                (false, false) => compare(s, x, ty, original[a], original[b]),
                (a, b) => a.cmp(&b),
            });
        } else {
            distinct.sort_by(|&a, &b| {
                (a == CharId::NA)
                    .cmp(&(b == CharId::NA))
                    .then_with(|| s.char_bytes(a).ok().cmp(&s.char_bytes(b).ok()))
            });
        }
        distinct
    } else {
        let v = s.coerce(levels, SexpType::Str)?.0;
        s.str_slice(v)?.to_vec()
    };
    levels.retain(|id| !excluded.contains(id));
    let mut map = HashMap::new();
    for (i, &id) in levels.iter().enumerate() {
        if map.insert(id, (i + 1) as i32).is_some() {
            return Err(error(
                ctx,
                call,
                format!("factor level [{}] is duplicated", i + 1),
            ));
        }
    }
    let mut codes: Vec<_> = source
        .iter()
        .map(|v| map.get(v).copied().unwrap_or(NA_INTEGER))
        .collect();
    if labels != SEXP_MISSING {
        let v = s.coerce(labels, SexpType::Str)?.0;
        let mut new = s.str_slice(v)?.to_vec();
        if new.len() == 1 && levels.len() != 1 {
            let prefix = text(s, new[0]);
            new = (1..=levels.len())
                .map(|i| ch(s, &format!("{prefix}{i}")))
                .collect::<Result<_, _>>()?;
        }
        if new.len() != levels.len() {
            return Err(error(
                ctx,
                call,
                format!(
                    "invalid 'labels'; length {} should be 1 or {}",
                    new.len(),
                    levels.len()
                ),
            ));
        }
        let mut labels = HashMap::new();
        levels.clear();
        let mut ids = Vec::new();
        for v in new {
            let pos = (levels.len() + 1) as i32;
            let id = *labels.entry(v).or_insert(pos);
            if id == pos {
                levels.push(v);
            }
            ids.push(id);
        }
        for code in &mut codes {
            if *code != NA_INTEGER {
                *code = ids[(*code - 1) as usize];
            }
        }
    }
    let out = s.alloc_int_from(&codes)?;
    let levels = s.alloc_str_from(&levels)?;
    let factor = ch(s, "factor")?;
    let classes = if ordered {
        vec![ch(s, "ordered")?, factor]
    } else {
        vec![factor]
    };
    let classes = s.alloc_str_from(&classes)?;
    set(s, out, "levels", levels)?;
    set(s, out, "class", classes)?;
    set(s, out, "names", attr(s, x, "names"))?;
    Ok(out)
}

pub(super) fn factor_values(s: &mut RSession, x: SexpId) -> EvalResult {
    let levels = chars(s, attr(s, x, "levels"));
    let labels = common::factor_labels(s, x, levels)?;
    Ok(s.alloc_str_from(&labels)?)
}

struct Column {
    name: String,
    value: SexpId,
    length: usize,
}

pub(super) fn data_frame(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let s = ctx.session();
    let row_names = args.named(s, "row.names").unwrap_or(SEXP_NIL);
    let check = args.named(s, "check.names").unwrap_or(SEXP_MISSING);
    let strings = args.named(s, "stringsAsFactors").unwrap_or(SEXP_MISSING);
    let inputs: Vec<_> = args
        .iter()
        .filter(|(n, _)| {
            !matches!(
                tag(s, *n),
                Some(
                    "row.names"
                        | "check.names"
                        | "check.rows"
                        | "stringsAsFactors"
                        | "fix.empty.names"
                )
            )
        })
        .collect();
    let check = flag(ctx, check, true)?;
    let strings = flag(ctx, strings, false)?;
    let mut columns = Vec::new();
    let call_args: Vec<_> = ctx
        .session()
        .cons_parts(call.expr)
        .ok()
        .map(|(_, args, _)| ctx.session().pairlist_iter(args).map(|(_, v)| v).collect())
        .unwrap_or_default();
    for (i, (name, mut x)) in inputs.into_iter().enumerate() {
        if x == SEXP_NIL {
            continue;
        }
        let s = ctx.session_mut();
        let name = tag(s, name).map(str::to_owned).unwrap_or_else(|| {
            call_args
                .get(i)
                .and_then(|&v| roxide_value::deparse(s, v, Default::default()).ok())
                .map_or_else(|| format!("V{}", i + 1), |v| v.join(" "))
        });
        let ty = s.type_of(x)?;
        if ty == SexpType::Vec {
            let nested = s.list_slice(x)?.to_vec();
            let labels = names(s, x).to_vec();
            for (j, value) in nested.into_iter().enumerate() {
                let suffix = labels
                    .get(j)
                    .filter(|&&v| v != CharId::BLANK && v != CharId::NA)
                    .map_or_else(|| format!("{}", j + 1), |&v| text(s, v));
                columns.push(Column {
                    name: format!("{name}.{suffix}"),
                    value,
                    length: s.xlength(value)?,
                });
            }
        } else if let Some((nr, nc)) = matrix_dims(s, x)? {
            let dn = attr(s, x, "dimnames");
            let labels = chars(s, s.list_get(dn, 1).unwrap_or(SEXP_NIL)).to_vec();
            for j in 0..nc {
                let p: Vec<_> = (j * nr..(j + 1) * nr).map(Some).collect();
                let value = gather(s, x, &p)?;
                let suffix = labels
                    .get(j)
                    .map_or_else(|| (j + 1).to_string(), |&v| text(s, v));
                columns.push(Column {
                    name: format!("{name}.{suffix}"),
                    value,
                    length: nr,
                });
            }
        } else {
            if ty == SexpType::Str && strings {
                x = make_factor(ctx, call, &Args::from_pairs(vec![(None, x)]), false)?;
            }
            columns.push(Column {
                name,
                value: x,
                length: ctx.session().xlength(x)?,
            });
        }
    }
    let rows = if columns.is_empty() && row_names != SEXP_NIL {
        ctx.session().xlength(row_names)?
    } else {
        columns.iter().map(|v| v.length).max().unwrap_or(0)
    };
    if columns
        .iter()
        .any(|v| v.length < rows && (v.length == 0 || !rows.is_multiple_of(v.length)))
    {
        let lengths = columns
            .iter()
            .map(|v| v.length.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(error(
            ctx,
            call,
            format!("arguments imply differing number of rows: {lengths}"),
        ));
    }
    vector_limit(
        ctx,
        call,
        rows.saturating_mul(columns.len()),
        size_of::<SexpId>(),
    )?;
    let mut vals = Vec::new();
    let mut label = Vec::new();
    for col in columns {
        let positions = (0..rows).map(|i| Some(i % col.length)).collect::<Vec<_>>();
        let value = selected(ctx, col.value, &positions)?;
        let s = ctx.session_mut();
        set(s, value, "names", SEXP_NIL)?;
        vals.push(value);
        label.push(if check {
            valid_name(&col.name)
        } else {
            col.name
        });
    }
    if check {
        label = unique_names(label);
    }
    let s = ctx.session_mut();
    let x = common::list(s, &vals)?;
    let labels = label
        .iter()
        .map(|v| ch(s, v))
        .collect::<Result<Vec<_>, _>>()?;
    set_names(s, x, &labels)?;
    let class = s.scalar_str("data.frame")?;
    set(s, x, "class", class)?;
    let row_names = if row_names == SEXP_NIL {
        if rows == 0 {
            s.alloc_int_from(&[])?
        } else {
            s.alloc_int_from(&[NA_INTEGER, -(rows as i32)])?
        }
    } else {
        let r = s.xlength(row_names)?;
        if r != rows {
            return Err(error(ctx, call, "invalid 'row.names' length"));
        }
        let values = s.coerce(row_names, SexpType::Str)?.0;
        let mut seen = HashSet::new();
        if s.str_slice(values)?
            .iter()
            .any(|&v| v == CharId::NA || !seen.insert(v))
        {
            return Err(error(ctx, call, "duplicate or missing row.names"));
        }
        if s.type_of(row_names)? == SexpType::Int {
            row_names
        } else {
            values
        }
    };
    set(s, x, "row.names", row_names)?;
    Ok(x)
}

fn valid_name(name: &str) -> String {
    let mut n = String::new();
    let mut lead = name.chars();
    if !lead.next().is_some_and(|ch| {
        ch.is_alphabetic() || (ch == '.' && !lead.next().is_some_and(|v| v.is_ascii_digit()))
    }) {
        n.push('X');
    }
    n.extend(name.chars().map(|v| {
        if v.is_alphanumeric() || matches!(v, '.' | '_') {
            v
        } else {
            '.'
        }
    }));
    if matches!(
        n.as_str(),
        "if" | "else"
            | "repeat"
            | "while"
            | "function"
            | "for"
            | "in"
            | "next"
            | "break"
            | "TRUE"
            | "FALSE"
            | "NULL"
            | "Inf"
            | "NaN"
            | "NA"
            | "NA_integer_"
            | "NA_real_"
            | "NA_complex_"
            | "NA_character_"
    ) {
        n.push('.');
    }
    n
}

pub(super) fn unique_names(labels: Vec<String>) -> Vec<String> {
    let mut used: HashSet<_> = labels.iter().cloned().collect();
    let mut seen = HashMap::new();
    labels
        .into_iter()
        .map(|label| {
            let count = seen.entry(label.clone()).or_insert(0);
            if *count == 0 {
                *count = 1;
                return label;
            }
            loop {
                let name = format!("{label}.{count}");
                *count += 1;
                if used.insert(name.clone()) {
                    return name;
                }
            }
        })
        .collect()
}

pub(super) fn rle(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let s = ctx.session();
    let ty = s.type_of(x)?;
    if !atomic(ty) || attr(s, x, "dim") != SEXP_NIL {
        return Err(error(ctx, call, "'x' must be a vector of an atomic type"));
    }
    let n = s.xlength(x)?;
    vector_limit(ctx, call, n, size_of::<Option<usize>>() + size_of::<i32>())?;
    let s = ctx.session_mut();
    let mut lengths = Vec::<i32>::new();
    let mut breaks = Vec::new();
    for i in 0..n {
        let equal = if i == 0 || is_missing(s, x, i) || is_missing(s, x, i - 1) {
            false
        } else if ty == SexpType::Cplx {
            s.cplx(x)?.get(i) == s.cplx(x)?.get(i - 1)
        } else {
            compare(s, x, ty, i, i - 1).is_eq()
        };
        if equal && let Some(current) = lengths.last_mut() {
            *current += 1;
        } else {
            lengths.push(1);
            breaks.push(Some(i));
        }
    }
    let lengths = s.alloc_int_from(&lengths)?;
    let values = gather(s, x, &breaks)?;
    let result = common::list(s, &[lengths, values])?;
    let names = [ch(s, "lengths")?, ch(s, "values")?];
    set_names(s, result, &names)?;
    let class = s.scalar_str("rle")?;
    set(s, result, "class", class)?;
    Ok(result)
}

pub(super) fn named_element(s: &RSession, x: SexpId, name: &str) -> SexpId {
    names(s, x)
        .iter()
        .position(|&v| s.char_bytes(v).ok() == Some(name.as_bytes()))
        .and_then(|i| s.list_get(x, i).ok())
        .unwrap_or(SEXP_NIL)
}

pub(super) fn inverse_rle(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let s = ctx.session();
    let lengths = named_element(s, x, "lengths");
    let values = named_element(s, x, "values");
    if lengths == SEXP_NIL || values == SEXP_NIL || s.xlength(lengths)? != s.xlength(values)? {
        return Err(error(ctx, call, "invalid 'rle' structure"));
    }
    let times = ctx.session_mut().intern("times")?;
    super::sequence::rep(
        ctx,
        call,
        &mut Args::from_pairs(vec![(None, values), (Some(times), lengths)]),
    )
}

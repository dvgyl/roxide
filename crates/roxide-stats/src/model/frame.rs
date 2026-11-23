use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, NA_INTEGER, RLogical, SEXP_NIL, SexpType, ValueApi};

use crate::distn::support::{attr, has_class, limit};

fn rows(ctx: &mut Ctx, x: SexpId) -> EvalResult<Option<usize>> {
    let dims = attr(ctx, x, "dim")?;
    if dims != SEXP_NIL {
        return Ok(ctx.session().int(dims)?.iter().next().map(|n| n as usize));
    }
    if has_class(ctx, x, "data.frame")? {
        let names = ctx.session_mut().attr_row_names(x)?;
        return Ok(Some(ctx.session().xlength(names)?));
    }
    if x == SEXP_NIL {
        return Ok(None);
    }
    Ok(Some(ctx.session().xlength(x)?))
}

fn scan(
    ctx: &mut Ctx,
    site: &Call,
    x: SexpId,
    counts: &mut Option<Vec<bool>>,
    outer: bool,
) -> EvalResult<()> {
    let kind = ctx.session().type_of(x)?;
    if kind == SexpType::Vec && outer {
        let frame = has_class(ctx, x, "data.frame")?;
        if !frame || ctx.session().xlength(x)? > 0 {
            for child in ctx.session().list_slice(x)?.to_vec() {
                scan(ctx, site, child, counts, false)?;
            }
            return Ok(());
        }
    }
    let empty_frame = kind == SexpType::Vec && outer;
    let Some(rows) = rows(ctx, x)? else {
        return Ok(());
    };
    if counts.as_ref().is_some_and(|values| values.len() != rows) {
        return Err(ctx.stop(site.err_call(), "not all arguments have the same length"));
    }
    limit::<RLogical>(ctx, site, rows)?;
    let values = counts.get_or_insert_with(|| vec![true; rows]);
    if rows == 0 {
        return Ok(());
    }
    let mut take = |i: usize, missing| {
        if missing {
            values[i % rows] = false;
        }
    };
    let s = ctx.session();
    match kind {
        SexpType::Lgl => s
            .lgl(x)?
            .iter()
            .map(|v| v.get().is_none())
            .enumerate()
            .for_each(|(i, v)| take(i, v)),
        SexpType::Int => s
            .int(x)?
            .iter()
            .map(|v| v == NA_INTEGER)
            .enumerate()
            .for_each(|(i, v)| take(i, v)),
        SexpType::Real => s
            .real(x)?
            .iter()
            .map(f64::is_nan)
            .enumerate()
            .for_each(|(i, v)| take(i, v)),
        SexpType::Cplx => s
            .cplx(x)?
            .iter()
            .map(|v| v.re.is_nan() || v.im.is_nan())
            .enumerate()
            .for_each(|(i, v)| take(i, v)),
        SexpType::Str => s
            .str_slice(x)?
            .iter()
            .map(|&v| v == CharId::NA)
            .enumerate()
            .for_each(|(i, v)| take(i, v)),
        SexpType::Vec if empty_frame => {}
        _ => {
            return Err(ctx.stop(
                site.err_call(),
                format!("invalid 'type' ({}) of argument", kind.type_name(false)),
            ));
        }
    }
    Ok(())
}

pub(super) fn complete_cases(ctx: &mut Ctx, site: &Call, args: &mut Args) -> EvalResult {
    let mut counts = None;
    for x in ctx.session().list_slice(args.get(0))?.to_vec() {
        scan(ctx, site, x, &mut counts, true)?;
    }
    let values = counts.ok_or_else(|| {
        ctx.stop(
            site.err_call(),
            "no input has determined the number of cases",
        )
    })?;
    let values = values
        .into_iter()
        .map(|x| RLogical::from_option(Some(x)))
        .collect::<Vec<_>>();
    Ok(ctx.session_mut().alloc_lgl_from(&values)?)
}

pub(super) fn row_names(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    Ok(ctx.session_mut().attr_row_names(args.get(0))?)
}

pub(super) fn predvar(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let coefs = attr(ctx, args.get(1), "coefs")?;
    if coefs == SEXP_NIL || ctx.session().type_of(x)? != SexpType::Lang {
        return Ok(x);
    }
    let mut fields = ctx.session().pairlist_iter(x);
    let Some((_, head)) = fields.next() else {
        return Ok(x);
    };
    let parts = fields
        .map(|(tag, x)| (tag.map_or(SEXP_NIL, |tag| tag.as_sexp()), x))
        .collect::<Vec<_>>();
    let coefs_tag = ctx.session_mut().intern("coefs")?.as_sexp();
    if parts.iter().any(|&(tag, _)| tag == coefs_tag) {
        return Ok(x);
    }
    let mut parts = parts;
    parts.push((coefs_tag, coefs));
    super::call(ctx.session_mut(), head, parts.into_iter())
}

pub(super) fn model_call(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let head = ctx.session().str_slice(args.get(0))?;
    let head = if let [id] = head {
        String::from_utf8_lossy(ctx.session().char_bytes(*id)?).into_owned()
    } else {
        return Ok(SEXP_NIL);
    };
    let values = ctx.session().list_slice(args.get(1))?.to_vec();
    super::lang(ctx.session_mut(), &head, &values)
}

pub(super) fn match_names(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let wanted = ctx.session().str_slice(args.get(0)).unwrap_or_default();
    let table = ctx.session().str_slice(args.get(1)).unwrap_or_default();
    let mut positions = std::collections::HashMap::new();
    for (i, &id) in table.iter().enumerate() {
        positions.entry(ctx.session().char_bytes(id)?).or_insert(i);
    }
    let mut out = Vec::with_capacity(wanted.len());
    for &id in wanted {
        let index = positions.get(ctx.session().char_bytes(id)?);
        out.push(
            index
                .and_then(|&i| i32::try_from(i + 1).ok())
                .unwrap_or(NA_INTEGER),
        );
    }
    Ok(ctx.session_mut().alloc_int_from(&out)?)
}

pub(super) fn unique_rows(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    use std::collections::{HashMap, HashSet};

    let original = args.get(0);
    if ctx.session().type_of(original)? == SexpType::Int {
        let mut seen = HashSet::new();
        if ctx
            .session()
            .int(original)?
            .iter()
            .all(|i| i != NA_INTEGER && seen.insert(i))
        {
            return Ok(original);
        }
    }
    let (converted, _) = ctx.session_mut().coerce(original, SexpType::Str)?;
    let names = ctx
        .session()
        .str_slice(converted)?
        .iter()
        .map(|&id| {
            if id == CharId::NA {
                Ok("NA".to_owned())
            } else {
                Ok(String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned())
            }
        })
        .collect::<EvalResult<Vec<_>>>()?;
    let reserved = names.iter().cloned().collect::<HashSet<_>>();
    let mut used = HashMap::new();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let next = used.entry(name.clone()).or_insert(0);
        if *next == 0 {
            out.push(name);
            *next = 1;
            continue;
        }
        loop {
            let candidate = format!("{name}.{next}");
            *next += 1;
            if !reserved.contains(&candidate) {
                out.push(candidate);
                break;
            }
        }
    }
    super::strings(ctx.session_mut(), out.iter().map(String::as_str))
}

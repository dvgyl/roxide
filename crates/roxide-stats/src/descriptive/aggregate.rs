use std::collections::BTreeMap;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{NA_INTEGER, ValueApi};

use crate::distn::support::limit;

pub(super) fn rows(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let columns = ctx.session().list_slice(args.get(0))?.to_vec();
    let len = match columns.first() {
        Some(&column) => ctx.session().xlength(column)?,
        None => 0,
    };
    limit::<i32>(ctx, call, len.saturating_mul(columns.len()))?;
    if len > i32::MAX as usize {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    let columns = columns
        .iter()
        .rev()
        .map(|&column| ctx.session().int(column))
        .collect::<Result<Vec<_>, _>>()?;
    if columns.iter().any(|column| column.len() != len) {
        return Err(ctx.stop(call.err_call(), "arguments must have same length"));
    }

    // Compare the last grouping column first. Keep input order within each group.
    let mut groups = BTreeMap::<Vec<i32>, Vec<i32>>::new();
    let mut key = Vec::with_capacity(columns.len());
    for row in 0..len {
        key.extend(columns.iter().map(|column| column.get(row)));
        if !key.contains(&NA_INTEGER) {
            groups
                .entry(key.clone())
                .or_default()
                .push((row + 1) as i32);
        }
        key.clear();
    }

    limit::<SexpId>(ctx, call, groups.len())?;
    let session = ctx.session_mut();
    let out = session.alloc_list(groups.len())?;
    for (i, group) in groups.into_values().enumerate() {
        let rows = session.alloc_int_from(&group)?;
        session.list_set(out, i, rows)?;
    }
    Ok(out)
}

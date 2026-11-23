//! Validate grob fields and name child lists.

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, SEXP_NIL, SexpType, ValueApi};

use super::fields::*;

pub(super) fn next_name(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    let prefix = text(ctx, args.get(1), 0)?.unwrap_or_default();
    let counter = args.get(2);
    if value != SEXP_NIL && !inherits(ctx, value, "grob")? {
        return Err(ctx.stop(call.err_call(), "invalid 'grob' argument"));
    }
    let class = attr(ctx, value, "class")?;
    let kind = text(ctx, class, 0)?.unwrap_or_else(|| "GROB".into());
    let n = num(ctx, counter, "grob", 0, 0.0)? as i32;
    let n = n.saturating_add(1);
    let count = ctx.session_mut().scalar_int(n)?;
    put(ctx, counter, "grob", count)?;
    Ok(ctx
        .session_mut()
        .scalar_str(&format!("{prefix}.{kind}.{n}"))?)
}

fn member(ctx: &mut Ctx, x: SexpId, name: &str, value: SexpId) -> EvalResult {
    let names = attr(ctx, x, "names")?;
    let mut keys = ctx.session().str_slice(names)?.to_vec();
    let mut values = ctx.session().list_slice(x)?.to_vec();
    let id = ctx
        .session_mut()
        .mk_char(name.as_bytes(), roxide_value::Encoding::Native)?;
    if let Some(i) = keys.iter().position(|key| *key == id) {
        values[i] = value;
    } else {
        keys.push(id);
        values.push(value);
    }
    let out = list(ctx, &values)?;
    let keys = ctx.session_mut().alloc_str_from(&keys)?;
    let class = attr(ctx, x, "class")?;
    attrib(ctx, out, "names", keys)?;
    attrib(ctx, out, "class", class)?;
    Ok(out)
}

pub(super) fn validate(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let mut name = get(ctx, x, "name")?;
    let auto = name == SEXP_NIL;
    if auto {
        let prefix = ctx.session_mut().scalar_str("GRID")?;
        name = next_name(
            ctx,
            call,
            &mut Args::from_pairs(vec![(None, x), (None, prefix), (None, args.get(1))]),
        )?;
    }
    if ctx.session().type_of(name)? != SexpType::Str
        || ctx.session().xlength(name)? != 1
        || text(ctx, name, 0)?.is_none()
    {
        return Err(ctx.stop(call.err_call(), "invalid grob name"));
    }
    let gp = get(ctx, x, "gp")?;
    if gp != SEXP_NIL && !inherits(ctx, gp, "gpar")? {
        return Err(ctx.stop(call.err_call(), "invalid 'gp' slot"));
    }
    let mut slots = vec!["vp"];
    if inherits(ctx, x, "gTree")? {
        slots.push("childrenvp");
    }
    for slot in slots {
        let vp = get(ctx, x, slot)?;
        if vp != SEXP_NIL
            && ctx.session().type_of(vp)? != SexpType::Str
            && !inherits(ctx, vp, "vpPath")?
            && !inherits(ctx, vp, "viewport")?
        {
            return Err(ctx.stop(call.err_call(), "invalid 'vp' slot"));
        }
    }
    if auto {
        member(ctx, x, "name", name)
    } else {
        Ok(x)
    }
}

pub(super) fn create_list(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let items = ctx.session().list_slice(args.get(0))?.to_vec();
    let names = attr(ctx, args.get(0), "names")?;
    let mut valid_items = Vec::new();
    let mut valid_names = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        if item == SEXP_NIL {
            continue;
        }
        if !inherits(ctx, item, "grob")? {
            return Err(ctx.stop(call.err_call(), "only 'grobs' allowed in \"gList\""));
        }
        valid_items.push(item);
        if names != SEXP_NIL {
            valid_names.push(ctx.session().str_slice(names)?[i]);
        }
    }
    let result = list(ctx, &valid_items)?;
    if !valid_items.is_empty() && names != SEXP_NIL {
        let names = ctx.session_mut().alloc_str_from(&valid_names)?;
        attrib(ctx, result, "names", names)?;
    }
    let class = strings(ctx, &["gList"])?;
    attrib(ctx, result, "class", class)?;
    Ok(result)
}

pub(super) fn children(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let children = args.get(1);
    if !inherits(ctx, children, "gList")? {
        return Err(ctx.stop(call.err_call(), "'children' must be a \"gList\""));
    }
    let items = ctx.session().list_slice(children)?.to_vec();
    let mut order = Vec::with_capacity(items.len());
    for &child in &items {
        let name = get(ctx, child, "name")?;
        let id = ctx
            .session()
            .str_slice(name)?
            .first()
            .copied()
            .unwrap_or(CharId::NA);
        order.push(id);
    }
    let order = ctx.session_mut().alloc_str_from(&order)?;
    let value = list(ctx, &items)?;
    if !items.is_empty() {
        attrib(ctx, value, "names", order)?;
    }
    let class = attr(ctx, children, "class")?;
    attrib(ctx, value, "class", class)?;
    let x = member(ctx, args.get(0), "children", value)?;
    member(ctx, x, "childrenOrder", order)
}

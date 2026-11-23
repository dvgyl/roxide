use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use super::metadata::{attr, flag, generic_definition, lookup, strings, texts};

const GROUPS: &[(&str, &[&str])] = &[
    ("Ops", &["Arith", "Compare", "Logic"]),
    ("Arith", &["+", "-", "*", "^", "%%", "%/%", "/"]),
    ("Compare", &["==", ">", "<", "!=", "<=", ">="]),
    ("Logic", &["&", "|"]),
    (
        "Math",
        &[
            "abs", "sign", "sqrt", "ceiling", "floor", "trunc", "cummax", "cummin", "cumprod",
            "cumsum", "exp", "expm1", "log", "log10", "log2", "log1p", "cos", "cosh", "sin",
            "sinh", "tan", "tanh", "acos", "acosh", "asin", "asinh", "atan", "atanh", "cospi",
            "sinpi", "tanpi", "gamma", "lgamma", "digamma", "trigamma",
        ],
    ),
    ("Math2", &["round", "signif"]),
    (
        "Summary",
        &["max", "min", "range", "prod", "sum", "any", "all"],
    ),
    ("Complex", &["Arg", "Conj", "Im", "Mod", "Re"]),
];

pub(super) fn members(name: &str) -> Option<&'static [&'static str]> {
    GROUPS
        .iter()
        .find_map(|&(group, members)| (group == name).then_some(members))
}

pub(super) fn parent(name: &str) -> Option<&'static str> {
    GROUPS
        .iter()
        .find_map(|&(group, members)| members.contains(&name).then_some(group))
}

pub(super) fn parents(name: &str) -> impl Iterator<Item = &'static str> {
    std::iter::successors(parent(name), |group| parent(group))
}

pub(super) fn group_names(ctx: &mut Ctx, name: &str) -> EvalResult {
    let list = ctx
        .session_mut()
        .alloc_list(usize::from(parent(name).is_some()))?;
    if let Some(parent) = parent(name) {
        let parent = ctx.session_mut().scalar_str(parent)?;
        ctx.session_mut().list_set(list, 0, parent)?;
    }
    Ok(list)
}

fn generic(ctx: &mut Ctx, value: roxide_gc::SexpId, env: roxide_gc::SexpId) -> EvalResult {
    if ctx.session().type_of(value)? == SexpType::Clo {
        return Ok(value);
    }
    let name = texts(ctx, value)?.into_iter().next().unwrap_or_default();
    match lookup(ctx, &name, "", env)? {
        Some(info) => generic_definition(ctx, &info, env),
        None => Ok(SEXP_NIL),
    }
}

pub(super) fn get_group(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let fun = generic(ctx, args.get(0), call.env)?;
    let groups = attr(ctx, fun, "group")?;
    let mut names = if ctx.session().type_of(groups)? == SexpType::Vec {
        let groups = ctx.session().list_slice(groups)?.to_vec();
        let mut names = Vec::new();
        for group in groups {
            names.extend(texts(ctx, group)?);
        }
        names
    } else {
        texts(ctx, groups)?
    };
    if flag(ctx, args.get(1)) {
        let mut i = 0;
        while i < names.len() {
            if let Some(group) = parent(&names[i])
                && !names.iter().any(|name| name == group)
            {
                names.push(group.to_owned());
            }
            i += 1;
        }
    }
    let list = ctx.session_mut().alloc_list(names.len())?;
    for (i, name) in names.iter().enumerate() {
        let value = ctx.session_mut().scalar_str(name)?;
        ctx.session_mut().list_set(list, i, value)?;
    }
    Ok(list)
}

pub(super) fn get_members(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let fun = generic(ctx, args.get(0), call.env)?;
    let name = attr(ctx, fun, "generic")?;
    let name = texts(ctx, name)?.into_iter().next().unwrap_or_default();
    let Some(direct) = members(&name) else {
        ctx.warn_now(call.err_call(), "character(0)")?;
        return strings(ctx, &[]);
    };
    let mut names = direct
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    if flag(ctx, args.get(1)) {
        let mut i = 0;
        while i < names.len() {
            if let Some(children) = members(&names[i]) {
                names.extend(children.iter().map(|name| (*name).to_owned()));
            }
            i += 1;
        }
    }
    if flag(ctx, args.get(2)) {
        return strings(ctx, &names);
    }
    let list = ctx.session_mut().alloc_list(names.len())?;
    for (i, name) in names.iter().enumerate() {
        let item = ctx.session_mut().scalar_str(name)?;
        ctx.session_mut().list_set(list, i, item)?;
    }
    Ok(list)
}

pub(super) fn is_group(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let fun = generic(ctx, args.get(0), args.get(1))?;
    let class = attr(ctx, fun, "class")?;
    let yes = texts(ctx, class)?
        .iter()
        .any(|class| class == "groupGenericFunction");
    Ok(if yes {
        roxide_value::TRUE_SCALAR
    } else {
        roxide_value::FALSE_SCALAR
    })
}

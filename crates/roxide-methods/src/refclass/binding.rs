use std::collections::HashSet;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, ValueApi};

use super::{
    RefEpoch, attr, class_definition, define, field_name, instance_meta, local, namespace,
    relations, set_attr, set_class, texts,
};
use crate::dispatch::metadata::{flag, strings};

pub(super) struct Method {
    owner: SexpId,
    fun: SexpId,
}

pub(super) fn find(
    ctx: &mut Ctx,
    meta: SexpId,
    key: &str,
    own: bool,
) -> EvalResult<Option<Method>> {
    let sym = ctx.session_mut().intern(key)?;
    let mut seen = HashSet::new();
    let mut pending = vec![(meta, own)];
    while let Some((owner, own)) = pending.pop() {
        if !seen.insert(owner) {
            continue;
        }
        let methods = local(ctx, owner, "methods")?;
        if own && let Some(fun) = ctx.get_local(sym, methods)? {
            return Ok(Some(Method { owner, fun }));
        }
        let parents = local(ctx, owner, "parents")?;
        pending.extend(
            ctx.session()
                .list_slice(parents)?
                .iter()
                .rev()
                .map(|&parent| (parent, true)),
        );
    }
    let ns = namespace(ctx);
    let methods = local(ctx, ns, ".ref_base_methods")?;
    let names = attr(ctx, methods, "names")?;
    let keys = texts(ctx, names)?;
    Ok(keys.iter().position(|name| name == key).and_then(|i| {
        ctx.session()
            .list_slice(methods)
            .ok()?
            .get(i)
            .copied()
            .map(|fun| Method {
                owner: SEXP_NIL,
                fun,
            })
    }))
}

pub(super) fn method_names(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    cached_method_names(ctx, args.get(0))
}

pub(super) fn cached_method_names(ctx: &mut Ctx, meta: SexpId) -> EvalResult {
    let epoch = ctx.ext::<RefEpoch>().0;
    let saved = local(ctx, meta, "methodEpoch")?;
    if saved != SEXP_NIL && ctx.session().real(saved)?.iter().next() == Some(epoch as f64) {
        return local(ctx, meta, "methodNames");
    }
    let ns = namespace(ctx);
    let methods = local(ctx, ns, ".ref_base_methods")?;
    let names = attr(ctx, methods, "names")?;
    let mut names = texts(ctx, names)?.into_iter().collect::<HashSet<_>>();
    let mut pending = vec![meta];
    let mut seen = HashSet::new();
    while let Some(meta) = pending.pop() {
        if !seen.insert(meta) {
            continue;
        }
        let methods = local(ctx, meta, "methods")?;
        for (name, _) in ctx.session().env_entries(methods)? {
            names.insert(ctx.session().sym_name(name)?.to_owned());
        }
        let parents = local(ctx, meta, "parents")?;
        pending.extend_from_slice(ctx.session().list_slice(parents)?);
    }
    let names = strings(ctx, &names.into_iter().collect::<Vec<_>>())?;
    let names = ctx.call_function("sort", &[(None, names)], ns)?;
    let epoch = ctx.session_mut().scalar_real(epoch as f64)?;
    define(ctx, meta, "methodEpoch", epoch)?;
    define(ctx, meta, "methodNames", names)?;
    Ok(names)
}

pub(super) fn lookup_method(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let key = field_name(ctx, args.get(1))?;
    let Some(method) = find(ctx, args.get(0), &key, true)? else {
        return Ok(SEXP_NIL);
    };
    let result = ctx.session_mut().alloc_list(2)?;
    ctx.session_mut().list_set(result, 0, method.owner)?;
    ctx.session_mut().list_set(result, 1, method.fun)?;
    let names = strings(ctx, &["owner".into(), "fun".into()])?;
    set_attr(ctx, result, "names", names)?;
    Ok(result)
}

pub(super) fn bind(ctx: &mut Ctx, object: SexpId, key: &str, method: Method) -> EvalResult {
    let ns = namespace(ctx);
    let data = attr(ctx, object, ".xData")?;
    let scope = ctx
        .session_mut()
        .alloc_env(if method.owner == SEXP_NIL { ns } else { data })?;
    let name = ctx.session_mut().scalar_str(key)?;
    if method.owner == SEXP_NIL {
        define(ctx, scope, ".self", object)?;
        let def = local(ctx, data, ".refClassDef")?;
        define(ctx, scope, ".refClassDef", def)?;
    } else {
        let template = local(ctx, ns, ".ref_super_binding")?;
        let (formals, body, _) = ctx.session().closure_parts(template)?;
        let super_scope = ctx.session_mut().alloc_env(ns)?;
        for (key, value) in [(".self", object), ("name", name), ("owner", method.owner)] {
            define(ctx, super_scope, key, value)?;
        }
        let call_super = ctx
            .session_mut()
            .alloc_closure(formals, body, super_scope)?;
        define(ctx, scope, "callSuper", call_super)?;
    }
    let (formals, body, _) = ctx.session().closure_parts(method.fun)?;
    let fun = ctx.session_mut().alloc_closure(formals, body, scope)?;
    for (tag, value) in ctx.session().attrs(method.fun)? {
        ctx.session_mut().set_attrib(fun, tag, value)?;
    }
    let class = if method.owner == SEXP_NIL {
        ctx.session_mut().scalar_str("envRefClass")?
    } else {
        local(ctx, method.owner, "name")?
    };
    let empty = strings(ctx, &[])?;
    let no_super = ctx.session_mut().scalar_str("")?;
    for (tag, value) in [
        ("mayCall", empty),
        ("name", name),
        ("refClassName", class),
        ("superClassMethod", no_super),
    ] {
        set_attr(ctx, fun, tag, value)?;
    }
    set_class(ctx, fun, "refMethodDef")?;
    Ok(fun)
}

pub(super) fn super_method(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    let key = field_name(ctx, args.get(1))?;
    let owner = args.get(2);
    if let Some(method) = find(ctx, owner, &key, false)? {
        return bind(ctx, object, &key, method);
    }
    if key == "initialize" {
        return Ok(SEXP_NIL);
    }
    let class = local(ctx, owner, "name")?;
    let class = field_name(ctx, class)?;
    Err(ctx.stop(SEXP_NIL, format!(
        "a call to superClass() is in the method ‘{key}’ but there is no superclass definition of this method for class “{class}”"
    )))
}

fn field_keys(ctx: &mut Ctx, meta: SexpId) -> EvalResult<Vec<String>> {
    let fields = local(ctx, meta, "fields")?;
    let names = attr(ctx, fields, "names")?;
    texts(ctx, names)
}

pub(super) fn initialize_fields(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    let data = attr(ctx, object, ".xData")?;
    let meta = instance_meta(ctx, data)?;
    let keys = field_keys(ctx, meta)?;
    let inputs = args.get(1);
    let names = attr(ctx, inputs, "names")?;
    let names = texts(ctx, names)?;
    let values = ctx.session().list_slice(inputs)?.to_vec();
    for (i, &value) in values.iter().enumerate() {
        if names.get(i).is_some_and(|name| !name.is_empty()) {
            continue;
        }
        if relations::matches(ctx, value, "envRefClass", data)? {
            let other = attr(ctx, value, ".xData")?;
            let other_meta = instance_meta(ctx, other)?;
            for key in field_keys(ctx, other_meta)?
                .iter()
                .filter(|key| keys.contains(key))
            {
                let sym = ctx.session_mut().intern(key)?;
                let value = ctx.get_var(sym, other)?.unwrap_or(SEXP_NIL);
                ctx.assign(sym, value, data)?;
            }
        } else {
            let class = relations::runtime_class(ctx, value)?;
            ctx.warn_now(SEXP_NIL, format!(
                "unnamed arguments to $new() must be objects from a reference class; got an object of class “{class}”"
            ))?;
        }
    }
    for (name, value) in names
        .iter()
        .zip(values)
        .filter(|(name, _)| !name.is_empty())
    {
        if !keys.contains(name) {
            let class = local(ctx, meta, "name")?;
            let class = field_name(ctx, class)?;
            return Err(ctx.stop(
                SEXP_NIL,
                format!("‘{name}’ is not a field in class “{class}”"),
            ));
        }
        let sym = ctx.session_mut().intern(name)?;
        ctx.assign(sym, value, data)?;
    }
    Ok(object)
}

pub(super) fn copy(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    let shallow = flag(ctx, args.get(1));
    let data = attr(ctx, object, ".xData")?;
    let def = local(ctx, data, ".refClassDef")?;
    let meta = attr(ctx, def, ".rx_ref_meta")?;
    let ns = namespace(ctx);
    let def = class_definition(ctx, meta)?;
    let result = ctx.call_function("new", &[(None, def)], ns)?;
    ctx.protect(result);
    let result_data = attr(ctx, result, ".xData")?;
    for name in field_keys(ctx, meta)? {
        let sym = ctx.session_mut().intern(&name)?;
        let mut value = ctx.get_var(sym, data)?.unwrap_or(SEXP_NIL);
        ctx.protect(value);
        if !shallow && relations::matches(ctx, value, "envRefClass", data)? {
            let key = ctx.session_mut().scalar_str("copy")?;
            let fun = ctx.call_function(".ref_member", &[(None, value), (None, key)], ns)?;
            value = ctx.apply(fun, &[(None, roxide_value::FALSE_SCALAR)], SEXP_NIL, ns)?;
        }
        ctx.assign(sym, value, result_data)?;
    }
    Ok(result)
}

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{EMPTY_ENV, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use super::metadata::{
    attr, bind, binding, canonical_generic_name, flag, formal_pairs, generic_definition,
    local_tables, logical, lookup, lookup_or_create_generic, package_string, set_attr, set_class,
    strings, table_name, text, texts,
};
use crate::registry::{GenericInfo, MethodsExt};

pub(super) const ANY: &str = "ANY";

pub(super) fn normalize(
    ctx: &mut Ctx,
    info: &GenericInfo,
    value: SexpId,
) -> EvalResult<Vec<String>> {
    let classes = texts(ctx, value)?;
    let names = attr(ctx, value, "names")?;
    let names = texts(ctx, names)?;
    let mut sig = vec![None; info.signature.len()];
    for (i, class) in classes
        .iter()
        .enumerate()
        .filter(|(i, _)| names.get(*i).is_some_and(|name| !name.is_empty()))
    {
        let name = &names[i];
        let Some(pos) = info.signature.iter().position(|param| param == name) else {
            return Err(ctx.stop(
                SEXP_NIL,
                format!("argument ‘{name}’ is not in the generic signature"),
            ));
        };
        if sig[pos].replace(class.clone()).is_some() {
            return Err(ctx.stop(SEXP_NIL, format!("duplicate signature argument ‘{name}’")));
        }
    }
    let mut free = sig.iter_mut().filter(|slot| slot.is_none());
    for (_, class) in classes
        .into_iter()
        .enumerate()
        .filter(|(i, _)| names.get(*i).is_none_or(String::is_empty))
    {
        let Some(slot) = free.next() else {
            return Err(ctx.stop(
                SEXP_NIL,
                "more classes than arguments in the generic signature",
            ));
        };
        *slot = Some(class);
    }
    Ok(sig
        .into_iter()
        .map(|slot| slot.unwrap_or_else(|| ANY.to_owned()))
        .collect())
}

pub(super) fn sig_width(sig: &[String]) -> usize {
    sig.iter().rposition(|cl| cl != ANY).map_or(1, |i| i + 1)
}

pub(super) fn method_key(sig: &[String], width: usize) -> String {
    if sig.is_empty() {
        ANY.to_owned()
    } else {
        sig[..width.min(sig.len())].join("#")
    }
}

#[derive(Clone)]
pub(super) struct Method {
    pub(super) signature: Vec<String>,
    pub(super) fun: SexpId,
}

pub(super) fn methods(ctx: &mut Ctx, info: &GenericInfo) -> EvalResult<Vec<Method>> {
    let mut methods = Vec::new();
    for (sym, _) in ctx.session().env_entries(info.table)? {
        let mut signature: Vec<_> = ctx
            .session()
            .sym_name(sym)?
            .split('#')
            .map(str::to_owned)
            .collect();
        signature.resize(info.signature.len(), ANY.to_owned());
        if let Some(fun) = ctx.get_local(sym, info.table)? {
            ctx.protect(fun);
            methods.push(Method { signature, fun });
        }
    }
    methods.sort_by(|a, b| a.signature.cmp(&b.signature));
    Ok(methods)
}

fn rewrite_width(
    ctx: &mut Ctx,
    table: SexpId,
    methods: impl IntoIterator<Item = Method>,
    old_width: usize,
    new_width: usize,
) -> EvalResult<()> {
    for method in methods {
        let previous = ctx
            .session_mut()
            .intern(&method_key(&method.signature, old_width))?;
        ctx.session_mut().env_remove(table, previous)?;
        bind(
            ctx,
            table,
            &method_key(&method.signature, new_width),
            method.fun,
        )?;
    }
    Ok(())
}

pub(super) fn signature_object(
    ctx: &mut Ctx,
    info: &GenericInfo,
    sig: &[String],
) -> EvalResult<SexpId> {
    let value = strings(ctx, sig)?;
    let names = strings(ctx, &info.signature[..sig.len()])?;
    set_attr(ctx, value, "names", names)?;
    let packages = sig
        .iter()
        .map(|class| {
            crate::registry::class_info(ctx, class, None)
                .map_or_else(|| "methods".to_owned(), |info| info.package)
        })
        .collect::<Vec<_>>();
    let packages = strings(ctx, &packages)?;
    set_attr(ctx, value, "package", packages)?;
    set_class(ctx, value, "signature")?;
    Ok(value)
}

pub(super) fn method_definition(
    ctx: &mut Ctx,
    info: &GenericInfo,
    defined: &[String],
    target: &[String],
    fun: SexpId,
    class: &str,
) -> EvalResult<SexpId> {
    if ctx.session().type_of(fun)? != SexpType::Clo {
        return Ok(fun);
    }
    let (formals, body, env) = ctx.session().closure_parts(fun)?;
    let fun = ctx.session_mut().alloc_closure(formals, body, env)?;
    let defined = signature_object(ctx, info, defined)?;
    let target = signature_object(ctx, info, target)?;
    let generic = package_string(ctx, &info.name, &info.package)?;
    for (key, value) in [
        ("target", target),
        ("defined", defined),
        ("generic", generic),
    ] {
        set_attr(ctx, fun, key, value)?;
    }
    set_class(ctx, fun, class)?;
    Ok(fun)
}

fn missing_defaults(ctx: &mut Ctx, generic: SexpId, definition: SexpId) -> EvalResult<SexpId> {
    let (_, body, env) = ctx.session().closure_parts(definition)?;
    let definition_formals = formal_pairs(ctx, definition)?;
    let generic_formals = formal_pairs(ctx, generic)?;
    let mut new = SEXP_NIL;
    let mut changed = false;
    for (tag, mut default) in definition_formals.into_iter().rev() {
        if default == SEXP_MISSING
            && let Some(&(_, inherited)) = generic_formals.iter().find(|&&(sym, _)| sym == tag)
        {
            changed |= inherited != SEXP_MISSING;
            default = inherited;
        }
        new = ctx.session_mut().cons(default, new, tag.as_sexp())?;
    }
    if changed {
        Ok(ctx.session_mut().alloc_closure(new, body, env)?)
    } else {
        Ok(definition)
    }
}

pub(super) fn local_table(ctx: &mut Ctx, info: &mut GenericInfo, where_: SexpId) -> EvalResult<()> {
    let key = table_name(&info.name, &info.package);
    if binding(ctx, where_, &key)?.is_none() {
        let table = ctx.session_mut().alloc_env(EMPTY_ENV)?;
        for (sym, value) in ctx.session().attrs(info.table)? {
            ctx.session_mut().set_attrib(table, sym, value)?;
        }
        if info.default_fun != SEXP_NIL {
            bind(ctx, table, "ANY", info.default_fun)?;
        }
        bind(ctx, where_, &key, table)?;
        info.table = table;
    } else if let Some(table) = binding(ctx, where_, &key)? {
        info.table = table;
    }
    Ok(())
}

pub(super) fn activate(ctx: &mut Ctx, info: &GenericInfo) -> EvalResult<()> {
    let active = logical(ctx, true)?;
    set_attr(ctx, info.table, ".rx_active", active)
}

pub(super) fn set_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut name = text(ctx, args.get(0))?;
    name = canonical_generic_name(&name).to_owned();
    let where_ = args.get(3);
    let sig_arg = args.get(1);
    let definition = args.get(2);
    let mut info = lookup_or_create_generic(ctx, &name, where_)?;
    let signature = normalize(ctx, &info, sig_arg)?;
    let generic = generic_definition(ctx, &info, where_)?;
    let definition = missing_defaults(ctx, generic, definition)?;
    let definition = method_definition(
        ctx,
        &info,
        &signature,
        &signature,
        definition,
        "MethodDefinition",
    )?;
    if flag(ctx, args.get(5)) {
        let sealed = logical(ctx, true)?;
        set_attr(ctx, definition, "sealed", sealed)?;
    }
    local_table(ctx, &mut info, where_)?;
    let old = methods(ctx, &info)?;
    let width = old
        .iter()
        .map(|m| sig_width(&m.signature))
        .max()
        .unwrap_or(1);
    let new_width = width.max(sig_width(&signature));
    let key = method_key(&signature, width);
    if new_width == width
        && let Some(prior) = binding(ctx, info.table, &key)?
    {
        let sealed = attr(ctx, prior, "sealed")?;
        if flag(ctx, sealed) {
            return Err(ctx.stop(call.err_call(), "the method is sealed"));
        }
    }
    if width != new_width {
        rewrite_width(ctx, info.table, old, width, new_width)?;
    }
    bind(
        ctx,
        info.table,
        &method_key(&signature, new_width),
        definition,
    )?;
    activate(ctx, &info)?;
    let registry = ctx.ext::<MethodsExt>();
    registry
        .generics
        .insert((name.clone(), info.package.clone()), info);
    registry.bump_epoch();
    Ok(ctx.session_mut().scalar_str(&name)?)
}

pub(super) fn remove_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = text(ctx, args.get(0))?;
    let name = canonical_generic_name(&name);
    let where_ = args.get(2);
    let Some(info) = local_tables(ctx, name, "", where_)?.into_iter().next() else {
        if lookup(ctx, name, "", where_)?.is_some() {
            return warn_missing_method(ctx, call, name, args.get(1));
        }
        ctx.warn_now(
            call.err_call(),
            format!("no generic function ‘{name}’ found"),
        )?;
        return logical(ctx, false);
    };
    let signature = normalize(ctx, &info, args.get(1))?;
    let old = methods(ctx, &info)?;
    let width = old
        .iter()
        .map(|m| sig_width(&m.signature))
        .max()
        .unwrap_or(1);
    if !old.iter().any(|method| method.signature == signature) {
        return warn_missing_method(ctx, call, name, args.get(1));
    }
    let key = method_key(&signature, width);
    if let Some(prior) = binding(ctx, info.table, &key)? {
        let sealed = attr(ctx, prior, "sealed")?;
        if flag(ctx, sealed) {
            return Err(ctx.stop(call.err_call(), "the method is sealed"));
        }
    }
    let key = ctx.session_mut().intern(&key)?;
    let removed = ctx.session_mut().env_remove(info.table, key)?;
    if removed {
        let new_width = old
            .iter()
            .filter(|m| m.signature != signature)
            .map(|m| sig_width(&m.signature))
            .max()
            .unwrap_or(1);
        if new_width != width {
            rewrite_width(
                ctx,
                info.table,
                old.into_iter().filter(|m| m.signature != signature),
                width,
                new_width,
            )?;
        }
        ctx.ext::<MethodsExt>().bump_epoch();
    }
    logical(ctx, removed)
}

fn warn_missing_method(ctx: &mut Ctx, call: &Call, name: &str, signature: SexpId) -> EvalResult {
    let given = texts(ctx, signature)?;
    let given = given
        .iter()
        .map(|class| roxide_value::encode_string(class.as_bytes()))
        .collect::<Vec<_>>()
        .join(", ");
    ctx.warn_now(
        call.err_call(),
        format!("no method found for function ‘{name}’ and signature {given}"),
    )?;
    logical(ctx, false)
}

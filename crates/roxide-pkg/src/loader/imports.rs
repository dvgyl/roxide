use std::collections::BTreeMap;
use std::path::PathBuf;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, TRUE_SCALAR, ValueApi};

use super::values::*;
use super::{env_names, export_names, exported, load};
use crate::description::Dependency;
use crate::library::{R_VERSION, check_dependency};
use crate::nsfile::{Import, NsSpec, Selection};

pub(super) fn populate(
    ctx: &mut Ctx,
    info: SexpId,
    imports: SexpId,
    spec: &NsSpec,
    paths: &[PathBuf],
    dependencies: &[Dependency],
) -> EvalResult<()> {
    let mut providers = vec![(String::from("base"), TRUE_SCALAR)];
    for (directives, prefix) in [
        (&spec.imports, ""),
        (&spec.import_classes, ".__C__"),
        (&spec.import_methods, ".__T__"),
    ] {
        for import in directives {
            let provider = load(ctx, &import.package, paths, SEXP_NIL, SEXP_NIL)?;
            let version = super::namespace_version(ctx, provider)?;
            for dependency in dependencies
                .iter()
                .filter(|dep| dep.package == import.package)
            {
                check_dependency(ctx, dependency, &version)?;
            }
            let mappings = import_from(ctx, imports, provider, import, prefix)?;
            record(ctx, &mut providers, &import.package, mappings)?;
        }
    }
    for dependency in dependencies.iter().filter(|dep| dep.package == "R") {
        check_dependency(ctx, dependency, R_VERSION)?;
    }
    store(ctx, info, &providers)
}

pub(super) fn import_from(
    ctx: &mut Ctx,
    target: SexpId,
    provider: SexpId,
    import: &Import,
    prefix: &str,
) -> EvalResult<Vec<(String, String)>> {
    let exports = export_names(ctx, provider)?;
    let bindings: Vec<_> = match &import.selection {
        Selection::All(except) => exports
            .iter()
            .filter(|name| !except.contains(name))
            .map(|name| (name.clone(), name.clone()))
            .collect(),
        Selection::Names(items) if prefix.is_empty() => items
            .iter()
            .map(|(alias, name)| {
                (
                    if alias.is_empty() {
                        name.clone()
                    } else {
                        alias.clone()
                    },
                    name.clone(),
                )
            })
            .collect(),
        Selection::Names(items) => {
            let mut selected = Vec::new();
            for (_, name) in items {
                let key = format!("{prefix}{name}");
                for export in &exports {
                    if export == &key
                        || (prefix == ".__T__"
                            && export
                                .strip_prefix(&key)
                                .is_some_and(|tail| tail.starts_with(':')))
                    {
                        selected.push((export.clone(), export.clone()));
                    }
                }
                if prefix == ".__T__" && exports.contains(name) {
                    selected.push((name.clone(), name.clone()));
                }
            }
            selected
        }
    };
    for (alias, name) in &bindings {
        let value = exported(ctx, provider, name)?;
        let sym = ctx.session_mut().intern(alias)?;
        ctx.assign(sym, value, target)?;
    }
    Ok(bindings)
}

pub(super) fn record(
    ctx: &mut Ctx,
    providers: &mut Vec<(String, SexpId)>,
    package: &str,
    bindings: Vec<(String, String)>,
) -> EvalResult<()> {
    if let Some((_, prior)) = providers.iter_mut().find(|(name, _)| name == package) {
        if *prior != TRUE_SCALAR {
            let labels = attr(ctx, *prior, "names")?;
            let labels = chars(ctx, labels)?;
            let sources = chars(ctx, *prior)?;
            let mut combined: Vec<_> = labels.into_iter().zip(sources).collect();
            for (alias, name) in bindings {
                if let Some(pair) = combined.iter_mut().find(|(key, _)| key == &alias) {
                    pair.1 = name;
                } else {
                    combined.push((alias, name));
                }
            }
            *prior = mapping_vector(ctx, &combined)?;
            ctx.protect(*prior);
        }
    } else {
        let value = mapping_vector(ctx, &bindings)?;
        ctx.protect(value);
        providers.push((package.to_owned(), value));
    }
    Ok(())
}

fn mapping_vector(ctx: &mut Ctx, mappings: &[(String, String)]) -> EvalResult<SexpId> {
    let names: Vec<_> = mappings.iter().map(|(name, _)| name.as_str()).collect();
    let sources = strings(ctx, mappings.iter().map(|(_, source)| source.as_str()))?;
    if names.is_empty() {
        return Ok(sources);
    }
    named(ctx, sources, &names)
}

pub(super) fn store(ctx: &mut Ctx, info: SexpId, providers: &[(String, SexpId)]) -> EvalResult<()> {
    let result = list(
        ctx,
        &providers
            .iter()
            .map(|(_, value)| *value)
            .collect::<Vec<_>>(),
    )?;
    named(
        ctx,
        result,
        &providers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
    )?;
    define(ctx, info, "imports", result)
}

pub(super) fn export(ctx: &mut Ctx, ns: SexpId, imports: SexpId, spec: &NsSpec) -> EvalResult<()> {
    let names = env_names(ctx, ns)?;
    let mut mappings: BTreeMap<String, String> = spec
        .exports
        .iter()
        .map(|name| (name.clone(), name.clone()))
        .collect();
    for name in &spec.export_classes {
        let key = format!(".__C__{name}");
        mappings.insert(key.clone(), key);
    }
    for name in &spec.export_methods {
        let key = format!(".__T__{name}");
        for object in &names {
            if object == name
                || object == &key
                || object
                    .strip_prefix(&key)
                    .is_some_and(|tail| tail.starts_with(':'))
            {
                mappings.insert(object.clone(), object.clone());
            }
        }
    }
    for (patterns, prefix) in [
        (&spec.export_patterns, ""),
        (&spec.export_class_patterns, ".__C__"),
    ] {
        let values = strings(
            ctx,
            names.iter().filter_map(|name| name.strip_prefix(prefix)),
        )?;
        ctx.protect(values);
        for regex in patterns {
            let pattern = ctx.session_mut().scalar_str(regex)?;
            let hits = base_call(ctx, "grep", &[pattern, values])?;
            for hit in ctx.session().int(hits)?.iter() {
                if let Some(item) = usize::try_from(hit)
                    .ok()
                    .and_then(|i| i.checked_sub(1))
                    .and_then(|i| names.iter().filter(|name| name.starts_with(prefix)).nth(i))
                {
                    mappings.insert(item.clone(), item.clone());
                }
            }
        }
    }
    let exports = ctx
        .ns_exports_env(ns)?
        .ok_or_else(|| ctx.stop(SEXP_NIL, "missing namespace export table"))?;
    let mut undefined = Vec::new();
    for (alias, target) in mappings {
        let sym = ctx.session_mut().intern(&target)?;
        if ctx.session().env_get(ns, sym).is_none() && ctx.session().env_get(imports, sym).is_none()
        {
            undefined.push(target);
            continue;
        }
        let mapped = ctx.session_mut().scalar_str(&target)?;
        define(ctx, exports, &alias, mapped)?;
    }
    if !undefined.is_empty() {
        return Err(ctx.stop(
            SEXP_NIL,
            format!("undefined exports: {}", undefined.join(", ")),
        ));
    }
    Ok(())
}

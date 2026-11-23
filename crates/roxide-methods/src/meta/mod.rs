//! Cache and inspect S4 metadata from namespace bindings.

use std::collections::{BTreeSet, HashMap, HashSet};

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, CtxExt, Dispatch, EvalResult, Module, PrimDef, PrimFn,
    PrimKind, Visibility,
};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi};

use crate::dispatch::metadata::{
    attr, canonical, environment_chain, flag, set_attr, set_class, strings, texts,
};
use crate::registry::{ClassInfo, GenericInfo, MethodsExt};

/// Register the S4 metadata operations in the methods namespace.
pub const MODULE: Module = Module {
    prims: &[
        prim(".rx_metaEnvironment", meta_environment, Visibility::On),
        prim(".rx_cacheMetaData", cache_metadata, Visibility::Off),
        prim(".rx_cacheGeneric", cache_generic, Visibility::On),
        prim(".rx_getClasses", get_classes, Visibility::On),
        prim(".rx_getGenerics", get_generics, Visibility::On),
        prim(
            ".rx_findMethodSignatures",
            method_signatures,
            Visibility::On,
        ),
        prim("packageSlot", package_slot, Visibility::On),
        prim("packageSlot<-", set_package_slot, Visibility::On),
        prim(".rx_S3Part", s3_part, Visibility::On),
    ],
    prelude: include_str!("prelude.R"),
};

const fn prim(name: &'static str, fun: PrimFn, visibility: Visibility) -> PrimDef {
    PrimDef {
        name,
        fun,
        visibility,
        args: ArgEval::Eval,
        arity: Arity::Any,
        dispatch: Dispatch::None,
        kind: PrimKind::Primitive,
    }
}

struct NamespaceMetadata {
    env: SexpId,
    tables: HashSet<SexpId>,
}

#[derive(Default)]
struct NamespaceCache(HashMap<String, NamespaceMetadata>);

impl Trace for NamespaceCache {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for entry in self.0.values() {
            tracer.edge(entry.env);
            for &table in &entry.tables {
                tracer.edge(table);
            }
        }
    }
}

impl CtxExt for NamespaceCache {}

fn forget(ctx: &mut Ctx, package: &str, env: SexpId) {
    let mut tables = ctx
        .ext::<NamespaceCache>()
        .0
        .remove(package)
        .map(|entry| entry.tables)
        .unwrap_or_default();
    if let Ok(entries) = ctx.session().env_entries(env) {
        tables.extend(entries.into_iter().filter_map(|(sym, binding)| {
            ctx.session()
                .sym_name(sym)
                .ok()?
                .starts_with(".__T__")
                .then_some(binding.value)
        }));
    }
    let registry = ctx.ext::<MethodsExt>();
    registry.classes.retain(|(_, owner), _| owner != package);
    registry
        .generics
        .retain(|(_, owner), info| owner != package && !tables.contains(&info.table));
    registry.bump_epoch();
}

/// Clear entries for unloaded namespaces. A detached namespace can remain loaded.
pub(crate) fn clear_unloaded(ctx: &mut Ctx) {
    if ctx.ext::<NamespaceCache>().0.is_empty() {
        return;
    }
    let loaded = ctx
        .ns_records()
        .iter()
        .map(|rec| rec.ns)
        .collect::<HashSet<_>>();
    let stale = ctx
        .ext::<NamespaceCache>()
        .0
        .iter()
        .filter(|(_, entry)| !loaded.contains(&entry.env))
        .map(|(name, entry)| (name.clone(), entry.env))
        .collect::<Vec<_>>();
    for (name, env) in stale {
        forget(ctx, &name, env);
    }
}

fn meta_bindings(ctx: &mut Ctx, env: SexpId, prefix: &str) -> EvalResult<Vec<(String, SexpId)>> {
    let mut found = Vec::new();
    for (sym, _) in ctx.session().env_entries(env)? {
        let Some(name) = ctx.session().sym_name(sym)?.strip_prefix(prefix) else {
            continue;
        };
        let name = name.to_owned();
        if let Some(value) = ctx.get_local(sym, env)? {
            ctx.protect(value);
            found.push((name, value));
        }
    }
    Ok(found)
}

fn pairs(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<(String, SexpId)>> {
    if value == SEXP_NIL {
        return Ok(Vec::new());
    }
    let names = attr(ctx, value, "names")?;
    Ok(texts(ctx, names)?
        .into_iter()
        .zip(ctx.session().list_slice(value)?.iter().copied())
        .collect())
}

fn class_info(ctx: &mut Ctx, def: SexpId, name: String) -> EvalResult<ClassInfo> {
    let package = attr(ctx, def, "package")?;
    let package = texts(ctx, package)?
        .into_iter()
        .next()
        .unwrap_or_else(|| ".GlobalEnv".into());
    if let Some(info) = ctx
        .ext::<MethodsExt>()
        .classes
        .get(&(name.clone(), package.clone()))
        && info.class_def == def
    {
        return Ok(info.clone());
    }
    let slots = attr(ctx, def, "slots")?;
    let slots = pairs(ctx, slots)?
        .into_iter()
        .map(|(name, class)| {
            Ok((
                name,
                texts(ctx, class)?.into_iter().next().unwrap_or_default(),
            ))
        })
        .collect::<EvalResult<_>>()?;
    let contains = attr(ctx, def, "contains")?;
    let superclasses = pairs(ctx, contains)?
        .into_iter()
        .enumerate()
        .map(|(rank, (parent, edge))| {
            let distance = attr(ctx, edge, "distance")?;
            let distance = if ctx.session().type_of(distance)? == SexpType::Int {
                ctx.session()
                    .int(distance)?
                    .iter()
                    .next()
                    .unwrap_or(1)
                    .max(1) as usize
            } else {
                ctx.session()
                    .real(distance)?
                    .iter()
                    .next()
                    .unwrap_or(1.0)
                    .max(1.0) as usize
            };
            Ok((parent, distance, rank))
        })
        .collect::<EvalResult<_>>()?;
    let virtual_ = attr(ctx, def, "virtual")?;
    Ok(ClassInfo {
        name,
        package,
        slots,
        superclasses,
        is_virtual: flag(ctx, virtual_),
        class_def: def,
    })
}

fn generic_tables(ctx: &mut Ctx, env: SexpId) -> EvalResult<Vec<GenericInfo>> {
    let mut tables = Vec::new();
    for (name, table) in meta_bindings(ctx, env, ".__T__")? {
        if let Some((name, package)) = name.rsplit_once(':')
            && ctx.session().type_of(table)? == SexpType::Env
        {
            tables.push(canonical(ctx, table, name, package, env)?);
        }
    }
    Ok(tables)
}

fn cache_metadata(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let env = args.get(0);
    let attach = args.len() < 2 || flag(ctx, args.get(1));
    let search = if args.len() >= 3 { args.get(2) } else { env };
    let package = ctx.namespace_name(env);
    if !attach && let Some(package) = package {
        forget(ctx, &package, env);
        return Ok(SEXP_NIL);
    }
    let mut classes = Vec::new();
    for (name, object) in meta_bindings(ctx, search, ".__C__")? {
        if ctx.session().is_s4(object)? {
            classes.push(class_info(ctx, object, name)?);
        }
    }
    let tables = generic_tables(ctx, search)?;
    if let Some(package) = package {
        let namespace = NamespaceMetadata {
            env,
            tables: tables.iter().map(|info| info.table).collect(),
        };
        ctx.ext::<NamespaceCache>().0.insert(package, namespace);
    }
    let registry = ctx.ext::<MethodsExt>();
    for info in classes {
        let key = (info.name.clone(), info.package.clone());
        if attach {
            registry.classes.insert(key, info);
        } else if registry
            .classes
            .get(&key)
            .is_some_and(|old| old.class_def == info.class_def)
        {
            registry.classes.remove(&key);
        }
    }
    for info in tables {
        update_generic(registry, info, attach);
    }
    registry.bump_epoch();
    Ok(SEXP_NIL)
}

fn update_generic(registry: &mut MethodsExt, info: GenericInfo, attach: bool) {
    let key = (info.name.clone(), info.package.clone());
    if attach {
        registry.generics.insert(key, info);
    } else if registry
        .generics
        .get(&key)
        .is_some_and(|old| old.table == info.table)
    {
        registry.generics.remove(&key);
    }
}

fn cache_generic(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let names = texts(ctx, args.get(1))?;
    let [name] = names.as_slice() else {
        return Err(ctx.stop(call.err_call(), "expected one generic name"));
    };
    let package = texts(ctx, args.get(4))?
        .into_iter()
        .next()
        .unwrap_or_default();
    let attach = flag(ctx, args.get(3));
    let table = args.get(5);
    let tables = if ctx.session().type_of(table)? == SexpType::Env {
        vec![canonical(ctx, table, name, &package, args.get(0))?]
    } else {
        generic_tables(ctx, args.get(0))?
    };
    let registry = ctx.ext::<MethodsExt>();
    for info in tables
        .into_iter()
        .filter(|info| info.name == *name && (package.is_empty() || info.package == package))
    {
        update_generic(registry, info, attach);
    }
    registry.bump_epoch();
    Ok(roxide_value::TRUE_SCALAR)
}

fn meta_environment(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    let search = ctx.search_path()?;
    let index = match ctx.session().type_of(value)? {
        SexpType::Env => return Ok(value),
        SexpType::Str => {
            let search_names = ctx.call_function("search", &[], call.env)?;
            let names = texts(ctx, search_names)?;
            let value = texts(ctx, value)?;
            names.iter().position(|name| Some(name) == value.first())
        }
        SexpType::Int => ctx
            .session()
            .int(value)?
            .iter()
            .next()
            .and_then(|pos| usize::try_from(pos).ok())
            .and_then(|pos| pos.checked_sub(1)),
        SexpType::Real => ctx
            .session()
            .real(value)?
            .iter()
            .next()
            .and_then(|pos| (pos as usize).checked_sub(1)),
        _ => None,
    };
    index
        .and_then(|index| search.get(index).copied())
        .ok_or_else(|| ctx.stop(call.err_call(), "invalid 'where' argument"))
}

fn get_classes(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    clear_unloaded(ctx);
    let where_ = args.get(0);
    let environments = if flag(ctx, args.get(1)) {
        environment_chain(ctx, where_)?
    } else {
        vec![where_]
    };
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for env in environments {
        let mut local = meta_bindings(ctx, env, ".__C__")?;
        local.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        names.extend(
            local
                .into_iter()
                .map(|(name, _)| name)
                .filter(|name| seen.insert(name.clone())),
        );
    }
    strings(ctx, &names)
}

fn get_generics(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    clear_unloaded(ctx);
    let where_ = args.get(0);
    let all = where_ == SEXP_NIL;
    let environments = if all {
        let mut envs = environment_chain(ctx, GLOBAL_ENV)?;
        envs.extend(ctx.ns_records().iter().map(|rec| rec.ns));
        envs
    } else {
        vec![where_]
    };
    let mut keys = BTreeSet::new();
    for env in environments {
        keys.extend(
            generic_tables(ctx, env)?
                .into_iter()
                .map(|info| (info.name, info.package)),
        );
    }
    let (names, packages): (Vec<_>, Vec<_>) = keys
        .into_iter()
        .map(|(name, package)| {
            (
                name,
                if flag(ctx, args.get(1)) && package != ".GlobalEnv" {
                    if package == "base"
                        || ctx
                            .ns_record(&package)
                            .is_some_and(|rec| rec.attached.is_some())
                    {
                        format!("package:{package}")
                    } else {
                        format!(":{package}")
                    }
                } else {
                    package
                },
            )
        })
        .unzip();
    let values = strings(ctx, &names)?;
    let packages = strings(ctx, &packages)?;
    let labels = if all { strings(ctx, &names)? } else { SEXP_NIL };
    if all {
        set_attr(ctx, packages, "names", labels)?;
    }
    set_attr(ctx, values, "package", packages)?;
    set_class(ctx, values, "ObjectsWithPackage")?;
    if all {
        set_attr(ctx, values, "names", labels)?;
    }
    Ok(values)
}

fn method_signatures(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let methods = args.get(0);
    let names = attr(ctx, methods, "names")?;
    let names = texts(ctx, names)?;
    let nrows = names.len();
    let arguments = attr(ctx, methods, "arguments")?;
    let arguments = texts(ctx, arguments)?;
    let width = names
        .iter()
        .map(|name| name.split('#').count())
        .max()
        .unwrap_or(1);
    let mut signatures = Vec::new();
    let target = flag(ctx, args.get(1));
    for (i, name) in names.iter().enumerate() {
        if !target {
            let fun = ctx.session().list_get(methods, i)?;
            let defined = attr(ctx, fun, "defined")?;
            if defined != SEXP_NIL {
                signatures.push(texts(ctx, defined)?);
                continue;
            }
        }
        signatures.push(name.split('#').map(str::to_owned).collect());
    }
    let rows =
        i32::try_from(nrows).map_err(|_| ctx.stop(SEXP_NIL, "too many method signatures"))?;
    let cols =
        i32::try_from(width).map_err(|_| ctx.stop(SEXP_NIL, "method signature is too wide"))?;
    let values = (0..width)
        .flat_map(|col| {
            signatures
                .iter()
                .map(move |sig| sig.get(col).cloned().unwrap_or_else(|| "ANY".into()))
        })
        .collect::<Vec<_>>();
    let result = strings(ctx, &values)?;
    let dim = ctx.session_mut().alloc_int_from(&[rows, cols])?;
    set_attr(ctx, result, "dim", dim)?;
    if nrows != 0 {
        let labels = strings(ctx, &arguments[..width.min(arguments.len())])?;
        let dimnames = ctx.session_mut().alloc_list(2)?;
        ctx.session_mut().list_set(dimnames, 1, labels)?;
        set_attr(ctx, result, "dimnames", dimnames)?;
    }
    Ok(result)
}

fn package_slot(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    attr(ctx, args.get(0), "package")
}

fn set_package_slot(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let object = ctx.session_mut().shallow_duplicate(args.get(0))?;
    set_attr(ctx, object, "package", args.get(1))?;
    Ok(object)
}

fn s3_part(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    if !ctx.session().is_s4(object)? {
        return Ok(object);
    }
    let class = attr(ctx, object, "class")?;
    let package = attr(ctx, class, "package")?;
    let name = texts(ctx, class)?.into_iter().next().unwrap_or_default();
    let package = texts(ctx, package)?.into_iter().next();
    let info = crate::registry::class_info(ctx, &name, package.as_deref());
    let s3 = attr(ctx, object, ".S3Class")?;
    if s3 == SEXP_NIL && ctx.session().type_of(object)? == SexpType::Obj {
        return Err(ctx.stop(call.err_call(), format!("S3Part() is only defined for classes set up by setOldCLass(), basic classes or subclasses of these:  not true of class “{name}”")));
    }

    let result = ctx.session_mut().shallow_duplicate(object)?;
    if let Some(info) = info {
        for (slot, _) in info.slots {
            if !matches!(slot.as_str(), ".Data" | ".S3Class") {
                set_attr(ctx, result, &slot, SEXP_NIL)?;
            }
        }
    }
    let strict = flag(ctx, args.get(1));
    if s3 == SEXP_NIL || strict {
        set_attr(ctx, result, "class", if strict { s3 } else { SEXP_NIL })?;
        set_attr(ctx, result, ".S3Class", SEXP_NIL)?;
        ctx.session_mut().set_s4_flag(result, false)?;
    } else {
        let classes = if args.get(2) == SEXP_NIL {
            s3
        } else {
            args.get(2)
        };
        let name = texts(ctx, classes)?.into_iter().next().unwrap_or_default();
        let class = ctx.session_mut().scalar_str(&name)?;
        set_attr(ctx, result, "class", class)?;
    }
    Ok(result)
}

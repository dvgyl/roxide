use std::collections::HashSet;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, EMPTY_ENV, SEXP_NIL, SexpType, ValueApi};

use super::{RefEpoch, attr, define, field_name, local, relations, set_attr, set_class, texts};
use crate::class::definitions::{lookup, package_name};
use crate::dispatch::metadata::{flag, strings};

fn named_entries(ctx: &mut Ctx, value: SexpId, label: &str) -> EvalResult<Vec<(String, SexpId)>> {
    let names = attr(ctx, value, "names")?;
    let values = ctx.session().list_slice(value)?.to_vec();
    if values.is_empty() {
        return Ok(Vec::new());
    }
    if label == "fields" && names == SEXP_NIL {
        return Err(ctx.stop(SEXP_NIL, "No field names supplied"));
    }
    let names = texts(ctx, names).map_err(|_| {
        ctx.stop(
            SEXP_NIL,
            format!("{label} must have distinct nonempty names"),
        )
    })?;
    let mut seen = HashSet::new();
    if label == "fields" && names.iter().any(|name| !seen.insert(name)) {
        let names = names
            .iter()
            .map(|name| format!("‘{name}’"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ctx.stop(
            SEXP_NIL,
            format!("All field names must be distinct in:\n({names})"),
        ));
    }
    seen.clear();
    if names.len() != values.len()
        || names
            .iter()
            .any(|name| name.is_empty() || !seen.insert(name))
    {
        return Err(ctx.stop(
            SEXP_NIL,
            format!("{label} must have distinct nonempty names"),
        ));
    }
    Ok(names.into_iter().zip(values).collect())
}

fn fields(ctx: &mut Ctx, value: SexpId, env: SexpId) -> EvalResult<Vec<(String, SexpId)>> {
    let fields = match ctx.session().type_of(value)? {
        SexpType::Vec => named_entries(ctx, value, "fields")?,
        SexpType::Str => {
            let names = attr(ctx, value, "names")?;
            if names == SEXP_NIL {
                let names = texts(ctx, value)?;
                let mut seen = HashSet::new();
                if names.iter().any(|name| !seen.insert(name)) {
                    let names = names
                        .iter()
                        .map(|name| format!("‘{name}’"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(ctx.stop(
                        SEXP_NIL,
                        format!("All field names must be distinct in:\n({names})"),
                    ));
                }
                if names.iter().any(String::is_empty) {
                    return Err(ctx.stop(SEXP_NIL, "fields must have distinct nonempty names"));
                }
                let any = ctx.session_mut().scalar_str("ANY")?;
                names.into_iter().map(|name| (name, any)).collect()
            } else {
                let values = ctx.session().str_slice(value)?.to_vec();
                let list = ctx.session_mut().alloc_list(values.len())?;
                for (i, value) in values.into_iter().enumerate() {
                    let scalar = ctx.session_mut().alloc_str_from(&[value])?;
                    ctx.session_mut().list_set(list, i, scalar)?;
                }
                set_attr(ctx, list, "names", names)?;
                named_entries(ctx, list, "fields")?
            }
        }
        _ => return Err(ctx.stop(SEXP_NIL, "'fields' must be a character vector or list")),
    };
    for (_, spec) in &fields {
        ctx.protect(*spec);
    }
    for (key, spec) in &fields {
        let spec = *spec;
        let ty = ctx.session().type_of(spec)?;
        if ty == SexpType::Clo {
            continue;
        }
        if ty != SexpType::Str
            || ctx.session().xlength(spec)? != 1
            || ctx.session().str_slice(spec)?.contains(&CharId::NA)
        {
            let class = relations::runtime_class(ctx, spec)?;
            return Err(ctx.stop(SEXP_NIL, format!(
                "field ‘{key}’ was supplied as an object of class “{class}”; must be a class name or a binding function"
            )));
        }
        let class = field_name(ctx, spec)?;
        if lookup(ctx, &class, None, env, true)?.is_none() {
            return Err(ctx.stop(
                SEXP_NIL,
                format!("class “{class}” for field ‘{key}’ is not defined"),
            ));
        }
    }
    Ok(fields)
}

pub(super) fn add_methods(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    install_methods(ctx, args.get(0), args.get(1))
}

fn install_methods(ctx: &mut Ctx, meta: SexpId, methods: SexpId) -> EvalResult {
    if ctx.session().type_of(methods)? != SexpType::Vec {
        return Err(ctx.stop(SEXP_NIL, "'methods' must be a list"));
    }
    let entries = named_entries(ctx, methods, "methods")?;
    for (name, fun) in &entries {
        if ctx.session().type_of(*fun)? != SexpType::Clo {
            return Err(ctx.stop(SEXP_NIL, format!("method ‘{name}’ must be a function")));
        }
    }
    let target = local(ctx, meta, "methods")?;
    if entries.is_empty() {
        return Ok(target);
    }
    for (name, function) in entries {
        define(ctx, target, &name, function)?;
    }
    let epoch = ctx.ext::<RefEpoch>();
    epoch.0 = epoch.0.wrapping_add(1);
    Ok(target)
}

fn append_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

pub(super) fn create(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let env = args.get(4);
    let class = args.get(0);
    let fields = fields(ctx, args.get(1), env)?;
    let mut contains = texts(ctx, args.get(2))?;
    let inherit_package = flag(ctx, args.get(5));
    let package = package_name(ctx, env)?;
    let package_value = ctx.session_mut().scalar_str(&package)?;
    ctx.protect(package_value);
    let mut parents = Vec::new();
    let mut inherited = Vec::<(String, SexpId)>::new();
    let mut locks = Vec::new();
    let mut superclasses = Vec::new();
    let mut parent_env = env;
    for name in &contains {
        let info = lookup(ctx, name, None, env, true)?
            .ok_or_else(|| ctx.stop(SEXP_NIL, format!("“{name}” is not a defined class")))?;
        let parent = attr(ctx, info.class_def, ".rx_ref_meta")?;
        if parent == SEXP_NIL {
            continue;
        }
        ctx.protect(parent);
        parents.push(parent);
        let fields = local(ctx, parent, "fields")?;
        for (key, value) in named_entries(ctx, fields, "fields")? {
            if !inherited.iter().any(|(name, _)| *name == key) {
                inherited.push((key, value));
            }
        }
        let inherited_locks = local(ctx, parent, "locks")?;
        for name in texts(ctx, inherited_locks)? {
            append_unique(&mut locks, name);
        }
        append_unique(&mut superclasses, name.clone());
        let parents_of_parent = local(ctx, parent, "superclasses")?;
        for name in texts(ctx, parents_of_parent)? {
            append_unique(&mut superclasses, name);
        }
        if inherit_package && parent_env == env && info.package != package {
            parent_env = local(ctx, parent, "parentEnv")?;
        }
    }
    if parents.is_empty() {
        append_unique(&mut contains, "envRefClass".to_owned());
    }
    append_unique(&mut superclasses, "envRefClass".to_owned());
    for (key, value) in fields {
        if let Some(entry) = inherited.iter_mut().find(|(name, _)| *name == key) {
            entry.1 = value;
        } else {
            inherited.push((key, value));
        }
    }
    let meta = ctx.session_mut().alloc_env(EMPTY_ENV)?;
    ctx.protect(meta);
    let methods = ctx.session_mut().alloc_env(EMPTY_ENV)?;
    let parents_list = ctx.session_mut().alloc_list(parents.len())?;
    for (i, parent) in parents.into_iter().enumerate() {
        ctx.session_mut().list_set(parents_list, i, parent)?;
    }
    let fields = ctx.session_mut().alloc_list(inherited.len())?;
    let mut names = Vec::new();
    for (i, (name, spec)) in inherited.into_iter().enumerate() {
        names.push(name);
        ctx.session_mut().list_set(fields, i, spec)?;
    }
    if !names.is_empty() {
        let names = strings(ctx, &names)?;
        set_attr(ctx, fields, "names", names)?;
    }
    let locks = strings(ctx, &locks)?;
    let contains = strings(ctx, &contains)?;
    let superclasses = strings(ctx, &superclasses)?;
    for (name, value) in [
        ("name", class),
        ("where", env),
        ("package", package_value),
        ("parentEnv", parent_env),
        ("fields", fields),
        ("methods", methods),
        ("parents", parents_list),
        ("superclasses", superclasses),
        ("locks", locks),
        ("contains", contains),
    ] {
        define(ctx, meta, name, value)?;
    }
    install_methods(ctx, meta, args.get(3))?;
    Ok(meta)
}

pub(super) fn field_types(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    cached_field_types(ctx, args.get(0))
}

fn cached_field_types(ctx: &mut Ctx, meta: SexpId) -> EvalResult {
    let cached = local(ctx, meta, "fieldTypes")?;
    if cached != SEXP_NIL {
        return Ok(cached);
    }
    let fields = local(ctx, meta, "fields")?;
    let names = attr(ctx, fields, "names")?;
    let specs = ctx.session().list_slice(fields)?.to_vec();
    let mut types = Vec::with_capacity(specs.len());
    for spec in specs {
        if ctx.session().type_of(spec)? == SexpType::Clo {
            types.push("activeBindingFunction".to_owned());
        } else {
            types.push(field_name(ctx, spec)?);
        }
    }
    let types = strings(ctx, &types)?;
    set_attr(ctx, types, "names", names)?;
    define(ctx, meta, "fieldTypes", types)?;
    Ok(types)
}

pub(super) fn finish(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let def = args.get(0);
    let meta = args.get(1);
    let fields = local(ctx, meta, "fields")?;
    let types = cached_field_types(ctx, meta)?;
    let names = attr(ctx, types, "names")?;
    let fields = named_entries(ctx, fields, "fields")?;
    let types = ctx.session().str_slice(types)?.to_vec();
    let prototypes = ctx.session_mut().alloc_env(EMPTY_ENV)?;
    let classes = ctx.session_mut().alloc_list(types.len())?;
    for (i, ((name, field), class)) in fields.into_iter().zip(types).enumerate() {
        define(ctx, prototypes, &name, field)?;
        let class = ctx.session_mut().alloc_str_from(&[class])?;
        ctx.session_mut().list_set(classes, i, class)?;
    }
    set_attr(ctx, classes, "names", names)?;
    let methods = local(ctx, meta, "methods")?;
    let superclasses = local(ctx, meta, "superclasses")?;
    for (slot, value) in [
        ("fieldClasses", classes),
        ("fieldPrototypes", prototypes),
        ("refMethods", methods),
        ("refSuperClasses", superclasses),
        (".rx_ref_meta", meta),
    ] {
        set_attr(ctx, def, slot, value)?;
    }
    set_class(ctx, def, "refClassRepresentation")?;
    define(ctx, meta, "def", def)?;
    Ok(def)
}

fn constructor(ctx: &mut Ctx, meta: SexpId) -> EvalResult {
    let ns = super::namespace(ctx);
    let template = local(ctx, ns, ".ref_construct")?;
    let (formals, body, _) = ctx.session().closure_parts(template)?;
    let scope = ctx.session_mut().alloc_env(ns)?;
    define(ctx, scope, "meta", meta)?;
    Ok(ctx.session_mut().alloc_closure(formals, body, scope)?)
}

pub(super) fn generator(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let meta = args.get(0);
    let make = constructor(ctx, meta)?;
    let name = local(ctx, meta, "name")?;
    let name = field_name(ctx, name)?;
    let package = local(ctx, meta, "package")?;
    let package_name = field_name(ctx, package)?;
    let name = crate::dispatch::metadata::package_string(ctx, &name, &package_name)?;
    let info = crate::registry::class_info(ctx, "refGeneratorSlot", Some("methods"))
        .ok_or_else(|| ctx.stop(SEXP_NIL, "reference class generator is unavailable"))?;
    let prototype = attr(ctx, info.class_def, "prototype")?;
    let generator = super::stamp_instance(ctx, prototype, &info)?;
    set_attr(ctx, generator, ".rx_ref_meta", meta)?;
    for (slot, value) in [
        ("generator", generator),
        ("className", name),
        ("package", package),
        (".rx_ref_meta", meta),
    ] {
        set_attr(ctx, make, slot, value)?;
    }
    set_class(ctx, make, "refObjectGenerator")?;
    Ok(make)
}

pub(super) fn generator_access(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let name = field_name(ctx, args.get(1))?;
    generator_member(ctx, args.get(0), &name)
}

pub(super) fn generator_member(ctx: &mut Ctx, object: SexpId, name: &str) -> EvalResult {
    if name == "new" {
        let meta = attr(ctx, object, ".rx_ref_meta")?;
        let fun = constructor(ctx, meta)?;
        let name = ctx.session_mut().scalar_str("new")?;
        let owner = ctx.session_mut().scalar_str("refGeneratorSlot")?;
        let empty = strings(ctx, &[])?;
        let no_super = ctx.session_mut().scalar_str("")?;
        for (slot, value) in [
            ("name", name),
            ("refClassName", owner),
            ("mayCall", empty),
            ("superClassMethod", no_super),
        ] {
            set_attr(ctx, fun, slot, value)?;
        }
        set_class(ctx, fun, "refMethodDef")?;
        return Ok(fun);
    }
    let ns = super::namespace(ctx);
    let key = ctx.session_mut().scalar_str(name)?;
    ctx.call_function(".ref_generator_member", &[(None, object), (None, key)], ns)
}

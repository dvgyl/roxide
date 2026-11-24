use std::collections::{HashMap, HashSet};

use roxide_eval::{Args, Call, Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{BASE_ENV, GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi};

use crate::registry::{ClassInfo, MethodsExt};

use super::inheritance;
use super::*;

mod builtins;
mod prototype;
pub(super) use builtins::bootstrap;
pub(super) use prototype::prototype;

pub(super) type Key = (String, String);

#[derive(Clone)]
pub(super) struct Edge {
    pub(super) target: Key,
    pub(super) methods: Option<super::relations::Conversion>,
}

impl Edge {
    pub(super) fn simple(info: &ClassInfo) -> Self {
        Self {
            target: key(info),
            methods: None,
        }
    }
}

#[derive(Clone)]
pub(super) struct Record {
    pub(super) where_: SexpId,
    pub(super) edges: Vec<Edge>,
    pub(super) data: Option<String>,
}

#[derive(Default)]
pub(super) struct ClassState {
    pub(super) records: HashMap<Key, Record>,
    order: Vec<Key>,
    pub(super) conversions: HashMap<(Key, Key), super::relations::Conversion>,
    pub(super) unions: HashMap<Key, Vec<Key>>,
    namespace: Option<SexpId>,
    edge_functions: HashMap<Key, (SexpId, SexpId)>,
    locked_definitions: HashMap<SexpId, Key>,
}

impl Trace for ClassState {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(ns) = self.namespace {
            tracer.edge(ns);
        }
        for record in self.records.values() {
            tracer.edge(record.where_);
            for conversion in record.edges.iter().filter_map(|edge| edge.methods.as_ref()) {
                conversion.trace(tracer);
            }
        }
        for &def in self.locked_definitions.keys() {
            tracer.edge(def);
        }
        for &(forward, replace) in self.edge_functions.values() {
            tracer.edge(forward);
            tracer.edge(replace);
        }
        for conversion in self.conversions.values() {
            conversion.trace(tracer);
        }
    }
}

impl CtxExt for ClassState {}

pub(super) fn key(info: &ClassInfo) -> Key {
    (info.name.clone(), info.package.clone())
}

pub(super) fn methods_env(ctx: &mut Ctx) -> SexpId {
    ctx.ext::<ClassState>().namespace.unwrap_or(GLOBAL_ENV)
}

pub(crate) fn package_name(ctx: &mut Ctx, env: SexpId) -> EvalResult<String> {
    let pkg = ctx.session_mut().intern(".packageName")?;
    if let Some(value) = ctx.get_local(pkg, env)? {
        return name(ctx, value, SEXP_NIL);
    }
    if env == BASE_ENV {
        return Ok("base".into());
    }
    Ok(ctx
        .namespace_name(env)
        .unwrap_or_else(|| ".GlobalEnv".into()))
}

pub(super) fn package(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = args.get(0);
    if ctx.session().type_of(env)? != SexpType::Env {
        return Err(error(ctx, call, "invalid 'where' argument"));
    }
    let package = package_name(ctx, env)?;
    Ok(ctx.session_mut().scalar_str(&package)?)
}

pub(super) fn class_spec(
    ctx: &mut Ctx,
    value: SexpId,
    call: SexpId,
) -> EvalResult<(String, Option<String>)> {
    let class = match ctx.session().type_of(value)? {
        SexpType::Str | SexpType::Sym => value,
        _ => attr(ctx, value, "className")?,
    };
    if class == SEXP_NIL {
        return Err(ctx.stop(
            call,
            "class should be either a character-string name or a class definition",
        ));
    }
    let class_name = name(ctx, class, call)?;
    let package = attr(ctx, class, "package")?;
    let package = (package != SEXP_NIL)
        .then(|| name(ctx, package, call))
        .transpose()?;
    Ok((class_name, package))
}

pub(super) fn cached(ctx: &mut Ctx, name: &str, package: Option<&str>) -> Option<ClassInfo> {
    let registry = ctx.ext::<MethodsExt>();
    if let Some(package) = package {
        return registry
            .classes
            .get(&(name.into(), package.into()))
            .cloned();
    }
    for package in [".GlobalEnv", "methods"] {
        if let Some(info) = registry.classes.get(&(name.into(), package.into())) {
            return Some(info.clone());
        }
    }
    let classes = ctx.ext::<ClassState>().order.clone();
    classes
        .iter()
        .rev()
        .find(|(class, _)| class == name)
        .and_then(|key| ctx.ext::<MethodsExt>().classes.get(key))
        .cloned()
}

pub(crate) fn lookup(
    ctx: &mut Ctx,
    class: &str,
    package: Option<&str>,
    env: SexpId,
    inherits: bool,
) -> EvalResult<Option<ClassInfo>> {
    let symbol = ctx.session_mut().intern(&format!(".__C__{class}"))?;
    let value = if inherits {
        ctx.get_var(symbol, env)?
    } else {
        ctx.get_local(symbol, env)?
    };
    if let Some(value) = value
        && ctx.session().is_s4(value)?
    {
        let pkg = attr(ctx, value, "package")?;
        let pkg = name(ctx, pkg, SEXP_NIL)?;
        if package.is_none_or(|wanted| wanted == pkg) {
            if let Some(info) = ctx
                .ext::<ClassState>()
                .locked_definitions
                .get(&value)
                .cloned()
                && let Some(cached) = ctx.ext::<MethodsExt>().classes.get(&info)
            {
                return Ok(Some(cached.clone()));
            }
            return definition_info(ctx, value).map(Some);
        }
    }
    Ok(if inherits {
        cached(ctx, class, package)
    } else {
        None
    })
}

pub(crate) fn definition_info(ctx: &mut Ctx, def: SexpId) -> EvalResult<ClassInfo> {
    let (name, package) = class_spec(ctx, def, SEXP_NIL)?;
    let package = package.unwrap_or_else(|| ".GlobalEnv".into());
    if let Some(info) = cached(ctx, &name, Some(&package))
        && info.class_def == def
    {
        return Ok(info);
    }
    let slots = attr(ctx, def, "slots")?;
    let slots = named_values(ctx, slots)?
        .into_iter()
        .map(|(slot, typ)| super::name(ctx, typ, SEXP_NIL).map(|typ| (slot, typ)))
        .collect::<EvalResult<_>>()?;
    let virtual_ = attr(ctx, def, "virtual")?;
    let contains = attr(ctx, def, "contains")?;
    let superclasses = named_values(ctx, contains)?
        .into_iter()
        .enumerate()
        .map(|(rank, (parent, edge))| {
            let distance = attr(ctx, edge, "distance")?;
            let distance = ctx.session().real(distance)?.iter().next().unwrap_or(1.0);
            Ok((parent, distance as usize, rank))
        })
        .collect::<EvalResult<_>>()?;
    Ok(ClassInfo {
        name,
        package,
        slots,
        superclasses,
        is_virtual: flag(ctx, virtual_)?,
        class_def: def,
    })
}

pub(super) fn resolve(
    ctx: &mut Ctx,
    value: SexpId,
    env: SexpId,
    call: SexpId,
) -> EvalResult<Option<ClassInfo>> {
    let (name, package) = class_spec(ctx, value, call)?;
    if ctx.session().type_of(value)? == SexpType::Obj && attr(ctx, value, "slots")? != SEXP_NIL {
        return definition_info(ctx, value).map(Some);
    }
    lookup(ctx, &name, package.as_deref(), env, true)
}

pub(crate) fn object_info(ctx: &mut Ctx, object: SexpId) -> EvalResult<Option<ClassInfo>> {
    let class = attr(ctx, object, "class")?;
    if class == SEXP_NIL {
        return Ok(None);
    }
    let (name, package) = class_spec(ctx, class, SEXP_NIL)?;
    if let Some(package) = package.as_deref()
        && let Some(env) = ctx.ns_lookup(package)
    {
        return lookup(ctx, &name, Some(package), env, true);
    }
    Ok(cached(ctx, &name, package.as_deref()))
}

pub(crate) fn cache_namespace_class(
    ctx: &mut Ctx,
    info: &ClassInfo,
    env: SexpId,
) -> EvalResult<()> {
    let key = key(info);
    if ctx.ext::<ClassState>().records.contains_key(&key) {
        return Ok(());
    }
    let contains = attr(ctx, info.class_def, "contains")?;
    let mut edges = Vec::new();
    for (_, extension) in named_values(ctx, contains)? {
        let distance = attr(ctx, extension, "distance")?;
        let distance = match ctx.session().type_of(distance)? {
            SexpType::Int => ctx.session().int(distance)?.iter().next().map(f64::from),
            SexpType::Real => ctx.session().real(distance)?.iter().next(),
            _ => None,
        };
        if distance != Some(1.0) {
            continue;
        }
        let target = attr(ctx, extension, "superClass")?;
        let (name, package) = class_spec(ctx, target, SEXP_NIL)?;
        let simple = attr(ctx, extension, "simple")?;
        let methods = if flag(ctx, simple)? {
            None
        } else {
            Some(relations::Conversion {
                forward: decode_slot(attr(ctx, extension, "coerce")?),
                reverse: decode_slot(attr(ctx, extension, "replace")?),
                test: decode_slot(attr(ctx, extension, "test")?),
            })
        };
        edges.push(Edge {
            target: (name, package.unwrap_or_else(|| info.package.clone())),
            methods,
        });
    }
    let data = data_class(ctx, info);
    let state = ctx.ext::<ClassState>();
    state.order.push(key.clone());
    state.records.insert(
        key,
        Record {
            where_: env,
            edges,
            data,
        },
    );
    Ok(())
}

pub(crate) fn forget_namespace_classes(ctx: &mut Ctx, package: &str) {
    let state = ctx.ext::<ClassState>();
    state.records.retain(|(_, owner), _| owner != package);
    state.order.retain(|(_, owner)| owner != package);
    state
        .locked_definitions
        .retain(|_, (_, owner)| owner != package);
    state
        .edge_functions
        .retain(|(_, owner), _| owner != package);
    state.unions.retain(|(_, owner), _| owner != package);
    state
        .conversions
        .retain(|((_, from), (_, to)), _| from != package && to != package);
    for record in state.records.values_mut() {
        record.edges.retain(|edge| edge.target.1 != package);
    }
}

pub(super) fn data_class(ctx: &mut Ctx, info: &ClassInfo) -> Option<String> {
    if let Some(record) = ctx.ext::<ClassState>().records.get(&key(info)) {
        return record.data.clone();
    }
    info.slots
        .iter()
        .find(|(name, _)| name == ".Data")
        .map(|(_, class)| class.clone())
}

struct Definition {
    info: ClassInfo,
    record: Record,
    prototype: SexpId,
    validity: SexpId,
    access: SexpId,
    sealed: bool,
}

fn class_object(ctx: &mut Ctx, def: &Definition) -> EvalResult {
    let object = ctx.session_mut().alloc_s4_object()?;
    let slots = def
        .info
        .slots
        .iter()
        .map(|(slot, class)| {
            let package = if class == &def.info.name {
                def.info.package.clone()
            } else {
                cached(ctx, class, None).map_or_else(|| def.info.package.clone(), |i| i.package)
            };
            Ok((slot.clone(), qualified(ctx, class, &package)?))
        })
        .collect::<EvalResult<Vec<_>>>()?;
    let slots = make_list(ctx, &slots, !slots.is_empty())?;
    let empty = make_list(ctx, &[], false)?;
    let class_name = qualified(ctx, &def.info.name, &def.info.package)?;
    let package = ctx.session_mut().scalar_str(&def.info.package)?;
    let virtual_ = logical(ctx, def.info.is_virtual)?;
    let sealed = logical(ctx, def.sealed)?;
    let class = qualified(ctx, "classRepresentation", "methods")?;
    for (name, value) in [
        ("slots", slots),
        ("contains", empty),
        ("virtual", virtual_),
        ("prototype", def.prototype),
        ("validity", def.validity),
        ("access", def.access),
        ("className", class_name),
        ("package", package),
        ("subclasses", empty),
        // The value API needs an external pointer constructor for this slot.
        ("versionKey", SEXP_NIL),
        ("sealed", sealed),
        ("class", class),
    ] {
        write_slot(ctx, object, name, value)?;
    }
    Ok(object)
}

fn store(ctx: &mut Ctx, info: &ClassInfo, env: SexpId) -> EvalResult<()> {
    let symbol = ctx.session_mut().intern(&format!(".__C__{}", info.name))?;
    ctx.session_mut().env_define(env, symbol, info.class_def)?;
    ctx.ext::<MethodsExt>()
        .classes
        .insert(key(info), info.clone());
    Ok(())
}

fn install(ctx: &mut Ctx, mut def: Definition) -> EvalResult<ClassInfo> {
    def.info.class_def = class_object(ctx, &def)?;
    store(ctx, &def.info, def.record.where_)?;
    let state = ctx.ext::<ClassState>();
    let key = key(&def.info);
    if !state.records.contains_key(&key) {
        state.order.push(key.clone());
    }
    state.records.insert(key, def.record);
    Ok(def.info)
}

pub(super) fn update_definition(
    ctx: &mut Ctx,
    info: &ClassInfo,
    slot: &str,
    value: SexpId,
) -> EvalResult {
    let mut updated = info.clone();
    updated.class_def = ctx.session_mut().shallow_duplicate(info.class_def)?;
    write_slot(ctx, updated.class_def, slot, value)?;
    let where_ = ctx
        .ext::<ClassState>()
        .records
        .get(&key(info))
        .map(|r| r.where_)
        .or_else(|| ctx.ns_lookup(&info.package))
        .unwrap_or(GLOBAL_ENV);
    store(ctx, &updated, where_)?;
    ctx.ext::<MethodsExt>().bump_epoch();
    Ok(updated.class_def)
}

fn bind_edge_function(ctx: &mut Ctx, template: &str, target: SexpId) -> EvalResult {
    let ns = methods_env(ctx);
    let sym = ctx.session_mut().intern(template)?;
    let Some(function) = ctx.get_local(sym, ns)? else {
        return Ok(SEXP_NIL);
    };
    let (formals, body, env) = ctx.session().closure_parts(function)?;
    let local = ctx.session_mut().alloc_env(env)?;
    let sym = ctx.session_mut().intern(".s4_target")?;
    ctx.session_mut().env_define(local, sym, target)?;
    Ok(ctx.session_mut().alloc_closure(formals, body, local)?)
}

fn class_name_value(ctx: &mut Ctx, key: &Key) -> EvalResult {
    if let Some(info) = ctx.ext::<MethodsExt>().classes.get(key) {
        let definition = info.class_def;
        return attr(ctx, definition, "className");
    }
    qualified(ctx, &key.0, &key.1)
}

fn edge_functions(ctx: &mut Ctx, target: &Key, class: SexpId) -> EvalResult<(SexpId, SexpId)> {
    if let Some(&functions) = ctx.ext::<ClassState>().edge_functions.get(target) {
        return Ok(functions);
    }
    let forward = bind_edge_function(ctx, ".s4_edge_coerce", class)?;
    let replace = bind_edge_function(ctx, ".s4_edge_replace", class)?;
    ctx.ext::<ClassState>()
        .edge_functions
        .insert(target.clone(), (forward, replace));
    Ok((forward, replace))
}

struct Extensions {
    class: SexpId,
    by: SexpId,
    test: SexpId,
    values: HashMap<(Key, Key, usize), SexpId>,
}

impl Extensions {
    fn new(ctx: &mut Ctx) -> EvalResult<Self> {
        let ns = methods_env(ctx);
        let sym = ctx.session_mut().intern(".s4_true")?;
        let test = ctx.get_local(sym, ns)?.unwrap_or(SEXP_NIL);
        let class = class_name_value(ctx, &("SClassExtension".into(), "methods".into()))?;
        let by = strings(ctx, [])?;
        ctx.protect(by);
        Ok(Self {
            class,
            by,
            test,
            values: HashMap::new(),
        })
    }

    fn get(
        &mut self,
        ctx: &mut Ctx,
        sub: &Key,
        parent: &Key,
        distance: usize,
        via: Option<&Key>,
    ) -> EvalResult {
        let key = (sub.clone(), parent.clone(), distance);
        if let Some(&obj) = self.values.get(&key) {
            return Ok(obj);
        }
        let record = ctx.ext::<ClassState>().records.get(sub);
        let methods = record.and_then(|record| {
            record
                .edges
                .iter()
                .find(|edge| edge.target == *parent)
                .and_then(|edge| edge.methods)
        });
        let direct_data = record.and_then(|r| r.data.as_deref()) == Some(parent.0.as_str());
        let obj = ctx.session_mut().alloc_s4_object()?;
        ctx.protect(obj);
        let subclass = class_name_value(ctx, sub)?;
        let superclass = class_name_value(ctx, parent)?;
        let package = attr(ctx, subclass, "package")?;
        let simple = logical(ctx, methods.is_none())?;
        let data_part = logical(ctx, direct_data)?;
        let distance = ctx.session_mut().scalar_real(distance as f64)?;
        let by = if let Some(via) = via {
            ctx.session_mut().scalar_str(&via.0)?
        } else {
            self.by
        };
        let methods = methods.unwrap_or_default();
        let (coerce, replace) = edge_functions(ctx, parent, superclass)?;
        let coerce = if methods.forward == SEXP_NIL {
            coerce
        } else {
            methods.forward
        };
        let replace = if methods.reverse == SEXP_NIL {
            replace
        } else {
            methods.reverse
        };
        let test = if methods.test == SEXP_NIL {
            self.test
        } else {
            methods.test
        };
        for (name, value) in [
            ("subClass", subclass),
            ("superClass", superclass),
            ("package", package),
            ("coerce", coerce),
            ("test", test),
            ("replace", replace),
            ("simple", simple),
            ("by", by),
            ("dataPart", data_part),
            ("distance", distance),
            ("class", self.class),
        ] {
            write_slot(ctx, obj, name, value)?;
        }
        self.values.insert(key, obj);
        Ok(obj)
    }
}

pub(super) fn refresh(ctx: &mut Ctx) -> EvalResult<()> {
    refresh_edges(ctx, None)
}

pub(super) fn refresh_edges(ctx: &mut Ctx, changed: Option<(&Key, &Key)>) -> EvalResult<()> {
    ctx.scoped(|ctx| {
        let state = ctx.ext::<ClassState>();
        let order = state.order.clone();
        let records = state.records.clone();
        let chains: HashMap<_, _> = order
            .iter()
            .map(|key| {
                (
                    key.clone(),
                    inheritance::walk(&records, key).collect::<Vec<_>>(),
                )
            })
            .collect();
        let mut subclasses: HashMap<Key, Vec<_>> = HashMap::new();
        for sub in &order {
            for step in &chains[sub] {
                subclasses.entry(step.class.clone()).or_default().push((
                    sub.clone(),
                    step.distance,
                    step.predecessor,
                ));
            }
        }
        let mut extensions = Extensions::new(ctx)?;
        for key in &order {
            let Some(mut info) = ctx.ext::<MethodsExt>().classes.get(key).cloned() else {
                continue;
            };
            let chain = &chains[key];
            let linear = chain
                .iter()
                .map(|step| (step.class.0.clone(), step.distance, step.parent_rank))
                .collect::<Vec<_>>();
            let parents = attr(ctx, info.class_def, "contains")?;
            let kids = attr(ctx, info.class_def, "subclasses")?;
            let descendants = subclasses.get(key).map_or(&[][..], Vec::as_slice);
            let same_parents = info.superclasses == linear
                && matching_extensions(
                    ctx,
                    parents,
                    chain.iter().map(|step| (step.class, step.distance)),
                )?;
            let same_children =
                matching_extensions(ctx, kids, descendants.iter().map(|(c, d, _)| (c, *d)))?;
            if same_parents
                && same_children
                && changed.is_none_or(|(from, to)| key != from && key != to)
            {
                continue;
            }
            let new_def = ctx.session_mut().shallow_duplicate(info.class_def)?;
            let contains = chain
                .iter()
                .map(|step| {
                    Ok((
                        step.class.0.clone(),
                        extensions.get(ctx, key, step.class, step.distance, step.predecessor)?,
                    ))
                })
                .collect::<EvalResult<Vec<_>>>()?;
            let contains = make_list(ctx, &contains, !contains.is_empty())?;
            let children = descendants
                .iter()
                .map(|(child, distance, via)| {
                    Ok((
                        child.0.clone(),
                        extensions.get(ctx, child, key, *distance, *via)?,
                    ))
                })
                .collect::<EvalResult<Vec<_>>>()?;
            let children = make_list(ctx, &children, !children.is_empty())?;
            write_slot(ctx, new_def, "contains", contains)?;
            write_slot(ctx, new_def, "subclasses", children)?;
            info.class_def = new_def;
            info.superclasses = linear;
            let env = records[key].where_;
            let symbol = ctx.session_mut().intern(&format!(".__C__{}", info.name))?;
            if ctx.session().env_is_locked(env)?
                || ctx.session().env_binding_is_locked(env, symbol)? == Some(true)
            {
                if let Some(binding) = ctx.session().env_get(env, symbol) {
                    ctx.ext::<ClassState>()
                        .locked_definitions
                        .insert(binding.value, key.clone());
                }
                ctx.ext::<MethodsExt>().classes.insert(key.clone(), info);
            } else {
                store(ctx, &info, env)?;
            }
        }
        ctx.ext::<MethodsExt>().bump_epoch();
        Ok(())
    })
}

fn matching_extensions<'a>(
    ctx: &mut Ctx,
    value: SexpId,
    expected: impl ExactSizeIterator<Item = (&'a Key, usize)>,
) -> EvalResult<bool> {
    let entries = named_values(ctx, value)?;
    if entries.len() != expected.len() {
        return Ok(false);
    }
    for ((name, entry), (key, distance)) in entries.into_iter().zip(expected) {
        let old_distance = attr(ctx, entry, "distance")?;
        if name != key.0 || ctx.session().real(old_distance)?.iter().next() != Some(distance as f64)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn get(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let class = args.get(0);
    let (class_name, class_pkg) = class_spec(ctx, class, call.err_call())?;
    let pkg = args.get(2);
    let pkg = if pkg == SEXP_NIL {
        class_pkg
    } else {
        Some(name(ctx, pkg, call.err_call())?)
    };
    let env = args.get(1);
    let inherits = flag(ctx, args.get(3))?;
    let mode = ctx
        .session()
        .int(args.get(4))?
        .iter()
        .next()
        .unwrap_or_default();
    if let Some(info) = lookup(ctx, &class_name, pkg.as_deref(), env, inherits)? {
        return Ok(info.class_def);
    }
    match mode {
        0 => Ok(SEXP_NIL),
        2 => {
            let def = Definition {
                info: ClassInfo {
                    name: class_name,
                    package: "base".into(),
                    slots: Vec::new(),
                    superclasses: Vec::new(),
                    is_virtual: true,
                    class_def: SEXP_NIL,
                },
                record: Record {
                    where_: env,
                    edges: Vec::new(),
                    data: None,
                },
                prototype: SEXP_NIL,
                validity: SEXP_NIL,
                access: make_list(ctx, &[], false)?,
                sealed: false,
            };
            class_object(ctx, &def)
        }
        _ => Err(bad_class(ctx, call.err_call(), &class_name)),
    }
}

pub(super) fn remove(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = name(ctx, args.get(0), call.err_call())?;
    let env = args.get(1);
    let symbol = ctx.session_mut().intern(&format!(".__C__{name}"))?;
    let Some(def) = ctx.get_local(symbol, env)? else {
        ctx.warn(
            call.err_call(),
            format!("class definition for “{name}” not found (no action taken)"),
        );
        return logical(ctx, false);
    };
    let info = definition_info(ctx, def)?;
    ctx.session_mut().env_remove(env, symbol)?;
    let key = key(&info);
    ctx.ext::<MethodsExt>().classes.remove(&key);
    let state = ctx.ext::<ClassState>();
    state.records.remove(&key);
    state.locked_definitions.retain(|_, owner| owner != &key);
    state.edge_functions.remove(&key);
    state.order.retain(|entry| entry != &key);
    state.unions.remove(&key);
    state
        .conversions
        .retain(|(from, to), _| from != &key && to != &key);
    for record in state.records.values_mut() {
        record.edges.retain(|edge| edge.target != key);
    }
    refresh(ctx)?;
    logical(ctx, true)
}

pub(super) fn generator(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let generator = args.get(0);
    let definition = args.get(1);
    let class_name = attr(ctx, definition, "className")?;
    let package = attr(ctx, definition, "package")?;
    let class = qualified(ctx, "classGeneratorFunction", "methods")?;
    ctx.session_mut().set_s4_flag(generator, true)?;
    write_slot(ctx, generator, "className", class_name)?;
    write_slot(ctx, generator, "package", package)?;
    write_slot(ctx, generator, "class", class)?;
    Ok(generator)
}

fn slot_entries(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<(String, SexpId)>> {
    if ctx.session().type_of(value)? != SexpType::Str {
        return named_values(ctx, value);
    }
    let names = attr(ctx, value, "names")?;
    let names = if names == SEXP_NIL {
        Vec::new()
    } else {
        text_vec(ctx, names)?
    };
    let types = text_vec(ctx, value)?;
    types
        .into_iter()
        .enumerate()
        .map(|(i, class)| {
            Ok((
                names.get(i).cloned().unwrap_or_default(),
                ctx.session_mut().scalar_str(&class)?,
            ))
        })
        .collect()
}

pub(super) fn define(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    ctx.scoped(|ctx| {
        let class = name(ctx, args.get(0), call.err_call())?;
        let env = args.get(6);
        let pkg = name(ctx, args.get(8), call.err_call())?;
        if let Some(old) = lookup(ctx, &class, Some(&pkg), env, false)? {
            let sealed = attr(ctx, old.class_def, "sealed")?;
            if flag(ctx, sealed)? {
                return Err(error(ctx, call, format!("“{class}” has a sealed class definition and cannot be redefined")));
            }
        }
        let representation = args.get(1);
        if representation != SEXP_NIL && ctx.session().type_of(representation)? != SexpType::Vec {
            let got = relations::runtime_class(ctx, representation)?;
            return Err(error(ctx, call, format!(
                "invalid class “classRepresentation” object: invalid object for slot \"slots\" in class \"classRepresentation\": got class \"{got}\", should be or extend class \"list\""
            )));
        }
        let representation = named_values(ctx, representation)?;
        let slot_entries = slot_entries(ctx, args.get(9))?;
        if !representation.is_empty() && !slot_entries.is_empty() {
            return Err(error(ctx, call, "Argument \"representation\" cannot be used if argument \"slots\" is supplied"));
        }
        let mut parents = text_vec(ctx, args.get(3))?;
        let mut slots = Vec::new();
        for (i, (slot, typ)) in representation.into_iter().chain(slot_entries).enumerate() {
            if ctx.session().type_of(typ)? != SexpType::Str || ctx.session().xlength(typ)? != 1 {
                return Err(error(ctx, call, format!("element {} of the representation was not a single character string", i + 1)));
            }
            let typ = name(ctx, typ, call.err_call())?;
            if slot.is_empty() {
                parents.push(typ);
            } else {
                slots.push((slot, typ));
            }
        }
        let mut distinct = HashSet::new();
        if !slots.iter().all(|(slot, _)| distinct.insert(slot)) {
            let names = slots.iter().map(|(slot, _)| format!("‘{slot}’")).collect::<Vec<_>>();
            return Err(error(ctx, call, format!("All slot names must be distinct in:\n({})", names.join(", "))));
        }
        let explicit_virtual = parents.iter().any(|parent| parent == "VIRTUAL");
        let mut ancestors = Vec::new();
        for parent in parents.iter().filter(|parent| *parent != "VIRTUAL") {
            let Some(info) = lookup(ctx, parent, None, env, true)? else {
                return Err(error(ctx, call, format!("no definition was found for superclass “{parent}” in the specification of class “{class}”")));
            };
            if info.name == class || names_in_chain(&info).any(|n| n == class) {
                return Err(error(ctx, call, format!("class “{class}” cannot contain itself")));
            }
            ancestors.push(info);
        }
        let is_virtual = explicit_virtual || (slots.is_empty() && ancestors.is_empty());
        let mut data = slots.iter().find(|(slot, _)| slot == ".Data").map(|(_, typ)| typ.clone());
        for parent in &ancestors {
            ctx.protect(parent.class_def);
            if data.is_none() {
                data = data_class(ctx, parent);
            }
            for (slot, inherited) in parent.slots.iter().filter(|(name, _)| name != ".Data") {
                if let Some((_, declared)) = slots.iter().find(|(name, _)| name == slot) {
                    if declared != inherited && inherited != "ANY"
                        && !lookup(ctx, declared, None, env, true)?.is_some_and(|info| {
                            info.superclasses.iter().any(|(name, _, _)| name == inherited)
                        })
                    {
                        return Err(error(ctx, call, format!(
                            "Definition of slot ‘{slot}’, in class “{class}”, as “{declared}” conflicts with definition, inherited from class “{}”, as “{inherited}”", parent.name
                        )));
                    }
                } else {
                    slots.push((slot.clone(), inherited.clone()));
                }
            }
        }
        if let Some(parent) = ancestors.iter().find(|p| matches!(p.name.as_str(), "environment" | "name"))
            && !slots.iter().any(|(name, _)| name == ".xData")
        {
            slots.push((".xData".into(), parent.name.clone()));
        }
        slots.retain(|(name, _)| name != ".Data");
        if let Some(data) = &data {
            slots.insert(0, (".Data".into(), data.clone()));
        }

        let prototype = prototype::build(
            ctx,
            call,
            args.get(2),
            prototype::Spec {
                class: &class,
                slots: &slots,
                data: data.as_deref(),
                ancestors: &ancestors,
                env,
            },
        )?;
        let validity = args.get(4);
        if validity != SEXP_NIL && !callable(ctx, validity)? {
            return Err(error(ctx, call, "the validity method must be a function"));
        }
        let def = Definition {
            info: ClassInfo {
                name: class, package: pkg, slots, superclasses: Vec::new(),
                is_virtual, class_def: SEXP_NIL,
            },
            record: Record { where_: env, edges: ancestors.iter().map(Edge::simple).collect(), data },
            prototype, validity, access: args.get(5), sealed: flag(ctx, args.get(7))?,
        };
        let installed = install(ctx, def)?;
        refresh(ctx)?;
        Ok(cached(ctx, &installed.name, Some(&installed.package))
            .map_or(installed.class_def, |info| info.class_def))
    })
}

pub(super) fn special_class(
    ctx: &mut Ctx,
    name: String,
    env: SexpId,
    slots: Vec<(String, String)>,
    parents: Vec<Edge>,
    prototype: SexpId,
    template: Option<&ClassInfo>,
) -> EvalResult<ClassInfo> {
    let data = template.and_then(|info| data_class(ctx, info));
    let package = package_name(ctx, env)?;
    let def = Definition {
        info: ClassInfo {
            name,
            package,
            slots,
            superclasses: Vec::new(),
            is_virtual: template.is_none_or(|info| info.is_virtual),
            class_def: SEXP_NIL,
        },
        record: Record {
            where_: env,
            edges: parents,
            data,
        },
        prototype,
        validity: SEXP_NIL,
        access: make_list(ctx, &[], false)?,
        sealed: false,
    };
    install(ctx, def)
}

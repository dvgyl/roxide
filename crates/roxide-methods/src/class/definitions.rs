use std::collections::{HashMap, HashSet, VecDeque};

use roxide_eval::{Args, Call, Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{BASE_ENV, GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi};

use crate::registry::{ClassInfo, MethodsExt};

use super::*;

mod builtins;
pub(super) use builtins::bootstrap;

type Key = (String, String);

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

fn package_name(ctx: &mut Ctx, env: SexpId) -> EvalResult<String> {
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

pub(super) fn lookup(
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
            return definition_info(ctx, value).map(Some);
        }
    }
    Ok(if inherits {
        cached(ctx, class, package).or_else(|| cached(ctx, class, None))
    } else {
        None
    })
}

fn definition_info(ctx: &mut Ctx, def: SexpId) -> EvalResult<ClassInfo> {
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

pub(super) fn object_info(ctx: &mut Ctx, object: SexpId) -> EvalResult<Option<ClassInfo>> {
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
    Ok(cached(ctx, &name, package.as_deref()).or_else(|| cached(ctx, &name, None)))
}

pub(super) fn data_class(ctx: &mut Ctx, info: &ClassInfo) -> EvalResult<Option<String>> {
    if let Some(record) = ctx.ext::<ClassState>().records.get(&key(info)) {
        return Ok(record.data.clone());
    }
    Ok(info
        .slots
        .iter()
        .find(|(name, _)| name == ".Data")
        .map(|(_, class)| class.clone()))
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

    fn get(&mut self, ctx: &mut Ctx, sub: &Key, parent: &Key, distance: usize) -> EvalResult {
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
            ("by", self.by),
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

type Chain = Vec<(Key, usize, usize)>;

fn linearize(source: &Key, records: &HashMap<Key, Record>) -> Chain {
    let mut found = HashSet::from([source.clone()]);
    let mut chain = Vec::new();
    let mut pending = VecDeque::new();
    if let Some(record) = records.get(source) {
        pending.extend(
            record
                .edges
                .iter()
                .enumerate()
                .map(|(i, edge)| (&edge.target, 1, i)),
        );
    }
    while let Some((parent, distance, rank)) = pending.pop_front() {
        if !found.insert(parent.clone()) {
            continue;
        }
        chain.push((parent.clone(), distance, rank));
        if let Some(record) = records.get(parent) {
            pending.extend(
                record
                    .edges
                    .iter()
                    .map(|edge| (&edge.target, distance + 1, rank)),
            );
        }
    }
    chain
}

pub(super) fn refresh(ctx: &mut Ctx) -> EvalResult<()> {
    ctx.scoped(|ctx| {
        let state = ctx.ext::<ClassState>();
        let order = state.order.clone();
        let records = state.records.clone();
        let chains: HashMap<_, _> = order
            .iter()
            .map(|key| (key.clone(), linearize(key, &records)))
            .collect();
        let mut subclasses: HashMap<Key, Vec<(Key, usize)>> = HashMap::new();
        for sub in &order {
            for (parent, distance, _) in &chains[sub] {
                subclasses
                    .entry(parent.clone())
                    .or_default()
                    .push((sub.clone(), *distance));
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
                .map(|((n, _), d, rank)| (n.clone(), *d, *rank))
                .collect::<Vec<_>>();
            let parents = attr(ctx, info.class_def, "contains")?;
            let kids = attr(ctx, info.class_def, "subclasses")?;
            let descendants = subclasses.get(key).map_or(&[][..], Vec::as_slice);
            let same_parents = info.superclasses == linear
                && matching_extensions(ctx, parents, chain.iter().map(|(p, d, _)| (p, *d)))?;
            let same_children =
                matching_extensions(ctx, kids, descendants.iter().map(|(c, d)| (c, *d)))?;
            if same_parents && same_children {
                continue;
            }
            let new_def = ctx.session_mut().shallow_duplicate(info.class_def)?;
            let contains = chain
                .iter()
                .map(|(parent, distance, _)| {
                    Ok((
                        parent.0.clone(),
                        extensions.get(ctx, key, parent, *distance)?,
                    ))
                })
                .collect::<EvalResult<Vec<_>>>()?;
            let contains = make_list(ctx, &contains, !contains.is_empty())?;
            let children = descendants
                .iter()
                .map(|(child, distance)| {
                    Ok((child.0.clone(), extensions.get(ctx, child, key, *distance)?))
                })
                .collect::<EvalResult<Vec<_>>>()?;
            let children = make_list(ctx, &children, !children.is_empty())?;
            write_slot(ctx, new_def, "contains", contains)?;
            write_slot(ctx, new_def, "subclasses", children)?;
            info.class_def = new_def;
            info.superclasses = linear;
            store(ctx, &info, records[key].where_)?;
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

pub(super) fn prototype(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let entries = named_values(ctx, args.get(0))?;
    let (base, named) = match entries.split_first() {
        Some(((n, value), rest)) if n.is_empty() => (Some(*value), rest),
        _ => (None, entries.as_slice()),
    };
    if named.iter().any(|(n, _)| n.is_empty()) {
        return Err(error(
            ctx,
            call,
            "all slot values in a prototype must be named",
        ));
    }
    if base == Some(SEXP_NIL) && !named.is_empty() {
        return Err(error(ctx, call, "attempt to set slot on NULL object"));
    }
    let object = if let Some(base) = base {
        ctx.session_mut().shallow_duplicate(base)?
    } else {
        ctx.session_mut().alloc_s4_object()?
    };
    if !named.is_empty() && ctx.session().type_of(object)? == SexpType::Sym {
        return Err(error(ctx, call, "cannot set an attribute on a 'symbol'"));
    }
    for (slot, value) in named {
        write_slot(ctx, object, slot, *value)?;
    }
    let slots = strings(ctx, named.iter().map(|(n, _)| n.as_str()))?;
    let data = logical(ctx, base.is_some())?;
    let class = qualified(ctx, "classPrototypeDef", "methods")?;
    let ret = ctx.session_mut().alloc_s4_object()?;
    for (name, value) in [
        ("object", object),
        ("slots", slots),
        ("dataPart", data),
        ("class", class),
    ] {
        write_slot(ctx, ret, name, value)?;
    }
    Ok(ret)
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
        let is_virtual = explicit_virtual || (slots.is_empty() && ancestors.iter().all(|p| p.is_virtual));
        let mut data = slots.iter().find(|(slot, _)| slot == ".Data").map(|(_, typ)| typ.clone());
        for parent in &ancestors {
            if data.is_none() {
                data = data_class(ctx, parent)?;
            }
            slots.extend(parent.slots.iter().filter(|(name, _)| name != ".Data").cloned().collect::<Vec<_>>());
        }
        if let Some(parent) = ancestors.iter().find(|p| matches!(p.name.as_str(), "environment" | "name"))
            && !slots.iter().any(|(name, _)| name == ".xData")
        {
            slots.push((".xData".into(), parent.name.clone()));
        }
        let mut seen = HashSet::new();
        slots.retain(|(name, _)| seen.insert(name.clone()));
        slots.retain(|(name, _)| name != ".Data");
        if let Some(data) = &data {
            slots.insert(0, (".Data".into(), data.clone()));
        }

        let mut defaults = HashMap::new();
        let given = args.get(2);
        let mut explicit_data = SEXP_NIL;
        if given != SEXP_NIL {
            let class_attr = attr(ctx, given, "class")?;
            let is_prototype = ctx.session().is_s4(given)?
                && class_attr != SEXP_NIL
                && name(ctx, class_attr, call.err_call())? == "classPrototypeDef";
            if is_prototype {
                let object = decode_slot(attr(ctx, given, "object")?);
                let slots = attr(ctx, given, "slots")?;
                for slot in text_vec(ctx, slots)? {
                    let val = decode_slot(attr(ctx, object, &slot)?);
                    defaults.insert(slot, val);
                }
                let data_part = attr(ctx, given, "dataPart")?;
                if flag(ctx, data_part)? {
                    explicit_data = object;
                }
            } else if ctx.session().type_of(given)? == SexpType::Vec && !ctx.session().is_s4(given)? {
                defaults = named_values(ctx, given)?.into_iter().collect();
            } else {
                explicit_data = given;
            }
        }
        if let Some(value) = defaults.remove(".Data") {
            explicit_data = value;
        }

        let undefined: Vec<_> = defaults.keys()
            .filter(|name| !slots.iter().any(|(slot, _)| slot == *name)).collect();
        if !undefined.is_empty() {
            let mut names = undefined.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>();
            names.sort();
            return Err(error(ctx, call, format!("The prototype for class “{class}” has undefined slot(s): {}", names.join(", "))));
        }
        let prototype = if let Some(data) = &data {
            let source = if explicit_data != SEXP_NIL {
                if !relations::matches(ctx, explicit_data, data, env)? {
                    return Err(error(ctx, call, format!("prototype data does not extend class “{data}”")));
                }
                explicit_data
            } else if let Some(parent) = ancestors.iter().find(|p| {
                cached_data_class(ctx, p).as_ref() == Some(data)
            }) {
                decode_slot(attr(ctx, parent.class_def, "prototype")?)
            } else {
                default_value(ctx, data, env)?
            };
            if matches!(ctx.session().type_of(source)?, SexpType::Nil | SexpType::Sym | SexpType::Env | SexpType::ExtPtr) {
                return Err(error(ctx, call, format!("in constructing the prototype for class “{class}”: prototype has class “S4”, but the data part specifies class “{data}”")));
            }
            objects::data_copy(ctx, source, data)?
        } else {
            ctx.session_mut().alloc_s4_object()?
        };
        ctx.protect(prototype);
        ctx.session_mut().set_s4_flag(prototype, true)?;
        let mut missing = Vec::new();
        for (slot, typ) in slots.iter().filter(|(slot, _)| slot != ".Data") {
            let value = if let Some(val) = defaults.get(slot) {
                if !relations::matches(ctx, *val, typ, env)? {
                    return Err(error(ctx, call, format!("in making the prototype for class “{class}” elements of the prototype failed to match the corresponding slot class: {slot} (class \"{typ}\" )")));
                }
                *val
            } else {
                let mut inherited = SEXP_NIL;
                for parent in &ancestors {
                    let source = decode_slot(attr(ctx, parent.class_def, "prototype")?);
                    inherited = attr(ctx, source, slot)?;
                    if inherited != SEXP_NIL { break; }
                }
                if inherited == SEXP_NIL {
                    if lookup(ctx, typ, None, env, true)?.is_none() && typ != &class {
                        missing.push(format!("{slot}(class \"{typ}\")"));
                    }
                    default_value(ctx, typ, env)?
                } else {
                    decode_slot(inherited)
                }
            };
            write_slot(ctx, prototype, slot, value)?;
        }
        if !missing.is_empty() {
            ctx.warn(call.err_call(), format!("undefined slot classes in definition of \"{class}\": {}", missing.join(", ")));
        }
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

fn cached_data_class(ctx: &mut Ctx, info: &ClassInfo) -> Option<String> {
    ctx.ext::<ClassState>()
        .records
        .get(&key(info))
        .and_then(|r| r.data.clone())
}

fn default_value(ctx: &mut Ctx, class: &str, env: SexpId) -> EvalResult {
    let Some(info) = lookup(ctx, class, None, env, true)? else {
        return Ok(SEXP_NIL);
    };
    if matches!(class, "ANY" | "NULL" | "VIRTUAL") {
        return Ok(SEXP_NIL);
    }
    let value = decode_slot(attr(ctx, info.class_def, "prototype")?);
    if info.is_virtual || !ctx.session().is_s4(value)? {
        return Ok(value);
    }
    objects::stamp_instance(ctx, value, &info)
}

pub(super) fn special_class(
    ctx: &mut Ctx,
    name: String,
    env: SexpId,
    slots: Vec<(String, String)>,
    parents: Vec<Edge>,
    prototype: SexpId,
) -> EvalResult<ClassInfo> {
    let package = package_name(ctx, env)?;
    let def = Definition {
        info: ClassInfo {
            name,
            package,
            slots,
            superclasses: Vec::new(),
            is_virtual: true,
            class_def: SEXP_NIL,
        },
        record: Record {
            where_: env,
            edges: parents,
            data: None,
        },
        prototype,
        validity: SEXP_NIL,
        access: make_list(ctx, &[], false)?,
        sealed: false,
    };
    install(ctx, def)
}

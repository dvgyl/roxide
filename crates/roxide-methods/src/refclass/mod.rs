//! Environment-backed S4 reference classes.

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, CtxExt, Dispatch, EvalResult, Module, PrimDef, PrimKind,
    Visibility,
};
use std::collections::HashSet;

use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{
    Binding, EMPTY_ENV, SEXP_MISSING, SEXP_NIL, SEXP_S4_NULL, SexpType, Sym, ValueApi,
};

use crate::class::{definitions::object_info, objects::stamp_instance, relations};
use crate::dispatch::metadata::{attr, set_attr, set_class, texts};
use crate::registry::{ClassInfo, MethodsExt};

mod binding;
mod class_meta;

/// Reference class definitions and operations in the methods namespace.
pub const MODULE: Module = Module {
    prims: &[
        PrimDef {
            name: ".ref_unique",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(1),
            dispatch: Dispatch::None,
            visibility: Visibility::On,
            fun: unique,
        },
        primitive(".ref_member", member, Arity::Exact(2)),
        primitive(".ref_replace", replace, Arity::Exact(3)),
        primitive(".ref_field_value", field_value, Arity::Any),
        primitive(".ref_object_meta", object_meta, Arity::Exact(1)),
        primitive(".ref_definition", definition, Arity::Exact(1)),
        primitive(".ref_generator", class_meta::generator, Arity::Exact(1)),
        primitive(
            ".ref_generator_access",
            class_meta::generator_access,
            Arity::Exact(2),
        ),
        primitive(".ref_field_types", class_meta::field_types, Arity::Exact(1)),
        primitive(
            ".ref_lookup_method",
            binding::lookup_method,
            Arity::Exact(2),
        ),
        primitive(".ref_super", binding::super_method, Arity::Exact(3)),
        primitive(
            ".ref_initialize_fields",
            binding::initialize_fields,
            Arity::Exact(2),
        ),
        primitive(".ref_copy", binding::copy, Arity::Exact(2)),
        primitive(".ref_seed", seed, Arity::Exact(2)),
        primitive(".ref_method_names", binding::method_names, Arity::Exact(1)),
        primitive(".ref_invalidate", invalidate, Arity::Exact(0)),
        PrimDef {
            name: ".ref_method_help",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(2),
            dispatch: Dispatch::None,
            visibility: Visibility::Off,
            fun: method_help,
        },
        primitive(".ref_class_meta", class_meta::create, Arity::Exact(6)),
        primitive(
            ".ref_finish_definition",
            class_meta::finish,
            Arity::Exact(2),
        ),
        primitive(".ref_add_methods", class_meta::add_methods, Arity::Exact(2)),
        PrimDef {
            name: ".ref_tag",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(2),
            dispatch: Dispatch::None,
            visibility: Visibility::On,
            fun: tag,
        },
        PrimDef {
            name: ".ref_register_access",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(3),
            dispatch: Dispatch::None,
            visibility: Visibility::Off,
            fun: register_access,
        },
    ],
    prelude: concat!(
        include_str!("definitions.R"),
        "\n",
        include_str!("instances.R"),
        "\n",
        include_str!("registration.R"),
    ),
};

fn unique(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let session = ctx.session_mut();
    let mut seen = HashSet::new();
    let keys: Vec<_> = session
        .str_slice(args.get(0))?
        .iter()
        .copied()
        .filter(|&key| seen.insert(key))
        .collect();
    Ok(session.alloc_str_from(&keys)?)
}

const fn primitive(name: &'static str, fun: roxide_eval::PrimFn, arity: Arity) -> PrimDef {
    PrimDef {
        name,
        fun,
        arity,
        kind: PrimKind::Primitive,
        args: ArgEval::Eval,
        dispatch: Dispatch::None,
        visibility: Visibility::On,
    }
}

#[derive(Default)]
struct RefEpoch(u64);

impl Trace for RefEpoch {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for RefEpoch {}

fn invalidate(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let epoch = ctx.ext::<RefEpoch>();
    epoch.0 = epoch.0.wrapping_add(1);
    Ok(SEXP_NIL)
}

fn namespace(ctx: &Ctx) -> SexpId {
    ctx.ns_lookup("methods").unwrap_or(EMPTY_ENV)
}

fn local(ctx: &mut Ctx, env: SexpId, name: &str) -> EvalResult {
    let tag = ctx.session_mut().intern(name)?;
    Ok(ctx.get_local(tag, env)?.unwrap_or(SEXP_NIL))
}

fn define(ctx: &mut Ctx, env: SexpId, name: &str, value: SexpId) -> EvalResult<()> {
    let tag = ctx.session_mut().intern(name)?;
    Ok(ctx.session_mut().env_define(env, tag, value)?)
}

fn field_name(ctx: &mut Ctx, value: SexpId) -> EvalResult<String> {
    if let Ok(name) = ctx.session().sym_name_of(value) {
        return Ok(name.to_owned());
    }
    Ok(texts(ctx, value)?.into_iter().next().unwrap_or_default())
}

fn definition(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    class_definition(ctx, args.get(0))
}

fn class_definition(ctx: &mut Ctx, meta: SexpId) -> EvalResult {
    let epoch = ctx.ext::<MethodsExt>().epoch;
    let saved = local(ctx, meta, "definitionEpoch")?;
    if saved != SEXP_NIL && ctx.session().real(saved)?.iter().next() == Some(epoch as f64) {
        return local(ctx, meta, "resolvedDefinition");
    }
    let name = local(ctx, meta, "name")?;
    let name = field_name(ctx, name)?;
    let package = local(ctx, meta, "package")?;
    let package = field_name(ctx, package)?;
    let where_ = local(ctx, meta, "where")?;
    let info = crate::class::definitions::lookup(ctx, &name, Some(&package), where_, true)?
        .ok_or_else(|| ctx.stop(SEXP_NIL, format!("“{name}” is not a defined class")))?;
    let def = if attr(ctx, info.class_def, ".rx_ref_meta")? == SEXP_NIL {
        local(ctx, meta, "def")?
    } else {
        info.class_def
    };
    let epoch = ctx.session_mut().scalar_real(epoch as f64)?;
    define(ctx, meta, "resolvedDefinition", def)?;
    define(ctx, meta, "definitionEpoch", epoch)?;
    Ok(def)
}

fn instance_meta(ctx: &mut Ctx, data: SexpId) -> EvalResult {
    let def = local(ctx, data, ".refClassDef")?;
    attr(ctx, def, ".rx_ref_meta")
}

fn object_meta(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    if ctx.session().is_s4(value)?
        && let Some(info) = object_info(ctx, value)?
    {
        return attr(ctx, info.class_def, ".rx_ref_meta");
    }
    Ok(SEXP_NIL)
}

fn member(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    let name = args.get(1);
    let key = field_name(ctx, name)?;
    let sym = ctx.session_mut().intern(&key)?;
    let data = attr(ctx, object, ".xData")?;
    if let Some(value) = ctx.get_local(sym, data)? {
        return Ok(value);
    }
    let class = attr(ctx, object, "class")?;
    let class = field_name(ctx, class)?;
    let name = ctx.session_mut().scalar_str(&key)?;
    ctx.protect(name);
    if class == "refGeneratorSlot" {
        return class_meta::generator_member(ctx, object, &key);
    }
    let meta = instance_meta(ctx, data)?;
    let Some(method) = binding::find(ctx, meta, &key, true)? else {
        let name = local(ctx, meta, "name")?;
        let name = field_name(ctx, name)?;
        return Err(ctx.stop(
            SEXP_NIL,
            format!("‘{key}’ is not a valid field or method name for reference class “{name}”"),
        ));
    };
    let method = binding::bind(ctx, object, &key, method)?;
    ctx.session_mut().env_define(data, sym, method)?;
    Ok(method)
}

fn replace(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    let data = attr(ctx, object, ".xData")?;
    let key = field_name(ctx, args.get(1))?;
    let meta = instance_meta(ctx, data)?;
    let fields = local(ctx, meta, "fields")?;
    let names = attr(ctx, fields, "names")?;
    if !texts(ctx, names)?.contains(&key) {
        let class = local(ctx, meta, "name")?;
        let class = field_name(ctx, class)?;
        return Err(ctx.stop(
            SEXP_NIL,
            format!("‘{key}’ is not a field in class “{class}”"),
        ));
    }
    let sym = ctx.session_mut().intern(&key)?;
    ctx.assign(sym, args.get(2), data)?;
    ctx.set_visible(false);
    Ok(object)
}

fn field_value(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let scope = ctx.session().env_enclos(call.env)?;
    let data = local(ctx, scope, "data")?;
    let key = local(ctx, scope, "key")?;
    let key = field_name(ctx, key)?;
    let backing = ctx.session_mut().intern(&format!(".->{key}"))?;
    if args.is_empty() {
        return Ok(ctx.get_local(backing, data)?.unwrap_or(SEXP_NIL));
    }
    let locked = local(ctx, scope, "locked")? == roxide_value::TRUE_SCALAR;
    if locked && ctx.session().env_binding_is_locked(data, backing)? == Some(true) {
        return Err(ctx.stop(
            SEXP_NIL,
            format!("invalid replacement: reference class field ‘{key}’ is read-only"),
        ));
    }
    let value = args.get(0);
    let spec = local(ctx, scope, "spec")?;
    let spec = field_name(ctx, spec)?;
    if !relations::matches(ctx, value, &spec, data)? {
        let class = relations::runtime_class(ctx, value)?;
        return Err(ctx.stop(SEXP_NIL, format!("invalid assignment for reference class field ‘{key}’, should be from class “{spec}” or a subclass (was class “{class}”)")));
    }
    ctx.session_mut().env_define(data, backing, value)?;
    if locked {
        ctx.session_mut().env_lock_binding(data, backing, true)?;
    }
    ctx.set_visible(false);
    Ok(value)
}

fn active(ctx: &mut Ctx, env: SexpId, name: Sym, fun: SexpId) -> EvalResult<()> {
    Ok(ctx.session_mut().env_define_binding(
        env,
        name,
        Binding {
            value: fun,
            active: true,
            locked: false,
            missing: 0,
        },
    )?)
}

fn seed(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    let meta = args.get(1);
    let ns = namespace(ctx);
    let fields = local(ctx, meta, "fields")?;
    let names = attr(ctx, fields, "names")?;
    let names = texts(ctx, names)?;
    let specs = ctx.session().list_slice(fields)?.to_vec();
    let locks = local(ctx, meta, "locks")?;
    let locks = texts(ctx, locks)?;
    let where_ = local(ctx, meta, "where")?;
    let def = class_definition(ctx, meta)?;
    let data = attr(ctx, object, ".xData")?;
    let parent = local(ctx, meta, "parentEnv")?;
    let methods = ctx.session_mut().alloc_env(parent)?;
    ctx.session_mut().env_set_enclos(data, methods)?;
    for (key, value) in [(".self", object), (".refClassDef", def)] {
        define(ctx, data, key, value)?;
        let sym = ctx.session_mut().intern(key)?;
        ctx.session_mut().env_lock_binding(data, sym, true)?;
    }

    let prototypes = field_prototypes(ctx, meta, where_, &names, &specs)?;
    let template = local(ctx, ns, ".ref_field_binding")?;
    let (formals, body, _) = ctx.session().closure_parts(template)?;
    for ((key, spec), prototype) in names.iter().zip(specs).zip(prototypes) {
        let sym = ctx.session_mut().intern(key)?;
        if ctx.session().type_of(spec)? == SexpType::Clo {
            let (formals, body, _) = ctx.session().closure_parts(spec)?;
            let fun = ctx.session_mut().alloc_closure(formals, body, data)?;
            active(ctx, data, sym, fun)?;
            continue;
        }
        let class = field_name(ctx, spec)?;
        let initial = if class == "ANY" {
            let initial = ctx.session_mut().alloc_s4_object()?;
            let name = ctx.session_mut().scalar_str(key)?;
            set_class(ctx, initial, "uninitializedField")?;
            set_attr(ctx, initial, "field", name)?;
            set_attr(ctx, initial, "className", spec)?;
            initial
        } else {
            let info = prototype.ok_or_else(|| {
                ctx.stop(call.err_call(), format!("class “{class}” is not defined"))
            })?;
            initial_field(ctx, &info, ns)?
        };
        if class == "ANY" {
            ctx.session_mut().env_define(data, sym, initial)?;
            continue;
        }
        let backing = ctx.session_mut().intern(&format!(".->{key}"))?;
        ctx.session_mut().env_define(data, backing, initial)?;
        let scope = ctx.session_mut().alloc_env(ns)?;
        let name = ctx.session_mut().scalar_str(key)?;
        let locked = if locks.contains(key) {
            roxide_value::TRUE_SCALAR
        } else {
            roxide_value::FALSE_SCALAR
        };
        for (key, value) in [
            ("data", data),
            ("key", name),
            ("spec", spec),
            ("locked", locked),
        ] {
            define(ctx, scope, key, value)?;
        }
        let fun = ctx.session_mut().alloc_closure(formals, body, scope)?;
        active(ctx, data, sym, fun)?;
    }
    let names = binding::cached_method_names(ctx, meta)?;
    let template = local(ctx, ns, ".ref_method_binding")?;
    let (formals, body, _) = ctx.session().closure_parts(template)?;
    let scope = ctx.session_mut().alloc_env(ns)?;
    define(ctx, scope, "object", object)?;
    for name in texts(ctx, names)? {
        let sym = ctx.session_mut().intern(&name)?;
        let binding_scope = ctx.session_mut().alloc_env(scope)?;
        let key = ctx.session_mut().scalar_str(&name)?;
        define(ctx, binding_scope, "key", key)?;
        let getter = ctx
            .session_mut()
            .alloc_closure(formals, body, binding_scope)?;
        active(ctx, methods, sym, getter)?;
    }
    binding::find(ctx, meta, "initialize", true)?.map_or(Ok(SEXP_NIL), |method| {
        binding::bind(ctx, object, "initialize", method)
    })
}

fn epoch_matches(ctx: &mut Ctx, cache: SexpId, key: &str, epoch: u64) -> EvalResult<bool> {
    let value = local(ctx, cache, key)?;
    Ok(ctx.session().real(value)?.iter().next() == Some(epoch as f64))
}

fn field_prototypes(
    ctx: &mut Ctx,
    meta: SexpId,
    where_: SexpId,
    names: &[String],
    specs: &[SexpId],
) -> EvalResult<Vec<Option<ClassInfo>>> {
    let class_epoch = ctx.ext::<MethodsExt>().epoch;
    let ref_epoch = ctx.ext::<RefEpoch>().0;
    let cache = local(ctx, meta, "prototypeCache")?;
    let entries = if cache != SEXP_NIL
        && epoch_matches(ctx, cache, "classEpoch", class_epoch)?
        && epoch_matches(ctx, cache, "refEpoch", ref_epoch)?
    {
        local(ctx, cache, "definitions")?
    } else {
        let entries = ctx.session_mut().alloc_list(specs.len())?;
        ctx.protect(entries);
        for (i, (&spec, key)) in specs.iter().zip(names).enumerate() {
            if ctx.session().type_of(spec)? != SexpType::Str {
                continue;
            }
            let name = field_name(ctx, spec)?;
            if name == "ANY" {
                continue;
            }
            let info = crate::class::definitions::lookup(ctx, &name, None, where_, true)?
                .ok_or_else(|| {
                    ctx.stop(
                        SEXP_NIL,
                        format!("class “{name}” for field ‘{key}’ is not defined"),
                    )
                })?;
            ctx.session_mut().list_set(entries, i, info.class_def)?;
        }
        let cache = ctx.session_mut().alloc_env(EMPTY_ENV)?;
        let class_epoch = ctx.session_mut().scalar_real(class_epoch as f64)?;
        let ref_epoch = ctx.session_mut().scalar_real(ref_epoch as f64)?;
        define(ctx, cache, "classEpoch", class_epoch)?;
        define(ctx, cache, "refEpoch", ref_epoch)?;
        define(ctx, cache, "definitions", entries)?;
        define(ctx, meta, "prototypeCache", cache)?;
        entries
    };
    ctx.protect(entries);
    ctx.session()
        .list_slice(entries)?
        .to_vec()
        .into_iter()
        .map(|def| {
            if def == SEXP_NIL {
                Ok(None)
            } else {
                crate::class::definitions::definition_info(ctx, def).map(Some)
            }
        })
        .collect()
}

fn initial_field(ctx: &mut Ctx, info: &ClassInfo, ns: SexpId) -> EvalResult {
    let prototype = attr(ctx, info.class_def, "prototype")?;
    let prototype = if prototype == SEXP_S4_NULL {
        SEXP_NIL
    } else {
        prototype
    };
    if info.is_virtual {
        Ok(prototype)
    } else if ctx.session().is_s4(prototype)? {
        ctx.call_function("new", &[(None, info.class_def)], ns)
    } else {
        stamp_instance(ctx, prototype, info)
    }
}

fn method_help(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let session = ctx.session();
    let Some(&name) = session.str_slice(args.get(0))?.first() else {
        return Err(ctx.stop(call.err_call(), "expected a method name"));
    };
    let name = String::from_utf8_lossy(session.char_bytes(name)?);
    let (formals, mut body, _) = session.closure_parts(args.get(1))?;
    let parameters = session
        .pairlist_iter(formals)
        .filter_map(|(tag, value)| {
            let name = session.sym_name(tag?).ok()?;
            Some(if value == SEXP_MISSING {
                name.to_owned()
            } else {
                format!("{name} = ")
            })
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut output = format!("Call:\n${name}({parameters})\n\n\n");
    if session.type_of(body)? == SexpType::Lang {
        let (head, tail, _) = session.cons_parts(body)?;
        if session.sym_name_of(head) == Ok("{") {
            body = session
                .pairlist_iter(tail)
                .next()
                .map_or(SEXP_NIL, |(_, expr)| expr);
        }
    }
    if session.type_of(body)? == SexpType::Str {
        for &line in session.str_slice(body)? {
            output.push_str(&String::from_utf8_lossy(session.char_bytes(line)?));
            output.push('\n');
        }
        output.push('\n');
    }
    let _ = ctx.host().write_out(output.as_bytes());
    Ok(SEXP_NIL)
}

fn tag(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    let class = args.get(1);
    let session = ctx.session_mut();
    let (formals, body, env) = session.closure_parts(object)?;
    let copy = session.alloc_closure(formals, body, env)?;
    for (attribute, value) in session.attrs(object)? {
        session.set_attrib(copy, attribute, value)?;
    }
    let tag = session.intern("class")?;
    session.set_attrib(copy, tag, class)?;
    session.set_s4_flag(copy, true)?;
    Ok(copy)
}

fn register_access(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    // Primitive replacement uses the base S3 method table when no S4 method handles it.
    for (generic, class, index) in [
        ("$", "envRefClass", 0),
        ("$<-", "envRefClass", 1),
        ("$", "refObjectGenerator", 2),
    ] {
        ctx.ns_register_s3("base", generic, class, args.get(index))?;
    }
    Ok(SEXP_NIL)
}

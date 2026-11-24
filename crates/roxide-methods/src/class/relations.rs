use std::collections::HashSet;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use crate::registry::{ClassInfo, MethodsExt};

use super::definitions::{
    ClassState, Edge, Key, cached, class_spec, data_class, key, lookup, methods_env, object_info,
    refresh, refresh_edges, resolve, special_class,
};
use super::inheritance::path;
use super::objects::{data_copy, set_data, slot_value, stamp_instance};
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Conversion {
    pub(super) forward: SexpId,
    pub(super) reverse: SexpId,
    pub(super) test: SexpId,
}

impl Default for Conversion {
    fn default() -> Self {
        Self {
            forward: SEXP_NIL,
            reverse: SEXP_NIL,
            test: SEXP_NIL,
        }
    }
}

impl Trace for Conversion {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.edge(self.forward);
        tracer.edge(self.reverse);
        tracer.edge(self.test);
    }
}

fn actual_classes(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<String>> {
    let class = attr(ctx, value, "class")?;
    if class != SEXP_NIL {
        return text_vec(ctx, class);
    }
    if value == SEXP_MISSING {
        return Ok(vec!["missing".into()]);
    }
    let dim = attr(ctx, value, "dim")?;
    if dim != SEXP_NIL {
        if ctx.session().xlength(dim)? == 2 {
            return Ok(vec!["matrix".into(), "array".into()]);
        }
        return Ok(vec!["array".into()]);
    }
    let class = match ctx.session().type_of(value)? {
        SexpType::Real => "numeric",
        SexpType::Clo | SexpType::Builtin | SexpType::Special => "function",
        SexpType::Sym => "name",
        SexpType::Lang => "call",
        typ => typ.type_name(false),
    };
    Ok(vec![class.into()])
}

pub(crate) fn runtime_class(ctx: &mut Ctx, value: SexpId) -> EvalResult<String> {
    Ok(actual_classes(ctx, value)?
        .into_iter()
        .next()
        .unwrap_or_else(|| "NULL".into()))
}

fn object_classes(
    ctx: &mut Ctx,
    obj: SexpId,
    env: SexpId,
) -> EvalResult<Vec<(String, Option<ClassInfo>)>> {
    let mut classes = Vec::new();
    for (i, name) in actual_classes(ctx, obj)?.into_iter().enumerate() {
        let info = if i == 0 {
            if let Some(info) = object_info(ctx, obj)? {
                Some(info)
            } else {
                lookup(ctx, &name, None, env, true)?
            }
        } else {
            lookup(ctx, &name, None, env, true)?
        };
        classes.push((name, info));
    }
    Ok(classes)
}

pub(crate) fn matches(
    ctx: &mut Ctx,
    object: SexpId,
    target: &str,
    env: SexpId,
) -> EvalResult<bool> {
    if target == "ANY" {
        return Ok(true);
    }
    for (class, info) in object_classes(ctx, object, env)? {
        if class == target || class == "numeric" && target == "double" {
            return Ok(true);
        }
        if let Some(info) = info
            && info.superclasses.iter().any(|(name, _, _)| name == target)
        {
            let conversion = edge_conversion(ctx, &info, target);
            if let Some(conversion) = conversion
                && conversion.test != SEXP_NIL
            {
                let result = ctx.scoped(|ctx| {
                    ctx.protect(conversion.test);
                    ctx.apply(conversion.test, &[(None, object)], SEXP_NIL, env)
                })?;
                if !flag(ctx, result)? {
                    continue;
                }
            }
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn is(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    let target = args.get(1);
    let env = args.get(2);
    if target != SEXP_NIL {
        let (target, _) = class_spec(ctx, target, call.err_call())?;
        let result = matches(ctx, object, &target, env)?;
        return logical(ctx, result);
    }
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for (class, info) in object_classes(ctx, object, env)? {
        for name in std::iter::once(class).chain(
            info.into_iter()
                .flat_map(|info| info.superclasses.into_iter().map(|(name, _, _)| name)),
        ) {
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
    }
    strings(ctx, names.iter().map(String::as_str))
}

pub(super) fn extends(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = args.get(4);
    let (class, pkg) = class_spec(ctx, args.get(0), call.err_call())?;
    let full_info = flag(ctx, args.get(2))?;
    let info = lookup(ctx, &class, pkg.as_deref(), env, true)?;
    if args.get(1) == SEXP_NIL {
        return match info {
            Some(info) if full_info => {
                let contains = attr(ctx, info.class_def, "contains")?;
                let mut entries = named_values(ctx, contains)?;
                entries.push((class, logical(ctx, true)?));
                make_list(ctx, &entries, true)
            }
            Some(info) => strings(ctx, names_in_chain(&info)),
            None if full_info => {
                let value = logical(ctx, true)?;
                make_list(ctx, &[(class, value)], true)
            }
            None => strings(ctx, [class.as_str()]),
        };
    }
    let (target, _) = class_spec(ctx, args.get(1), call.err_call())?;
    if target == "ANY" || class == target {
        return logical(ctx, true);
    }
    if let Some(info) = info
        && info.superclasses.iter().any(|(name, _, _)| name == &target)
    {
        let contains = attr(ctx, info.class_def, "contains")?;
        let entries = named_values(ctx, contains)?;
        if let Some((_, extension)) = entries.into_iter().find(|(n, _)| n == &target) {
            if full_info && (info.package != "methods" || !info.slots.is_empty()) {
                return Ok(extension);
            }
            if edge_conversion(ctx, &info, &target).is_some_and(|m| m.test != SEXP_NIL) {
                return Ok(args.get(3));
            }
        }
        return logical(ctx, true);
    }
    logical(ctx, false)
}

fn require_class(ctx: &mut Ctx, value: SexpId, env: SexpId, call: &Call) -> EvalResult<ClassInfo> {
    if let Some(info) = resolve(ctx, value, env, call.err_call())? {
        return Ok(info);
    }
    let (name, _) = class_spec(ctx, value, call.err_call())?;
    Err(bad_class(ctx, call.err_call(), &name))
}

pub(super) fn union(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = name(ctx, args.get(0), call.err_call())?;
    let env = args.get(2);
    let mut members = Vec::new();
    let mut missing = Vec::new();
    for member in text_vec(ctx, args.get(1))? {
        if member == name {
            return Err(error(ctx, call, "a class union cannot contain itself"));
        }
        if let Some(info) = lookup(ctx, &member, None, env, true)? {
            members.push(key(&info));
        } else {
            missing.push(member);
        }
    }
    if !missing.is_empty() {
        return Err(error(
            ctx,
            call,
            format!("the member classes must be defined: {}", missing.join(", ")),
        ));
    }
    let info = special_class(ctx, name, env, Vec::new(), Vec::new(), SEXP_NIL, None)?;
    let union_key = key(&info);
    let state = ctx.ext::<ClassState>();
    for record in state.records.values_mut() {
        record.edges.retain(|edge| edge.target != union_key);
    }
    for member in &members {
        if let Some(record) = state.records.get_mut(member) {
            record.edges.push(Edge::simple(&info));
        }
    }
    state.unions.insert(union_key, members);
    refresh(ctx)?;
    Ok(info.class_def)
}

pub(super) fn old_class(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let classes = text_vec(ctx, args.get(0))?;
    let env = args.get(2);
    if classes.is_empty() {
        return Err(error(ctx, call, "no class names were supplied"));
    }
    let template = (args.get(3) != SEXP_NIL)
        .then(|| require_class(ctx, args.get(3), env, call))
        .transpose()?;
    let mut parent = cached(ctx, "oldClass", Some("methods"));
    for (i, name) in classes.iter().enumerate().rev() {
        if let Some(info) = lookup(ctx, name, None, env, true)? {
            parent = Some(info);
            continue;
        }
        let value = strings(ctx, classes[i..].iter().map(String::as_str))?;
        let template = template.as_ref().filter(|_| i == 0);
        let mut slots = template.map_or_else(Vec::new, |info| info.slots.clone());
        slots.retain(|(name, _)| name != ".S3Class");
        slots.push((".S3Class".into(), "character".into()));
        let prototype = if let Some(info) = template {
            let original = decode_slot(attr(ctx, info.class_def, "prototype")?);
            ctx.session_mut().shallow_duplicate(original)?
        } else if i == 0 && args.get(1) != SEXP_NIL {
            ctx.session_mut().shallow_duplicate(args.get(1))?
        } else {
            ctx.session_mut().alloc_s4_object()?
        };
        write_slot(ctx, prototype, ".S3Class", value)?;
        let info = special_class(
            ctx,
            name.clone(),
            env,
            slots,
            parent.iter().map(Edge::simple).collect(),
            prototype,
            template,
        )?;
        parent = Some(info);
    }
    refresh(ctx)?;
    Ok(SEXP_NIL)
}

fn edge_conversion(ctx: &mut Ctx, source: &ClassInfo, target: &str) -> Option<Conversion> {
    ctx.ext::<ClassState>()
        .records
        .get(&key(source))?
        .edges
        .iter()
        .find(|edge| edge.target.0 == target)?
        .methods
}

fn check_conversion(ctx: &mut Ctx, call: &Call, values: &[SexpId]) -> EvalResult<()> {
    for &fun in values {
        if fun != SEXP_NIL && !callable(ctx, fun)? {
            return Err(error(ctx, call, "the supplied method must be a function"));
        }
    }
    Ok(())
}

pub(super) fn set_is(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = args.get(5);
    let from = require_class(ctx, args.get(0), env, call)?;
    let to = require_class(ctx, args.get(1), env, call)?;
    if names_in_chain(&to).any(|name| name == from.name) {
        return Err(error(
            ctx,
            call,
            format!("class “{}” cannot extend itself", from.name),
        ));
    }
    let methods = Conversion {
        test: args.get(2),
        forward: args.get(3),
        reverse: args.get(4),
    };
    check_conversion(ctx, call, &[methods.test, methods.forward, methods.reverse])?;
    if methods.forward == SEXP_NIL {
        for (slot, _) in &to.slots {
            if !from.slots.iter().any(|(name, _)| name == slot) {
                return Err(error(
                    ctx,
                    call,
                    format!(
                        "class “{}” cannot extend class “{}”: class “{}” is missing slot from class “{}” ({slot}), and no coerce method was supplied",
                        from.name, to.name, from.name, to.name
                    ),
                ));
            }
        }
    }
    let Some(record) = ctx.ext::<ClassState>().records.get_mut(&key(&from)) else {
        return Err(error(
            ctx,
            call,
            format!("cannot modify the definition of class “{}”", from.name),
        ));
    };
    let parent = key(&to);
    let custom = [methods.test, methods.forward, methods.reverse]
        .iter()
        .any(|&f| f != SEXP_NIL);
    let new_edge = Edge {
        target: parent.clone(),
        methods: custom.then_some(methods),
    };
    if let Some(edge) = record.edges.iter_mut().find(|edge| edge.target == parent) {
        *edge = new_edge;
    } else {
        record.edges.push(new_edge);
    }
    refresh_edges(ctx, Some((&key(&from), &parent)))?;
    logical(ctx, true)
}

pub(super) fn set_as(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = args.get(4);
    let from = require_class(ctx, args.get(0), env, call)?;
    let to = require_class(ctx, args.get(1), env, call)?;
    let forward = args.get(2);
    let reverse = args.get(3);
    check_conversion(ctx, call, &[forward, reverse])?;
    if forward == SEXP_NIL {
        return Err(error(
            ctx,
            call,
            "the coercion definition must be a function",
        ));
    }
    ctx.ext::<ClassState>().conversions.insert(
        (key(&from), key(&to)),
        Conversion {
            forward,
            reverse,
            test: SEXP_NIL,
        },
    );
    ctx.ext::<MethodsExt>().bump_epoch();
    Ok(SEXP_NIL)
}

fn conversion(
    ctx: &mut Ctx,
    source: &str,
    target: &str,
    info: Option<&ClassInfo>,
) -> Option<(String, Conversion)> {
    let from = info.cloned().or_else(|| cached(ctx, source, None))?;
    let to = cached(ctx, target, None)?;
    let to = key(&to);
    for name in names_in_chain(&from) {
        let source = if name == from.name {
            Some(from.clone())
        } else {
            cached(ctx, name, None)
        };
        if let Some(source) = source
            && let Some(conversion) = ctx
                .ext::<ClassState>()
                .conversions
                .get(&(key(&source), to.clone()))
        {
            return Some((source.name, *conversion));
        }
    }
    None
}

fn apply_conversion(
    ctx: &mut Ctx,
    fun: SexpId,
    values: &[(Option<Sym>, SexpId)],
    target: &str,
    strict: bool,
    env: SexpId,
    call: &Call,
) -> EvalResult {
    ctx.scoped(|ctx| {
        let mut values = values.to_vec();
        if ctx.session().type_of(fun)? == SexpType::Clo {
            let (formals, _, _) = ctx.session().closure_parts(fun)?;
            let tags = ctx
                .session()
                .pairlist_iter(formals)
                .filter_map(|(tag, _)| tag)
                .collect::<Vec<_>>();
            let to = ctx.session_mut().intern("to")?;
            let strict_sym = ctx.session_mut().intern("strict")?;
            if tags.contains(&to) {
                let value = ctx.session_mut().scalar_str(target)?;
                ctx.protect(value);
                values.push((Some(to), value));
            }
            if tags.contains(&strict_sym) {
                let value = logical(ctx, strict)?;
                values.push((Some(strict_sym), value));
            }
        }
        ctx.apply(fun, &values, call.err_call(), env)
    })
}

fn table_conversion(
    ctx: &mut Ctx,
    source: &str,
    target: &str,
    env: SexpId,
    call: &Call,
) -> EvalResult<Option<SexpId>> {
    if ctx
        .ext::<MethodsExt>()
        .find_generic("coerce", None)
        .is_none()
    {
        return Ok(None);
    }
    let ns = methods_env(ctx);
    let sym = ctx.session_mut().intern("selectMethod")?;
    let Some(selector) = ctx.get_var(sym, env)?.or(ctx.get_local(sym, ns)?) else {
        return Ok(None);
    };
    let generic = ctx.session_mut().scalar_str("coerce")?;
    let signature = strings(ctx, [source, target])?;
    let optional = ctx.session_mut().intern("optional")?;
    let yes = logical(ctx, true)?;
    let fun = ctx.apply(
        selector,
        &[(None, generic), (None, signature), (Some(optional), yes)],
        call.err_call(),
        env,
    )?;
    if fun == SEXP_NIL {
        Ok(None)
    } else {
        Ok(Some(fun))
    }
}

fn project(
    ctx: &mut Ctx,
    object: SexpId,
    info: &ClassInfo,
    source: Option<&ClassInfo>,
) -> EvalResult {
    if info.slots.is_empty() && info.package == "methods" {
        let data = source.and_then(|info| data_class(ctx, info));
        if ctx.session().is_s4(object)? {
            return data_copy(ctx, object, data.as_deref().unwrap_or(&info.name));
        }
        return Ok(object);
    }
    let prototype = decode_slot(attr(ctx, info.class_def, "prototype")?);
    let mut out = stamp_instance(ctx, prototype, info)?;
    if let Some(data) = data_class(ctx, info) {
        out = set_data(ctx, out, object, &data)?;
    }
    for (slot, _) in info.slots.iter().filter(|(slot, _)| slot != ".Data") {
        let value = slot_value(ctx, object, slot)?;
        write_slot(ctx, out, slot, value)?;
    }
    Ok(out)
}

fn union_member(ctx: &mut Ctx, target: &ClassInfo, source: &ClassInfo) -> Option<String> {
    let members = ctx.ext::<ClassState>().unions.get(&key(target))?;
    names_in_chain(source)
        .find(|name| members.iter().any(|key| key.0 == *name))
        .map(str::to_owned)
}

fn conversion_builtin(class: &str) -> Option<&'static str> {
    Some(match class {
        "logical" => "as.logical",
        "integer" => "as.integer",
        "double" | "numeric" => "as.double",
        "complex" => "as.complex",
        "character" => "as.character",
        "raw" => "as.raw",
        "list" => "as.list",
        "expression" => "as.expression",
        "matrix" => "as.matrix",
        "array" => "as.array",
        "data.frame" => "as.data.frame",
        "name" => "as.name",
        "call" => "as.call",
        "environment" => "as.environment",
        "factor" => "as.factor",
        "ordered" => "as.ordered",
        _ => return None,
    })
}

enum InheritanceConversion {
    NoCustomPath,
    Rejected,
    Converted(SexpId),
}

fn inheritance_conversion(
    ctx: &mut Ctx,
    mut value: SexpId,
    mut source: Key,
    target: &str,
    strict: bool,
    env: SexpId,
    call: &Call,
) -> EvalResult<InheritanceConversion> {
    let Some(route) = path(&ctx.ext::<ClassState>().records, &source, target) else {
        return Ok(InheritanceConversion::NoCustomPath);
    };
    if !route.iter().any(|edge| edge.methods.is_some()) {
        return Ok(InheritanceConversion::NoCustomPath);
    }
    for edge in route {
        let Some(mut next) = path(&ctx.ext::<ClassState>().records, &source, target)
            .and_then(|path| path.into_iter().next())
            .filter(|next| next.target == edge.target)
        else {
            return Ok(InheritanceConversion::Rejected);
        };
        if let Some(methods) = next.methods
            && methods.test != SEXP_NIL
        {
            ctx.protect(methods.test);
            ctx.protect(methods.forward);
            let valid = ctx.apply(methods.test, &[(None, value)], SEXP_NIL, env)?;
            if !flag(ctx, valid)? {
                return Ok(InheritanceConversion::Rejected);
            }
            let Some(updated) = path(&ctx.ext::<ClassState>().records, &source, target)
                .and_then(|path| path.into_iter().next())
                .filter(|updated| updated.target == next.target)
            else {
                return Ok(InheritanceConversion::Rejected);
            };
            next = updated;
        }
        let (name, package) = &next.target;
        let Some(info) = cached(ctx, name, Some(package)) else {
            return Ok(InheritanceConversion::Rejected);
        };
        if let Some(methods) = next.methods.filter(|methods| methods.forward != SEXP_NIL) {
            value = apply_conversion(
                ctx,
                methods.forward,
                &[(None, value)],
                name,
                strict || name != target,
                env,
                call,
            )?;
        } else if strict || name != target {
            let source_info = cached(ctx, &source.0, Some(&source.1));
            value = project(ctx, value, &info, source_info.as_ref())?;
        }
        ctx.protect(value);
        source = next.target;
    }
    Ok(InheritanceConversion::Converted(value))
}

pub(super) fn convert(
    ctx: &mut Ctx,
    object: SexpId,
    target: &str,
    strict: bool,
    extension: SexpId,
    env: SexpId,
    call: &Call,
) -> EvalResult {
    ctx.scoped(|ctx| {
        ctx.protect(object);
        let source = runtime_class(ctx, object)?;
        if source == target || target == "ANY" {
            return Ok(object);
        }
        let source_info = object_info(ctx, object)?.or_else(|| cached(ctx, &source, None));
        if let Some((defined, methods)) = conversion(ctx, &source, target, source_info.as_ref()) {
            let arg = if defined == source {
                object
            } else if let Some(info) = lookup(ctx, &defined, None, env, true)? {
                project(ctx, object, &info, source_info.as_ref())?
            } else {
                object
            };
            return apply_conversion(
                ctx,
                methods.forward,
                &[(None, arg)],
                target,
                strict,
                env,
                call,
            );
        }
        if extension != SEXP_NIL && ctx.session().is_s4(extension)? {
            let fun = decode_slot(attr(ctx, extension, "coerce")?);
            if fun != SEXP_NIL {
                return apply_conversion(ctx, fun, &[(None, object)], target, strict, env, call);
            }
        }
        if let Some(fun) = table_conversion(ctx, &source, target, env, call)? {
            return apply_conversion(ctx, fun, &[(None, object)], target, strict, env, call);
        }
        let outcome = match &source_info {
            Some(info) => {
                inheritance_conversion(ctx, object, key(info), target, strict, env, call)?
            }
            None => InheritanceConversion::NoCustomPath,
        };
        if let InheritanceConversion::Converted(value) = outcome {
            return Ok(value);
        }
        if matches!(outcome, InheritanceConversion::NoCustomPath)
            && matches(ctx, object, target, env)?
            && let Some(info) = lookup(ctx, target, None, env, true)?
        {
            if !strict {
                return Ok(object);
            }
            if let Some(member) = source_info
                .as_ref()
                .and_then(|source| union_member(ctx, &info, source))
            {
                if let Some(member) = lookup(ctx, &member, None, env, true)? {
                    return project(ctx, object, &member, source_info.as_ref());
                }
                return Ok(object);
            }
            if info.is_virtual && info.slots.is_empty() && info.package != "methods" {
                return Ok(object);
            }
            return project(ctx, object, &info, source_info.as_ref());
        }
        if let Some(fun) = conversion_builtin(target) {
            let sym = ctx.session_mut().intern(fun)?;
            if let Some(fun) = ctx.get_var(sym, env)? {
                return ctx.apply(fun, &[(None, object)], call.err_call(), env);
            }
        }
        Err(error(
            ctx,
            call,
            format!("no method or default for coercing “{source}” to “{target}”"),
        ))
    })
}

pub(super) fn as_class(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let (target, _) = class_spec(ctx, args.get(1), call.err_call())?;
    convert(
        ctx,
        args.get(0),
        &target,
        flag(ctx, args.get(2))?,
        args.get(3),
        args.get(4),
        call,
    )
}

pub(super) fn as_set(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    let (target, _) = class_spec(ctx, args.get(1), call.err_call())?;
    let value = args.get(2);
    let env = args.get(3);
    let source = runtime_class(ctx, object)?;
    let source_info = object_info(ctx, object)?;
    let replace = conversion(ctx, &source, &target, source_info.as_ref())
        .map(|(_, methods)| methods)
        .or_else(|| {
            source_info
                .as_ref()
                .and_then(|info| edge_conversion(ctx, info, &target))
        });
    if let Some(methods) = replace
        && methods.reverse != SEXP_NIL
    {
        let value_sym = ctx.session_mut().intern("value")?;
        return apply_conversion(
            ctx,
            methods.reverse,
            &[(None, object), (Some(value_sym), value)],
            &target,
            true,
            env,
            call,
        );
    }
    let value = convert(ctx, value, &target, true, SEXP_NIL, env, call)?;
    ctx.protect(value);
    if source == target {
        return Ok(value);
    }
    if let Some(source_info) = source_info
        && let Some(info) = lookup(ctx, &target, None, env, true)?
        && names_in_chain(&source_info).any(|name| name == target)
    {
        let out = if let Some(data) = data_class(ctx, &info) {
            set_data(ctx, object, value, &data)?
        } else {
            ctx.session_mut().shallow_duplicate(object)?
        };
        for (slot, _) in info.slots.iter().filter(|(name, _)| name != ".Data") {
            let val = slot_value(ctx, value, slot)?;
            write_slot(ctx, out, slot, val)?;
        }
        return Ok(out);
    }
    Err(error(
        ctx,
        call,
        format!("no method or default for as() replacement of “{source}” with Class=\"{target}\""),
    ))
}

use std::collections::HashSet;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi};

use crate::registry::ClassInfo;

use super::definitions::{self, data_class, lookup, methods_env, object_info, resolve};
use super::{relations, *};

pub(super) fn slots(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    let env = args.get(1);
    let only_names = flag(ctx, args.get(2))?;
    let def = if ctx.session().type_of(value)? == SexpType::Str
        || attr(ctx, value, "className")? != SEXP_NIL && attr(ctx, value, "slots")? != SEXP_NIL
    {
        resolve(ctx, value, env, call.err_call())?
    } else if only_names {
        object_info(ctx, value)?
    } else {
        return Err(error(
            ctx,
            call,
            "class should be either a character-string name or a class definition",
        ));
    };
    let Some(def) = def else {
        if !only_names {
            let class = name(ctx, value, call.err_call())?;
            return Err(bad_class(ctx, call.err_call(), &class));
        }
        return strings(ctx, []);
    };
    if only_names {
        return strings(ctx, def.slots.iter().map(|(name, _)| name.as_str()));
    }
    if def.slots.is_empty() {
        return Ok(SEXP_NIL);
    }
    let types = strings(ctx, def.slots.iter().map(|(_, class)| class.as_str()))?;
    let names = strings(ctx, def.slots.iter().map(|(name, _)| name.as_str()))?;
    set_attr(ctx, types, "names", names)?;
    Ok(types)
}

pub(super) fn data_copy(ctx: &mut Ctx, source: SexpId, class: &str) -> EvalResult {
    let copy = ctx.session_mut().shallow_duplicate(source)?;
    if copy == source {
        return Ok(copy);
    }
    let dim = attr(ctx, source, "dim")?;
    let dimnames = attr(ctx, source, "dimnames")?;
    ctx.session_mut().clear_attrs(copy)?;
    ctx.session_mut().set_s4_flag(copy, false)?;
    if matches!(class, "matrix" | "array" | "structure") {
        set_attr(ctx, copy, "dim", dim)?;
        set_attr(ctx, copy, "dimnames", dimnames)?;
    }
    Ok(copy)
}

pub(super) fn slot_value(ctx: &mut Ctx, obj: SexpId, slot: &str) -> EvalResult {
    if slot == ".Data" {
        if let Some(info) = object_info(ctx, obj)?
            && let Some(class) = data_class(ctx, &info)?
        {
            return data_copy(ctx, obj, &class);
        }
        let xdata = attr(ctx, obj, ".xData")?;
        if xdata != SEXP_NIL {
            return Ok(decode_slot(xdata));
        }
        if ctx.session().type_of(obj)? != SexpType::Obj {
            let class = relations::runtime_class(ctx, obj)?;
            return data_copy(ctx, obj, &class);
        }
        return Ok(SEXP_NIL);
    }
    Ok(decode_slot(attr(ctx, obj, slot)?))
}

fn slot_exists(ctx: &mut Ctx, obj: SexpId, slot: &str) -> EvalResult<bool> {
    if slot == ".Data" {
        return Ok(
            ctx.session().type_of(obj)? != SexpType::Obj || attr(ctx, obj, ".xData")? != SEXP_NIL
        );
    }
    Ok(attr(ctx, obj, slot)? != SEXP_NIL)
}

pub(super) fn has_slot(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = name(ctx, args.get(1), call.err_call())?;
    let has = slot_exists(ctx, args.get(0), &name)?;
    logical(ctx, has)
}

pub(super) fn slot(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let obj = args.get(0);
    let name = name(ctx, args.get(1), call.err_call())?;
    if name == ".Data" {
        return slot_value(ctx, obj, &name);
    }
    if !ctx.session().is_s4(obj)? && attr(ctx, obj, "class")? == SEXP_NIL {
        let typ = ctx.session().type_of(obj)?.type_name(false);
        return Err(error(
            ctx,
            call,
            format!("cannot get a slot (\"{name}\") from an object of type \"{typ}\""),
        ));
    }
    if slot_exists(ctx, obj, &name)? {
        return slot_value(ctx, obj, &name);
    }
    let class = relations::runtime_class(ctx, obj)?;
    Err(error(
        ctx,
        call,
        format!("no slot of name \"{name}\" for this object of class \"{class}\""),
    ))
}

pub(super) fn check_assignment(
    ctx: &mut Ctx,
    obj: SexpId,
    slot: &str,
    value: SexpId,
) -> EvalResult {
    let class = relations::runtime_class(ctx, obj)?;
    let info = object_info(ctx, obj)?.or_else(|| definitions::cached(ctx, &class, None));
    let Some(info) = info else {
        return Err(bad_class(ctx, SEXP_NIL, &class));
    };
    let Some((_, wanted)) = info.slots.iter().find(|(name, _)| name == slot) else {
        return Err(ctx.stop(
            SEXP_NIL,
            format!("‘{slot}’ is not a slot in class “{class}”"),
        ));
    };
    if relations::matches(ctx, value, wanted, GLOBAL_ENV)? {
        return Ok(value);
    }
    let source = relations::runtime_class(ctx, value)?;
    Err(ctx.stop(SEXP_NIL, format!(
        "assignment of an object of class “{source}” is not valid for @‘{slot}’ in an object of class “{class}”; is(value, \"{wanted}\") is not TRUE"
    )))
}

pub(super) fn set_data(ctx: &mut Ctx, obj: SexpId, value: SexpId, data_class: &str) -> EvalResult {
    let result = ctx.session_mut().shallow_duplicate(value)?;
    if result == value {
        if ctx.session().type_of(value)? == SexpType::Env {
            let obj = ctx.session_mut().shallow_duplicate(obj)?;
            write_slot(ctx, obj, ".xData", value)?;
            return Ok(obj);
        }
        return Ok(value);
    }
    let source = ctx.session().attrs(value)?;
    let original = ctx.session().attrs(obj)?;
    ctx.session_mut().clear_attrs(result)?;
    let s4 = ctx.session().is_s4(obj)?;
    ctx.session_mut().set_s4_flag(result, s4)?;
    let dimensioned = matches!(data_class, "matrix" | "array" | "structure");
    for (key, old) in original {
        let key_name = ctx.session().sym_name(key)?;
        let supplied = ctx.session().attr(value, key);
        let updated = match key_name {
            "dim" | "dimnames" if !dimensioned => SEXP_NIL,
            "dim" | "dimnames" | "names" if supplied != SEXP_NIL => supplied,
            _ => old,
        };
        ctx.session_mut().set_attrib(result, key, updated)?;
    }
    for (key, val) in source {
        let key_name = ctx.session().sym_name(key)?;
        if key_name == "class"
            || !dimensioned && matches!(key_name, "dim" | "dimnames")
            || ctx.session().attr(result, key) != SEXP_NIL
        {
            continue;
        }
        ctx.session_mut().set_attrib(result, key, val)?;
    }
    Ok(result)
}

pub(super) fn slot_set(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    let name = name(ctx, args.get(1), call.err_call())?;
    let mut value = args.get(3);
    match ctx.session().type_of(object)? {
        SexpType::Nil => return Err(error(ctx, call, "attempt to set slot on NULL object")),
        SexpType::Sym => return Err(error(ctx, call, "cannot set an attribute on a 'symbol'")),
        _ => {}
    }
    if flag(ctx, args.get(2))? {
        value = check_assignment(ctx, object, &name, value)?;
    }
    if name == ".Data"
        && let Some(info) = object_info(ctx, object)?
        && let Some(data) = data_class(ctx, &info)?
    {
        return set_data(ctx, object, value, &data);
    }
    let copy = ctx.session_mut().unshare(object)?;
    write_slot(ctx, copy, &name, value)?;
    Ok(copy)
}

pub(crate) fn stamp_instance(ctx: &mut Ctx, prototype: SexpId, info: &ClassInfo) -> EvalResult {
    if prototype == SEXP_NIL {
        return Ok(SEXP_NIL);
    }
    if ctx.session().type_of(prototype)? == SexpType::Env {
        let parent = ctx.session().env_enclos(prototype)?;
        return Ok(ctx.session_mut().alloc_env(parent)?);
    }
    let obj = ctx.session_mut().shallow_duplicate(prototype)?;
    if ctx.session().is_s4(prototype)? && obj != prototype {
        let xdata = attr(ctx, obj, ".xData")?;
        if ctx.session().type_of(xdata)? == SexpType::Env {
            let parent = ctx.session().env_enclos(xdata)?;
            let env = ctx.session_mut().alloc_env(parent)?;
            write_slot(ctx, obj, ".xData", env)?;
        }
        let class = qualified(ctx, &info.name, &info.package)?;
        set_attr(ctx, obj, "class", class)?;
    }
    Ok(obj)
}

pub(super) fn new(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let class = args.get(0);
    let env = args.get(1);
    let Some(info) = resolve(ctx, class, env, call.err_call())? else {
        let (name, _) = definitions::class_spec(ctx, class, call.err_call())?;
        return Err(bad_class(ctx, call.err_call(), &name));
    };
    if info.is_virtual {
        return Err(error(
            ctx,
            call,
            format!(
                "trying to generate an object from a virtual class (\"{}\")",
                info.name
            ),
        ));
    }
    let prototype = decode_slot(attr(ctx, info.class_def, "prototype")?);
    stamp_instance(ctx, prototype, &info)
}

pub(super) fn initializer(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let env = args.get(0);
    let sym = ctx.session_mut().intern("initialize")?;
    let fallback = methods_env(ctx);
    match ctx.get_var(sym, env)? {
        Some(fun) if callable(ctx, fun)? => Ok(fun),
        _ => ctx.find_fun(sym, fallback),
    }
}

fn inherit_values(
    ctx: &mut Ctx,
    mut object: SexpId,
    info: &ClassInfo,
    value: SexpId,
    env: SexpId,
    call: &Call,
) -> EvalResult {
    if let Some(data) = data_class(ctx, info)?
        && relations::matches(ctx, value, &data, env)?
    {
        object = set_data(ctx, object, value, &data)?;
        ctx.protect(object);
        if !ctx.session().is_s4(value)? {
            return Ok(object);
        }
    }
    let class = relations::runtime_class(ctx, value)?;
    if let Some((_, expected)) = info.slots.iter().find(|(slot, _)| slot == ".xData")
        && relations::matches(ctx, value, expected, env)?
    {
        write_slot(ctx, object, ".xData", value)?;
        return Ok(object);
    }
    let parent = object_info(ctx, value)?.or_else(|| definitions::cached(ctx, &class, None));
    let Some(parent) = parent.filter(|p| names_in_chain(info).any(|n| n == p.name)) else {
        return Err(error(
            ctx,
            call,
            format!(
                "cannot use object of class “{class}” in new():  class “{}” does not extend that class",
                info.name
            ),
        ));
    };
    for (slot, _) in &parent.slots {
        if slot != ".Data" && slot_exists(ctx, value, slot)? {
            let v = slot_value(ctx, value, slot)?;
            write_slot(ctx, object, slot, v)?;
        }
    }
    Ok(object)
}

pub(super) fn initialize(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    ctx.scoped(|ctx| {
        let obj = args.get(0);
        let env = args.get(2);
        let values = named_values(ctx, args.get(1))?;
        if values.is_empty() {
            return Ok(obj);
        }
        let Some(info) = object_info(ctx, obj)? else {
            let class = relations::runtime_class(ctx, obj)?;
            if let [(name, first)] = values.as_slice()
                && name.is_empty()
            {
                if relations::matches(ctx, *first, &class, env)? {
                    return data_copy(ctx, *first, &class);
                }
                return relations::convert(ctx, *first, &class, true, SEXP_NIL, env, call);
            }
            return Err(error(
                ctx,
                call,
                format!("unnamed argument must extend class “{class}”"),
            ));
        };
        let mut object = ctx.session_mut().shallow_duplicate(obj)?;
        ctx.protect(object);
        // Unnamed superclass values supply defaults for named slot arguments.
        for (_, value) in values.iter().filter(|(name, _)| name.is_empty()) {
            object = inherit_values(ctx, object, &info, *value, env, call)?;
        }
        for (name, value) in values.iter().filter(|(name, _)| !name.is_empty()) {
            if !info.slots.iter().any(|(slot, _)| name == slot) {
                return Err(error(
                    ctx,
                    call,
                    format!("invalid name for slot of class “{}”: {name}", info.name),
                ));
            }
            if name == ".Data"
                && let Some(data) = data_class(ctx, &info)?
            {
                let value = relations::convert(ctx, *value, &data, true, SEXP_NIL, env, call)?;
                ctx.protect(value);
                object = set_data(ctx, object, value, &data)?;
                ctx.protect(object);
            } else {
                write_slot(ctx, object, name, *value)?;
            }
        }
        validate(ctx, object, false, false, env, call)?;
        Ok(object)
    })
}

pub(super) fn set_validity(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = args.get(2);
    let Some(info) = resolve(ctx, args.get(0), env, call.err_call())? else {
        let (name, _) = definitions::class_spec(ctx, args.get(0), call.err_call())?;
        return Err(bad_class(ctx, call.err_call(), &name));
    };
    let method = args.get(1);
    if method != SEXP_NIL && !callable(ctx, method)? {
        let class = relations::runtime_class(ctx, method)?;
        return Err(error(
            ctx,
            call,
            format!(
                "a validity method must be a function of one argument, got an object of class “{class}”"
            ),
        ));
    }
    if method != SEXP_NIL && ctx.session().type_of(method)? == SexpType::Clo {
        let (formals, _, _) = ctx.session().closure_parts(method)?;
        let argc = ctx.session().pairlist_iter(formals).count();
        if argc != 1 {
            return Err(error(
                ctx,
                call,
                format!(
                    "trying to change the argument list of for validity method for class '{}' with {argc} arguments to have arguments (object)",
                    info.name
                ),
            ));
        }
        if let Some((Some(tag), _)) = ctx.session().pairlist_iter(formals).next() {
            let formal = ctx.session().sym_name(tag)?.to_owned();
            if formal != "object" {
                ctx.message(call.err_call(), format!(
                    "NOTE: arguments in definition for validity method for class '{}' changed from ({formal}) to (object)\n",
                    info.name
                ))?;
            }
        }
    }
    definitions::update_definition(ctx, &info, "validity", method)
}

struct Validator<'a> {
    call: &'a Call,
    env: SexpId,
    complete: bool,
    active: HashSet<SexpId>,
}

impl Validator<'_> {
    fn check(
        &mut self,
        ctx: &mut Ctx,
        object: SexpId,
        info: &ClassInfo,
    ) -> EvalResult<Vec<String>> {
        if self.active.len() >= ctx.opts().expressions.min(128) {
            return Err(error(ctx, self.call, "evaluation nested too deeply"));
        }
        if !self.active.insert(object) {
            return Ok(Vec::new());
        }
        let result = self.check_active(ctx, object, info);
        self.active.remove(&object);
        result
    }

    fn check_active(
        &mut self,
        ctx: &mut Ctx,
        object: SexpId,
        info: &ClassInfo,
    ) -> EvalResult<Vec<String>> {
        let mut errors = Vec::new();
        for (name, typ) in &info.slots {
            if !slot_exists(ctx, object, name)? {
                errors.push(format!(
                    "slots in class definition but not in object: \"{name}\""
                ));
                continue;
            }
            let value = slot_value(ctx, object, name)?;
            ctx.protect(value);
            if !relations::matches(ctx, value, typ, self.env)? {
                let class = relations::runtime_class(ctx, value)?;
                errors.push(format!("invalid object for slot \"{name}\" in class \"{}\": got class \"{class}\", should be or extend class \"{typ}\"", info.name));
            } else if self.complete
                && ctx.session().is_s4(value)?
                && let Some(nested) = object_info(ctx, value)?
            {
                let nested_errors = self.check(ctx, value, &nested)?;
                errors.extend(
                    nested_errors
                        .into_iter()
                        .map(|e| format!("In slot \"{name}\" of class \"{}\": {e}", nested.name)),
                );
            }
        }
        if !errors.is_empty() {
            return Ok(errors);
        }
        for class in info
            .superclasses
            .iter()
            .map(|(n, _, _)| n.as_str())
            .chain(std::iter::once(info.name.as_str()))
        {
            let Some(target) = lookup(ctx, class, None, self.env, true)? else {
                continue;
            };
            let method = decode_slot(attr(ctx, target.class_def, "validity")?);
            if method == SEXP_NIL {
                continue;
            }
            let arg = if class == info.name {
                object
            } else {
                relations::convert(ctx, object, class, true, SEXP_NIL, self.env, self.call)?
            };
            let result = ctx.apply(method, &[(None, arg)], self.call.err_call(), self.env)?;
            if result == SEXP_NIL
                || ctx.session().type_of(result)? == SexpType::Lgl
                    && ctx.session().xlength(result)? == 1
                    && flag(ctx, result)?
            {
                continue;
            }
            let text = ctx.session_mut().coerce(result, SexpType::Str)?.0;
            errors.extend(text_vec(ctx, text)?);
        }
        Ok(errors)
    }
}

fn validate(
    ctx: &mut Ctx,
    obj: SexpId,
    test: bool,
    complete: bool,
    env: SexpId,
    call: &Call,
) -> EvalResult {
    ctx.scoped(|ctx| {
        let Some(info) = object_info(ctx, obj)? else {
            return logical(ctx, true);
        };
        let mut validator = Validator {
            call,
            env,
            complete,
            active: HashSet::new(),
        };
        let errors = validator.check(ctx, obj, &info)?;
        if errors.is_empty() {
            return logical(ctx, true);
        }
        if test {
            return strings(ctx, errors.iter().map(String::as_str));
        }
        let prefix = format!("invalid class “{}” object: ", info.name);
        let message = if let [message] = errors.as_slice() {
            format!("{prefix}{message}")
        } else {
            errors
                .iter()
                .enumerate()
                .map(|(i, error)| format!("{prefix}{}: {error}", i + 1))
                .collect::<Vec<_>>()
                .join("\n")
        };
        Err(error(ctx, call, message))
    })
}

pub(super) fn valid(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    validate(
        ctx,
        args.get(0),
        flag(ctx, args.get(1))?,
        flag(ctx, args.get(2))?,
        args.get(3),
        call,
    )
}

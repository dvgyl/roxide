use std::collections::HashMap;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use super::{ClassState, key, lookup};
use crate::class::{
    attr, decode_slot, error, flag, logical, name, named_values, objects, qualified, relations,
    strings, text_vec, write_slot,
};
use crate::registry::ClassInfo;

pub(in crate::class) fn prototype(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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

pub(super) struct Spec<'a> {
    pub(super) class: &'a str,
    pub(super) slots: &'a [(String, String)],
    pub(super) data: Option<&'a str>,
    pub(super) ancestors: &'a [ClassInfo],
    pub(super) env: SexpId,
}

pub(super) fn build(ctx: &mut Ctx, call: &Call, given: SexpId, spec: Spec<'_>) -> EvalResult {
    let Spec {
        class,
        slots,
        data,
        ancestors,
        env,
    } = spec;
    let mut defaults = HashMap::new();
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

    let undefined: Vec<_> = defaults
        .keys()
        .filter(|name| !slots.iter().any(|(slot, _)| slot == *name))
        .collect();
    if !undefined.is_empty() {
        let mut names = undefined
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>();
        names.sort();
        return Err(error(
            ctx,
            call,
            format!(
                "The prototype for class “{class}” has undefined slot(s): {}",
                names.join(", ")
            ),
        ));
    }
    let prototype = if let Some(data) = data {
        let source = if explicit_data != SEXP_NIL {
            if !relations::matches(ctx, explicit_data, data, env)? {
                return Err(error(
                    ctx,
                    call,
                    format!("prototype data does not extend class “{data}”"),
                ));
            }
            explicit_data
        } else if let Some(parent) = ancestors.iter().find(|p| {
            ctx.ext::<ClassState>()
                .records
                .get(&key(p))
                .and_then(|record| record.data.as_deref())
                == Some(data)
        }) {
            decode_slot(attr(ctx, parent.class_def, "prototype")?)
        } else {
            default_value(ctx, data, env)?
        };
        if matches!(
            ctx.session().type_of(source)?,
            SexpType::Nil | SexpType::Sym | SexpType::Env | SexpType::ExtPtr
        ) {
            return Err(error(
                ctx,
                call,
                format!(
                    "in constructing the prototype for class “{class}”: prototype has class “S4”, but the data part specifies class “{data}”"
                ),
            ));
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
                return Err(error(
                    ctx,
                    call,
                    format!(
                        "in making the prototype for class “{class}” elements of the prototype failed to match the corresponding slot class: {slot} (class \"{typ}\" )"
                    ),
                ));
            }
            *val
        } else {
            let mut inherited = SEXP_NIL;
            for parent in ancestors {
                let source = decode_slot(attr(ctx, parent.class_def, "prototype")?);
                inherited = attr(ctx, source, slot)?;
                if inherited != SEXP_NIL
                    && relations::matches(ctx, decode_slot(inherited), typ, env)?
                {
                    break;
                }
                inherited = SEXP_NIL;
            }
            if inherited == SEXP_NIL {
                if lookup(ctx, typ, None, env, true)?.is_none() && typ != class {
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
        ctx.warn(
            call.err_call(),
            format!(
                "undefined slot classes in definition of \"{class}\": {}",
                missing.join(", ")
            ),
        );
    }
    Ok(prototype)
}

pub(super) fn default_value(ctx: &mut Ctx, class: &str, env: SexpId) -> EvalResult {
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

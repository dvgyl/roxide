//! S4 class definitions, construction, slots, validity, and coercion.

pub(crate) mod definitions;
pub(crate) mod objects;
pub(crate) mod relations;

use roxide_eval::{
    ArgEval, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, FALSE_SCALAR, RLogical, SEXP_NIL, SEXP_S4_NULL, SexpType, Sym, TRUE_SCALAR,
    ValueApi,
};

use crate::registry::ClassInfo;

macro_rules! prim {
    ($name:literal, $fun:path, $arity:expr) => {
        PrimDef {
            name: $name,
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact($arity),
            dispatch: Dispatch::None,
            visibility: Visibility::On,
            fun: $fun,
        }
    };
}

/// Class functions registered in the methods namespace.
pub const MODULE: Module = Module {
    prims: &[
        prim!(".s4_bootstrap", definitions::bootstrap, 1),
        prim!(".s4_package", definitions::package, 1),
        prim!(".s4_define", definitions::define, 10),
        prim!(".s4_get", definitions::get, 5),
        prim!(".s4_remove", definitions::remove, 2),
        prim!(".s4_generator", definitions::generator, 2),
        prim!(".s4_prototype", definitions::prototype, 1),
        prim!(".s4_slots", objects::slots, 3),
        prim!(".hasSlot", objects::has_slot, 2),
        prim!(".s4_slot", objects::slot, 2),
        prim!(".s4_slot_set", objects::slot_set, 4),
        prim!(".s4_new", objects::new, 2),
        prim!(".s4_initializer", objects::initializer, 1),
        prim!(".s4_initialize", objects::initialize, 3),
        prim!(".s4_validity_set", objects::set_validity, 3),
        prim!(".s4_valid", objects::valid, 4),
        prim!(".s4_union", relations::union, 3),
        prim!(".s4_old", relations::old_class, 4),
        prim!(".s4_set_is", relations::set_is, 6),
        prim!(".s4_is", relations::is, 3),
        prim!(".s4_extends", relations::extends, 5),
        prim!(".s4_as", relations::as_class, 5),
        prim!(".s4_as_set", relations::as_set, 4),
        prim!(".s4_set_as", relations::set_as, 5),
    ],
    prelude: include_str!("prelude.R"),
};

/// Return the S4 superclass chain for S3 dispatch.
pub(crate) fn class_chain(ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<SexpId>> {
    if !ctx.session().is_s4(value)? {
        return Ok(None);
    }
    let Some(info) = definitions::object_info(ctx, value)? else {
        return Ok(None);
    };
    Ok(Some(strings(ctx, names_in_chain(&info))?))
}

/// Validate a value before assignment by the evaluator's `@<-` primitive.
pub(crate) fn check_slot(
    ctx: &mut Ctx,
    obj: SexpId,
    name: Sym,
    value: SexpId,
) -> EvalResult<SexpId> {
    let name = ctx.session().sym_name(name)?.to_owned();
    objects::check_assignment(ctx, obj, &name, value)
}

fn names_in_chain(info: &ClassInfo) -> impl Iterator<Item = &str> {
    std::iter::once(info.name.as_str()).chain(info.superclasses.iter().map(|(n, _, _)| n.as_str()))
}

fn strings<'a>(ctx: &mut Ctx, values: impl IntoIterator<Item = &'a str>) -> EvalResult {
    let chars = values
        .into_iter()
        .map(|s| ctx.session_mut().mk_char(s.as_bytes(), Encoding::Utf8))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ctx.session_mut().alloc_str_from(&chars)?)
}

fn char_text(ctx: &Ctx, value: CharId) -> EvalResult<String> {
    Ok(String::from_utf8_lossy(ctx.session().char_bytes(value)?).into_owned())
}

fn text_vec(ctx: &Ctx, value: SexpId) -> EvalResult<Vec<String>> {
    ctx.session()
        .str_slice(value)?
        .iter()
        .map(|&s| char_text(ctx, s))
        .collect()
}

fn name(ctx: &mut Ctx, value: SexpId, call: SexpId) -> EvalResult<String> {
    if ctx.session().type_of(value)? == SexpType::Sym {
        return Ok(ctx.session().sym_name_of(value)?.to_owned());
    }
    if ctx.session().type_of(value)? == SexpType::Str
        && let Some(&first) = ctx.session().str_slice(value)?.first()
    {
        return char_text(ctx, first);
    }
    Err(ctx.stop(call, "expected a character string"))
}

fn attr(ctx: &mut Ctx, obj: SexpId, key: &str) -> EvalResult {
    let sym = ctx.session_mut().intern(key)?;
    Ok(ctx.session().attr(obj, sym))
}

fn set_attr(ctx: &mut Ctx, obj: SexpId, key: &str, value: SexpId) -> EvalResult<()> {
    let sym = ctx.session_mut().intern(key)?;
    Ok(ctx.session_mut().set_attrib(obj, sym, value)?)
}

fn decode_slot(value: SexpId) -> SexpId {
    if value == SEXP_S4_NULL {
        SEXP_NIL
    } else {
        value
    }
}

fn write_slot(ctx: &mut Ctx, obj: SexpId, key: &str, value: SexpId) -> EvalResult<()> {
    let stored = if value == SEXP_NIL {
        SEXP_S4_NULL
    } else {
        value
    };
    set_attr(ctx, obj, key, stored)
}

fn flag(ctx: &Ctx, value: SexpId) -> EvalResult<bool> {
    Ok(ctx.session().lgl(value)?.iter().next() == Some(RLogical::TRUE))
}

fn logical(_ctx: &mut Ctx, value: bool) -> EvalResult {
    Ok(if value { TRUE_SCALAR } else { FALSE_SCALAR })
}

fn qualified(ctx: &mut Ctx, class: &str, package: &str) -> EvalResult {
    let value = ctx.session_mut().scalar_str(class)?;
    let package = ctx.session_mut().scalar_str(package)?;
    set_attr(ctx, value, "package", package)?;
    Ok(value)
}

fn named_values(ctx: &mut Ctx, list: SexpId) -> EvalResult<Vec<(String, SexpId)>> {
    if list == SEXP_NIL {
        return Ok(Vec::new());
    }
    let names = attr(ctx, list, "names")?;
    let names = if names == SEXP_NIL {
        Vec::new()
    } else {
        text_vec(ctx, names)?
    };
    Ok(ctx
        .session()
        .list_slice(list)?
        .iter()
        .copied()
        .enumerate()
        .map(|(i, v)| (names.get(i).cloned().unwrap_or_default(), v))
        .collect())
}

fn make_list(ctx: &mut Ctx, entries: &[(String, SexpId)], named: bool) -> EvalResult {
    let list = ctx.session_mut().alloc_list(entries.len())?;
    for (i, &(_, value)) in entries.iter().enumerate() {
        ctx.session_mut().list_set(list, i, value)?;
    }
    if named {
        let names = strings(ctx, entries.iter().map(|(n, _)| n.as_str()))?;
        set_attr(ctx, list, "names", names)?;
    }
    Ok(list)
}

fn bad_class(ctx: &mut Ctx, call: SexpId, name: &str) -> roxide_eval::Unwind {
    ctx.stop(call, format!("“{name}” is not a defined class"))
}

fn callable(ctx: &Ctx, val: SexpId) -> EvalResult<bool> {
    Ok(matches!(
        ctx.session().type_of(val)?,
        SexpType::Clo | SexpType::Builtin | SexpType::Special
    ))
}

fn error(ctx: &mut Ctx, call: &Call, message: impl Into<String>) -> roxide_eval::Unwind {
    ctx.stop(call.err_call(), message)
}

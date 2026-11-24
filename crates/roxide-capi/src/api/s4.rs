//! S4 object allocation, slots, and class lookup.

use std::ffi::{CStr, c_char, c_int};

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, SEXP_NIL, SEXP_S4_NULL, SYM_CLASS, SexpType, Sym, ValueApi};

use super::eval::current_env;
use super::string::{c_text, utf8};
use super::sym::install;
use super::vector::{allocating, api, id};
use crate::abi::Sexp;

fn slot_name(ctx: &mut Ctx, name: Sexp, api_name: &str) -> EvalResult<Sym> {
    let name = id(ctx, name, api_name)?;
    if ctx.session().type_of(name)? == SexpType::Sym {
        return Ok(ctx.session().sexp_as_sym(name)?);
    }
    if ctx.session().type_of(name)? != SexpType::Str || ctx.session().xlength(name)? == 0 {
        return Err(ctx.stop(SEXP_NIL, "invalid slot name"));
    }
    let ch = ctx.session_mut().str_get(name, 0)?;
    if ch == CharId::NA {
        return Err(ctx.stop(SEXP_NIL, "invalid slot name"));
    }
    let session = ctx.session();
    let bytes = utf8(session.char_bytes(ch)?, session.char_encoding(ch)?).into_owned();
    let sym = install(ctx, &bytes)?;
    Ok(ctx.session().sexp_as_sym(sym)?)
}

fn slot_value(ctx: &mut Ctx, object: SexpId, name: Sym) -> EvalResult<SexpId> {
    if ctx.session().sym_name(name)? == ".Data" && ctx.session().is_s4(object)? {
        let env = current_env(ctx);
        return ctx.call_function("@", &[(None, object), (None, name.as_sexp())], env);
    }
    let value = ctx.session().attr(object, name);
    match value {
        SEXP_NIL => Err(ctx.stop(
            SEXP_NIL,
            format!("no slot named '{}'", ctx.session().sym_name(name)?),
        )),
        SEXP_S4_NULL => Ok(SEXP_NIL),
        _ => Ok(value),
    }
}

fn class_definition(ctx: &mut Ctx, name: &[u8], required: bool) -> EvalResult<Sexp> {
    let name = std::str::from_utf8(name).map_err(|_| ctx.stop(SEXP_NIL, "invalid class name"))?;
    ctx.scoped(|ctx| {
        let env = current_env(ctx);
        let methods = ctx.load_namespace("methods", SEXP_NIL)?;
        let class = ctx.session_mut().scalar_str(name)?;
        ctx.protect(class);
        let function = if required { "getClass" } else { "getClassDef" };
        let where_arg = ctx.session_mut().intern("where")?;
        ctx.call_function(function, &[(None, class), (Some(where_arg), env)], methods)
            .map(Sexp::from_id)
    })
}

fn class_names(ctx: &mut Ctx, classes: SexpId, limit: usize) -> EvalResult<Vec<Vec<u8>>> {
    if ctx.session().type_of(classes)? != SexpType::Str {
        return Ok(Vec::new());
    }
    let size = ctx.session().xlength(classes)?.min(limit);
    let mut names = Vec::new();
    for i in 0..size {
        let ch = ctx.session_mut().str_get(classes, i)?;
        if ch != CharId::NA {
            let session = ctx.session();
            names.push(utf8(session.char_bytes(ch)?, session.char_encoding(ch)?).into_owned());
        }
    }
    Ok(names)
}

api! {
    /// Allocate an empty S4 object.
    fn Rf_allocS4Object() -> Sexp = Sexp::NULL; |ctx| {
        allocating(ctx, &[], |ctx| Ok(Sexp::from_id(ctx.session_mut().alloc_s4_object()?)))
    }

    /// Read the value of a named slot.
    fn R_do_slot(x: Sexp, name: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "R_do_slot")?;
        let name = slot_name(ctx, name, "R_do_slot")?;
        allocating(ctx, &[x, name.as_sexp()], |ctx| slot_value(ctx, x, name).map(Sexp::from_id))
    }

    /// Set a slot and return the object.
    fn R_do_slot_assign(x: Sexp, name: Sexp, value: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "R_do_slot_assign")?;
        let value = id(ctx, value, "R_do_slot_assign")?;
        let name = slot_name(ctx, name, "R_do_slot_assign")?;
        if ctx.session().sym_name(name)? == ".Data" {
            let env = current_env(ctx);
            return ctx.call_function("@<-", &[(None, x), (None, name.as_sexp()), (None, value)], env)
                .map(Sexp::from_id);
        }
        let stored = if value == SEXP_NIL { SEXP_S4_NULL } else { value };
        ctx.session_mut().set_attrib(x, name, stored)?;
        Ok(Sexp::from_id(x))
    }

    /// Test whether an object has the named slot.
    fn R_has_slot(x: Sexp, name: Sexp) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "R_has_slot")?;
        let name = slot_name(ctx, name, "R_has_slot")?;
        let present = ctx.session().attr(x, name) != SEXP_NIL;
        if !present && ctx.session().sym_name(name)? == ".Data" {
            let xdata = ctx.session_mut().intern(".xData")?;
            return Ok(c_int::from(ctx.session().is_s4(x)?
                && (ctx.session().type_of(x)? != SexpType::Obj || ctx.session().attr(x, xdata) != SEXP_NIL)));
        }
        Ok(c_int::from(present))
    }

    /// Find a class definition by name.
    fn R_getClassDef(name: *const c_char) -> Sexp = Sexp::NULL; |ctx| {
        let name = c_text(ctx, name, "R_getClassDef")?;
        class_definition(ctx, name, false)
    }

    /// Get a class definition or signal an R error.
    fn R_do_MAKE_CLASS(name: *const c_char) -> Sexp = Sexp::NULL; |ctx| {
        let name = c_text(ctx, name, "R_do_MAKE_CLASS")?;
        class_definition(ctx, name, true)
    }

    /// Instantiate an S4 class from its definition.
    fn R_do_new_object(definition: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let definition = id(ctx, definition, "R_do_new_object")?;
        allocating(ctx, &[definition], |ctx| {
            let env = current_env(ctx);
            let methods = ctx.load_namespace("methods", SEXP_NIL)?;
            ctx.call_function(".s4_new", &[(None, definition), (None, env)], methods)
                .map(Sexp::from_id)
        })
    }

    /// Find the index of an allowed class or return -1.
    fn R_check_class_etc(x: Sexp, classes: *mut *const c_char) -> c_int = -1; |ctx| {
        let x = id(ctx, x, "R_check_class_etc")?;
        if classes.is_null() {
            return Ok(-1);
        }
        allocating(ctx, &[x], |ctx| {
            let direct = class_names(ctx, ctx.session().attr(x, SYM_CLASS), 1)?;
            let chain = match ctx.s4_class_chain(x)? {
                Some(chain) => class_names(ctx, chain, usize::MAX)?,
                None => Vec::new(),
            };
            let mut inherited = None;
            for index in 0..c_int::MAX {
                // SAFETY: The caller supplies a readable array with an empty string or NULL at the end.
                let candidate = classes.add(index as usize).read();
                if candidate.is_null() {
                    break;
                }
                // SAFETY: Each non-null entry is a NUL-terminated C string.
                let candidate = CStr::from_ptr(candidate).to_bytes();
                if candidate.is_empty() {
                    break;
                }
                if direct.iter().any(|name| name == candidate) {
                    return Ok(index);
                }
                if inherited.is_none() && chain.iter().any(|name| name == candidate) {
                    inherited = Some(index);
                }
            }
            Ok(inherited.unwrap_or(-1))
        })
    }
}

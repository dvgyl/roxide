//! S4 display through the methods package.

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{BASE_ENV, SEXP_NIL, SEXP_S4_NULL, SexpType, ValueApi, encode_string};

use crate::registry;

/// The S4 show generic and its default method.
pub const MODULE: Module = Module {
    prims: &[PrimDef {
        name: ".rx_showDefault",
        kind: PrimKind::Primitive,
        args: ArgEval::Eval,
        arity: Arity::Exact(2),
        dispatch: Dispatch::None,
        visibility: Visibility::Off,
        fun: show_default,
    }],
    prelude: r#"
showDefault <- function(object, oldMethods = TRUE) .rx_showDefault(object, oldMethods)
setGeneric("show", function(object) standardGeneric("show"),
           where = environment(), package = "methods", useAsDefault = showDefault)
"#,
};

/// Print an S4 value through the registered show method.
pub(crate) fn show_object(ctx: &mut Ctx, value: SexpId, env: SexpId) -> EvalResult<()> {
    let show = ctx.session_mut().intern("show")?;
    let ns = ctx.ns_lookup("methods").unwrap_or(env);
    let fun = ctx.find_fun(show, ns)?;
    ctx.apply(fun, &[(None, value)], SEXP_NIL, env)?;
    Ok(())
}

fn show_default(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let value = args.get(0);
    if !ctx.session().is_s4(value)? {
        ctx.call_function("print", &[(None, value)], BASE_ENV)?;
        return Ok(SEXP_NIL);
    }
    let class_sym = ctx.session_mut().intern("class")?;
    let package_sym = ctx.session_mut().intern("package")?;
    let class = ctx.session().attr(value, class_sym);
    let package = ctx.session().attr(class, package_sym);
    let class = first_string(ctx, class)?;
    let package = first_string(ctx, package)?;
    let info = class
        .as_ref()
        .and_then(|class| registry::class_info(ctx, class, package.as_deref()));
    let ty = ctx.session().type_of(value)?;
    if let Some(class) = &class {
        let label = encode_string(class.as_bytes());
        let _ = ctx
            .host()
            .write_out(format!("An object of class {label}\n").as_bytes());
    }
    if ty != SexpType::Obj {
        let data = ctx.session_mut().shallow_duplicate(value)?;
        ctx.protect(data);
        if data == value {
            ctx.print_value_default(value, ctx.opts().digits)?;
        } else {
            if info.is_some() {
                ctx.session_mut().clear_attrs(data)?;
            } else {
                ctx.session_mut().set_attrib(data, class_sym, SEXP_NIL)?;
            }
            ctx.session_mut().set_s4_flag(data, false)?;
            ctx.call_function("print", &[(None, data)], BASE_ENV)?;
        }
    }
    if let Some(info) = info {
        for (slot, _) in info.slots {
            if slot == ".Data" {
                continue;
            }
            let tag = ctx.session_mut().intern(&slot)?;
            let stored = ctx.session().attr(value, tag);
            let stored = if stored == SEXP_S4_NULL {
                SEXP_NIL
            } else {
                stored
            };
            let label = encode_string(slot.as_bytes());
            let _ = ctx.host().write_out(format!("Slot {label}:\n").as_bytes());
            ctx.call_function("print", &[(None, stored)], BASE_ENV)?;
            let _ = ctx.host().write_out(b"\n");
        }
    }
    Ok(SEXP_NIL)
}

fn first_string(ctx: &Ctx, value: SexpId) -> EvalResult<Option<String>> {
    let Some(id) = ctx
        .session()
        .str_slice(value)
        .ok()
        .and_then(|ids| ids.first())
        .copied()
    else {
        return Ok(None);
    };
    Ok(Some(
        String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned(),
    ))
}

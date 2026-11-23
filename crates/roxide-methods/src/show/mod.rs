//! S4 display through the methods package.

use std::fmt::Write as _;

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{
    BASE_ENV, GLOBAL_ENV, SEXP_NIL, SEXP_S4_NULL, SexpType, ValueApi, encode_string,
};

use crate::class::{definitions::definition_info, relations::runtime_class};
use crate::dispatch::metadata::{attr, flag, texts};
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
for (.show_class in c("classRepresentation", "MethodDefinition",
                      "standardGeneric", "nonstandardGenericFunction")) {
    setMethod("show", .show_class, function(object) .rx_showDefault(object, TRUE),
              where = environment())
}
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
    match class.as_deref() {
        Some("classRepresentation") => return show_class_definition(ctx, value),
        Some("MethodDefinition" | "MethodWithNext") => return show_method(ctx, value),
        Some("standardGeneric" | "nonstandardGenericFunction") => {
            return show_generic(ctx, value, class.as_deref().unwrap_or_default());
        }
        _ => {}
    }
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

fn show_class_definition(ctx: &mut Ctx, value: SexpId) -> EvalResult<SexpId> {
    let info = definition_info(ctx, value)?;
    let virtual_ = if info.is_virtual { "Virtual " } else { "" };
    let location = if info.package == ".GlobalEnv" {
        "in"
    } else {
        "package"
    };
    let name = encode_string(info.name.as_bytes());
    let package = encode_string(info.package.as_bytes());
    let mut output = format!("{virtual_}Class {name} [{location} {package}]\n");
    if info.slots.is_empty() {
        let prototype = attr(ctx, value, "prototype")?;
        let prototype = if prototype == SEXP_S4_NULL {
            SEXP_NIL
        } else {
            prototype
        };
        let class = runtime_class(ctx, prototype)?;
        let class = encode_string(class.as_bytes());
        let _ = writeln!(output, "\nNo Slots, prototype of class {class}");
    } else {
        output.push_str("\nSlots:\n");
        let widths = info
            .slots
            .iter()
            .map(|(name, class)| name.chars().count().max(class.chars().count()))
            .collect::<Vec<_>>();
        let blank = " ".repeat(6 + widths.iter().map(|width| width + 1).sum::<usize>());
        let _ = writeln!(output, "{blank}");
        output.push_str("Name: ");
        for ((name, _), width) in info.slots.iter().zip(&widths) {
            let _ = write!(output, " {name:>width$}");
        }
        output.push_str("\nClass:");
        for ((_, class), width) in info.slots.iter().zip(&widths) {
            let _ = write!(output, " {class:>width$}");
        }
        output.push('\n');
    }
    for (slot, heading) in [("contains", "Extends"), ("subclasses", "Known Subclasses")] {
        let entries = attr(ctx, value, slot)?;
        let names = attr(ctx, entries, "names")?;
        let names = texts(ctx, names)?;
        if names.is_empty() {
            continue;
        }
        let entries = ctx.session().list_slice(entries)?.to_vec();
        let mut lines = Vec::new();
        for (name, edge) in names.into_iter().zip(entries) {
            let name = encode_string(name.as_bytes());
            let distance = attr(ctx, edge, "distance")?;
            let distance = ctx.session().real(distance)?.iter().next().unwrap_or(1.0);
            let data_part = attr(ctx, edge, "dataPart")?;
            let simple = attr(ctx, edge, "simple")?;
            let by = attr(ctx, edge, "by")?;
            let by = first_string(ctx, by)?;
            let direct = distance == 1.0 && !flag(ctx, data_part) && flag(ctx, simple);
            let relation = if flag(ctx, data_part) {
                "from data part".to_owned()
            } else if let Some(by) = by {
                let by = encode_string(by.as_bytes());
                format!("by class {by}, distance {distance}")
            } else if distance > 1.0 {
                format!("distance {distance}")
            } else {
                "directly".to_owned()
            };
            let explicit = if flag(ctx, simple) {
                ""
            } else {
                ", with explicit coerce"
            };
            lines.push((direct, name, format!("{relation}{explicit}")));
        }
        let _ = write!(output, "\n{heading}: ");
        if lines.iter().all(|(direct, _, _)| *direct) {
            output.push_str(
                &lines
                    .into_iter()
                    .map(|(_, name, _)| name)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            output.push('\n');
        } else {
            output.push('\n');
            for (_, name, relation) in lines {
                let _ = writeln!(output, "Class {name}, {relation}");
            }
        }
    }
    let _ = ctx.host().write_out(output.as_bytes());
    Ok(SEXP_NIL)
}

fn show_function(ctx: &mut Ctx, function: SexpId) -> EvalResult<SexpId> {
    let (formals, body, env) = ctx.session().closure_parts(function)?;
    let plain = ctx.session_mut().alloc_closure(formals, body, env)?;
    ctx.print_value_default(plain, ctx.opts().digits)?;
    Ok(env)
}

fn show_method(ctx: &mut Ctx, method: SexpId) -> EvalResult<SexpId> {
    let _ = ctx.host().write_out(b"Method Definition:\n\n");
    show_function(ctx, method)?;
    let target = attr(ctx, method, "target")?;
    let defined = attr(ctx, method, "defined")?;
    let names = attr(ctx, target, "names")?;
    let names = texts(ctx, names)?;
    let target = texts(ctx, target)?;
    let defined = texts(ctx, defined)?;
    let mut header = String::from("        ");
    let mut target_line = String::from("target  ");
    let mut defined_line = String::from("defined ");
    for ((name, target), defined) in names.iter().zip(&target).zip(&defined) {
        let target = encode_string(target.as_bytes());
        let defined = encode_string(defined.as_bytes());
        let width = name
            .chars()
            .count()
            .max(target.chars().count())
            .max(defined.chars().count());
        let _ = write!(header, "{name:<width$} ");
        let _ = write!(target_line, "{target:<width$} ");
        let _ = write!(defined_line, "{defined:<width$} ");
    }
    header.pop();
    target_line.pop();
    defined_line.pop();
    let _ = ctx
        .host()
        .write_out(format!("\nSignatures:\n{header}\n{target_line}\n{defined_line}\n").as_bytes());
    Ok(SEXP_NIL)
}

fn show_generic(ctx: &mut Ctx, generic: SexpId, class: &str) -> EvalResult<SexpId> {
    let name = attr(ctx, generic, "generic")?;
    let name = first_string(ctx, name)?.unwrap_or_default();
    let package = attr(ctx, generic, "package")?;
    let package = first_string(ctx, package)?.unwrap_or_default();
    let label = encode_string(name.as_bytes());
    let package = encode_string(package.as_bytes());
    let _ = ctx
        .host()
        .write_out(format!("{class} for {label} defined from package {package}\n\n").as_bytes());
    let env = show_function(ctx, generic)?;
    if env != GLOBAL_ENV {
        ctx.print_value_default(env, ctx.opts().digits)?;
    }
    let signature = attr(ctx, generic, "signature")?;
    let signature = texts(ctx, signature)?.join(", ");
    let _ = ctx.host().write_out(format!(
        "Methods may be defined for arguments: {signature}\nUse  showMethods({name})  for currently available ones.\n"
    ).as_bytes());
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

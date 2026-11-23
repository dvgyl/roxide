//! Environment-backed S4 reference classes.

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility,
};
use std::collections::HashSet;

use roxide_value::{Binding, SEXP_MISSING, SEXP_NIL, SEXP_S4_NULL, SexpType, ValueApi};

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
        PrimDef {
            name: ".ref_install_methods",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(4),
            dispatch: Dispatch::None,
            visibility: Visibility::Off,
            fun: install_methods,
        },
        PrimDef {
            name: ".ref_method_help",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(2),
            dispatch: Dispatch::None,
            visibility: Visibility::Off,
            fun: method_help,
        },
        PrimDef {
            name: ".ref_store_definition",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(3),
            dispatch: Dispatch::None,
            visibility: Visibility::On,
            fun: store_definition,
        },
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

fn install_methods(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let session = ctx.session_mut();
    let (formals, body, namespace) = session.closure_parts(args.get(2))?;
    let object_tag = session.intern("object")?;
    let key_tag = session.intern("key")?;
    for key in session.str_slice(args.get(1))?.to_vec() {
        let name = String::from_utf8_lossy(session.char_bytes(key)?).into_owned();
        let name = session.intern(&name)?;
        let scope = session.alloc_env(namespace)?;
        let key = session.alloc_str_from(&[key])?;
        session.env_define(scope, object_tag, args.get(3))?;
        session.env_define(scope, key_tag, key)?;
        let getter = session.alloc_closure(formals, body, scope)?;
        session.env_define_binding(
            args.get(0),
            name,
            Binding {
                value: getter,
                active: true,
                locked: false,
                missing: 0,
            },
        )?;
    }
    Ok(SEXP_NIL)
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

fn store_definition(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let def = args.get(0);
    let class = args.get(1);
    let slots = args.get(2);
    if ctx.session().type_of(def)? != SexpType::Obj {
        return Err(ctx.stop(call.err_call(), "expected an S4 class definition"));
    }
    let session = ctx.session_mut();
    let names = session.intern("names")?;
    let keys = session.str_slice(session.attr(slots, names))?.to_vec();
    let values = session.list_slice(slots)?.to_vec();
    for (key, value) in keys.into_iter().zip(values) {
        let key = String::from_utf8_lossy(session.char_bytes(key)?).into_owned();
        let key = session.intern(&key)?;
        let stored = if value == SEXP_NIL {
            SEXP_S4_NULL
        } else {
            value
        };
        session.set_attrib(def, key, stored)?;
    }
    let class_attr = session.intern("class")?;
    session.set_attrib(def, class_attr, class)?;
    Ok(def)
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

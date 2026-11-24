use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, EMPTY_ENV, GLOBAL_ENV, RLogical, SEXP_NIL, SEXP_UNBOUND, SexpType,
    ValueApi,
};

use crate::{Args, Call, Ctx, EvalResult};

pub(super) fn namespace(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let package = ctx.name(args.get(0), call.err_call())?;
    let name = ctx.name(args.get(1), call.err_call())?;
    let sym = ctx.session.intern(&name)?;
    let is_triple = ctx.prims[usize::from(call.prim.0)].name == ":::";
    if package == "base" {
        return ctx.get_var(sym, BASE_ENV)?.ok_or_else(|| {
            ctx.stop(
                call.err_call(),
                format!("'{name}' is not an exported object from 'namespace:base'"),
            )
        });
    }
    let ns = ctx.load_namespace(&package, call.err_call())?;
    if is_triple {
        if let Some(binding) = ctx.session.env_get(ns, sym)
            && binding.value != SEXP_UNBOUND
        {
            return ctx.read_binding(binding, ns);
        }
        return Err(ctx.stop(call.err_call(), format!("object '{name}' not found")));
    }
    let exported = if let Some(exports_env) = ctx.ns_exports_env(ns)? {
        if let Some(exp_binding) = ctx.session.env_get(exports_env, sym) {
            let target_sym = if ctx.session.type_of(exp_binding.value)? == SexpType::Str {
                let mapped = ctx.string(exp_binding.value, call.err_call())?;
                ctx.session.intern(&mapped)?
            } else {
                sym
            };
            // Resolve strictly within `ns` and its `imports:<pkg>` frame (never `.GlobalEnv`/`base`).
            if let Some(binding) = ctx.session.env_get(ns, target_sym)
                && binding.value != SEXP_UNBOUND
            {
                Some(ctx.read_binding(binding, ns)?)
            } else {
                let imports = ctx.session.env_enclos(ns)?;
                if imports != SEXP_NIL
                    && imports != BASE_NAMESPACE
                    && imports != BASE_ENV
                    && imports != GLOBAL_ENV
                    && imports != EMPTY_ENV
                    && let Some(binding) = ctx.session.env_get(imports, target_sym)
                    && binding.value != SEXP_UNBOUND
                {
                    Some(ctx.read_binding(binding, imports)?)
                } else {
                    None
                }
            }
        } else {
            None
        }
    } else if !name.starts_with('.') {
        ctx.session
            .env_get(ns, sym)
            .filter(|b| b.value != SEXP_UNBOUND)
            .map(|b| ctx.read_binding(b, ns))
            .transpose()?
    } else {
        None
    };
    exported.ok_or_else(|| {
        ctx.stop(
            call.err_call(),
            format!("'{name}' is not an exported object from 'namespace:{package}'"),
        )
    })
}

pub(super) fn as_namespace(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let arg = args.get(0);
    if ctx.session.type_of(arg)? == SexpType::Env && ctx.is_namespace_env(arg) {
        return Ok(arg);
    }
    let name = ctx.string(arg, call.err_call())?;
    let base_ok = if args.len() >= 2 {
        ctx.truth(args.get(1), call.err_call())?
    } else {
        true
    };
    if name == "base" {
        if !base_ok {
            return Err(ctx.stop(call.err_call(), "namespace 'base' is not allowed here"));
        }
        return Ok(BASE_NAMESPACE);
    }
    ctx.load_namespace(&name, call.err_call())
}

pub(super) fn is_namespace(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let arg = args.get(0);
    let yes = ctx.session.type_of(arg)? == SexpType::Env && ctx.is_namespace_env(arg);
    Ok(ctx.session.scalar_lgl(RLogical::from_option(Some(yes)))?)
}

pub(super) fn loaded_namespaces(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let names = ctx.ns_loaded_names();
    ctx.strings(names.iter().map(String::as_str))
}

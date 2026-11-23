//! Register S3 methods with their generic's namespace.

use roxide_eval::{Args, Call, Ctx, EvalResult, Module, Visibility};
use roxide_gc::SexpId;
use roxide_value::{BASE_ENV, BASE_NAMESPACE, EMPTY_ENV, SEXP_NIL, SexpType, ValueApi};

use crate::loader::{namespace_info, values::*};
use crate::nsfile::NsSpec;

/// Base S3 method registration function.
pub const MODULE: Module = Module {
    prims: &[internal("C_rx_register_s3", 4, register, Visibility::Off)],
    prelude: r#"
registerS3method <- function(genname, class, method, envir = parent.frame())
    .Internal(C_rx_register_s3(genname, class, method, envir))
registerS3method("Ops", "numeric_version", Ops.numeric_version, baseenv())
"#,
};

pub(crate) struct Registration {
    provider: String,
    generic: String,
    class: String,
    method: SexpId,
}

pub(crate) fn resolve(ctx: &mut Ctx, ns: SexpId, spec: &NsSpec) -> EvalResult<Vec<Registration>> {
    let mut registrations = Vec::with_capacity(spec.s3.len());
    let mut resolved = spec.clone();
    for entry in &mut resolved.s3 {
        let name = entry
            .method
            .get_or_insert_with(|| format!("{}.{}", entry.generic, entry.class));
        let Some(method) = local(ctx, ns, name)? else {
            ctx.warn_now(
                SEXP_NIL,
                format!("S3 method ‘{name}’ was declared in NAMESPACE but not found"),
            )?;
            continue;
        };
        ctx.protect(method);
        let provider = match &entry.provider {
            Some(provider) => provider.clone(),
            None => generic_provider(ctx, &entry.generic, ns)?,
        };
        registrations.push(Registration {
            provider,
            generic: entry.generic.clone(),
            class: entry.class.clone(),
            method,
        });
    }
    let matrix = resolved.s3_matrix(ctx)?;
    let info = namespace_info(ctx, ns)?;
    define(ctx, info, "S3methods", matrix)?;
    Ok(registrations)
}

pub(crate) fn install(ctx: &mut Ctx, registrations: Vec<Registration>) -> EvalResult<()> {
    for entry in registrations {
        ctx.ns_register_s3(&entry.provider, &entry.generic, &entry.class, entry.method)?;
    }
    Ok(())
}

fn generic_provider(ctx: &mut Ctx, generic: &str, env: SexpId) -> EvalResult<String> {
    if matches!(
        generic,
        "Ops" | "Math" | "Summary" | "Complex" | "matrixOps"
    ) {
        return Ok("base".to_owned());
    }
    let key = ctx.session_mut().intern(generic)?;
    let fun = ctx.find_fun(key, env)?;
    if ctx.session().type_of(fun)? == SexpType::Clo {
        let (_, _, mut enclosing) = ctx.session().closure_parts(fun)?;
        while enclosing != SEXP_NIL
            && enclosing != EMPTY_ENV
            && enclosing != BASE_ENV
            && enclosing != BASE_NAMESPACE
        {
            if let Some(name) = ctx.namespace_name(enclosing) {
                return Ok(name);
            }
            enclosing = ctx.session().env_enclos(enclosing)?;
        }
    }
    Ok("base".to_owned())
}

fn register(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let generic = string(ctx, args.get(0))?;
    let class = string(ctx, args.get(1))?;
    let env = args.get(3);
    if ctx.session().type_of(env)? != SexpType::Env {
        return Err(ctx.stop(call.err_call(), "invalid 'envir' argument"));
    }
    let mut method = args.get(2);
    if ctx.session().type_of(method)? == SexpType::Str {
        let name = string(ctx, method)?;
        let sym = ctx.session_mut().intern(&name)?;
        method = ctx.find_fun(sym, env)?;
    }
    if !function(ctx, method)? {
        return Err(ctx.stop(call.err_call(), "bad method"));
    }
    let provider = generic_provider(ctx, &generic, env)?;
    ctx.ns_register_s3(&provider, &generic, &class, method)?;
    Ok(SEXP_NIL)
}

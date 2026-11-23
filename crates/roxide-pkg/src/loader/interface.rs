use roxide_eval::{Args, Call, Ctx, EvalResult, Module, Visibility::*};
use roxide_gc::SexpId;
use roxide_value::{BASE_NAMESPACE, FALSE_SCALAR, SEXP_NIL, SexpType, TRUE_SCALAR, ValueApi};

use super::values::*;
use super::{
    attach, export_names, exported, load_with_options, namespace_info, namespace_version, unload,
};
use crate::library::{integer, selected_libraries};
use crate::nsfile::{Import, Selection, parse_namespace_file};

pub(super) const MODULE: Module = Module {
    prims: &[
        internal("C_rx_parse_namespace", 3, parse_namespace_file, On),
        internal("C_rx_load_namespace", 4, load_namespace, On),
        internal("C_rx_attach_namespace", 4, attach_namespace, Off),
        internal("C_rx_unload_namespace", 1, unload_namespace, Off),
        internal("C_rx_namespace_loaded", 1, loaded, On),
        internal("C_rx_namespace_exports", 1, exports, On),
        internal("C_rx_namespace_name", 1, ns_name, On),
        internal("C_rx_namespace_version", 1, version, On),
        internal("C_rx_exported_value", 2, exported_value, On),
        internal("C_rx_namespace_info", 2, get_info, On),
        internal("C_rx_from_namespace", 4, from_namespace, On),
        internal("C_rx_assign_namespace", 5, assign_namespace, Off),
        internal("C_rx_namespace_import", 4, namespace_import, Off),
    ],
    prelude: r#"
.rx_namespace_in <- function(x, table) {
    result <- logical(length(x))
    for (i in seq_along(x)) {
        result[[i]] <- if (is.na(x[[i]])) any(is.na(table)) else any(x[[i]] == table, na.rm = TRUE)
    }
    result
}
parseNamespaceFile <- function(package, package.lib, mustExist = TRUE)
    .Internal(C_rx_parse_namespace(package, package.lib, mustExist))
loadNamespace <- function(package, lib.loc = NULL, keep.source = getOption("keep.source.pkgs"),
                          partial = FALSE, versionCheck = NULL,
                          keep.parse.data = getOption("keep.parse.data.pkgs"))
    .Internal(C_rx_load_namespace(package, lib.loc, partial, versionCheck))
attachNamespace <- function(ns, pos = 2L, depends = NULL, exclude, include.only)
    .Internal(C_rx_attach_namespace(ns, pos, if (missing(exclude)) NULL else exclude,
                                   if (missing(include.only)) NULL else include.only))
unloadNamespace <- function(ns) .Internal(C_rx_unload_namespace(ns))
.rx_cleanup_hook <- function(fun, path, hook, pkg) {
    tryCatch(fun(path), error = function(e) {
        warning(hook, " failed for '", pkg, "': ", conditionMessage(e), call. = FALSE)
    })
}
requireNamespace <- function(package, ..., quietly = FALSE) {
    if (isNamespaceLoaded(package)) return(invisible(TRUE))
    if (!quietly) packageStartupMessage("Loading required namespace: ", package)
    invisible(tryCatch({loadNamespace(package, ...); TRUE}, error = function(e) {
        if (!quietly) message("Failed with error:  ‘", conditionMessage(e), "’")
        FALSE
    }))
}
isNamespaceLoaded <- function(name) .Internal(C_rx_namespace_loaded(name))
getNamespace <- function(name) loadNamespace(name)
getNamespaceExports <- function(ns) .Internal(C_rx_namespace_exports(ns))
getNamespaceImports <- function(ns) getNamespaceInfo(ns, "imports")
getNamespaceName <- function(ns) .Internal(C_rx_namespace_name(ns))
getNamespaceVersion <- function(ns) .Internal(C_rx_namespace_version(ns))
getNamespaceInfo <- function(ns, which) .Internal(C_rx_namespace_info(ns, which))
getExportedValue <- function(ns, name) .Internal(C_rx_exported_value(ns, name))
getFromNamespace <- function(x, ns, pos = -1, envir = as.environment(pos))
    .Internal(C_rx_from_namespace(x, if (missing(ns)) NULL else ns, pos,
                                 if (missing(envir)) NULL else envir))
assignInNamespace <- function(x, value, ns, pos = -1, envir = as.environment(pos))
    .Internal(C_rx_assign_namespace(x, value, if (missing(ns)) NULL else ns, pos,
                                   if (missing(envir)) NULL else envir))
namespaceImport <- function(self, ..., from = NULL, except = character(0L))
    .Internal(C_rx_namespace_import(self, list(...), from, except))
"#,
};

fn load_namespace(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let value = args.get(0);
    if ctx.is_namespace_env(value) {
        return Ok(value);
    }
    let package = string(ctx, value)?;
    let paths = selected_libraries(ctx, args.get(1))?;
    let partial = flag(ctx, args.get(2))?;
    load_with_options(ctx, &package, &paths, args.get(3), call.err_call(), partial)
}

fn as_ns(ctx: &mut Ctx, ns: SexpId) -> EvalResult<SexpId> {
    if ctx.is_namespace_env(ns) {
        return Ok(ns);
    }
    let name = string(ctx, ns)?;
    ctx.load_namespace(&name, SEXP_NIL)
}

fn attach_namespace(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let ns = as_ns(ctx, args.get(0))?;
    let pos = usize::try_from(integer(ctx, args.get(1))?).unwrap_or(0);
    let exclude = chars(ctx, args.get(2))?;
    let include = (args.get(3) != SEXP_NIL)
        .then(|| chars(ctx, args.get(3)))
        .transpose()?;
    attach(ctx, ns, pos, &exclude, include.as_deref())
}

fn unload_namespace(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let value = args.get(0);
    let name = if ctx.is_namespace_env(value) {
        ctx.namespace_name(value).unwrap_or_default()
    } else {
        string(ctx, value)?
    };
    unload(ctx, &name)?;
    Ok(SEXP_NIL)
}

fn loaded(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let name = string(ctx, args.get(0))?;
    Ok(if ctx.ns_lookup(&name).is_some() {
        TRUE_SCALAR
    } else {
        FALSE_SCALAR
    })
}

fn exports(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let ns = as_ns(ctx, args.get(0))?;
    let names = export_names(ctx, ns)?;
    strings(ctx, names.iter().map(String::as_str))
}

fn ns_name(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let ns = as_ns(ctx, args.get(0))?;
    let name = ctx
        .namespace_name(ns)
        .ok_or_else(|| ctx.stop(SEXP_NIL, "not a namespace"))?;
    let value = ctx.session_mut().scalar_str(&name)?;
    named(ctx, value, &["name"])
}

fn version(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let ns = as_ns(ctx, args.get(0))?;
    let version = namespace_version(ctx, ns)?;
    let value = ctx.session_mut().scalar_str(&version)?;
    named(ctx, value, &["version"])
}

fn exported_value(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let ns = as_ns(ctx, args.get(0))?;
    let name = string(ctx, args.get(1))?;
    exported(ctx, ns, &name)
}

fn get_info(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let ns = as_ns(ctx, args.get(0))?;
    let which = string(ctx, args.get(1))?;
    if ns == BASE_NAMESPACE {
        return Ok(SEXP_NIL);
    }
    let info = namespace_info(ctx, ns)?;
    Ok(local(ctx, info, &which)?.unwrap_or(SEXP_NIL))
}

fn namespace_from_env(ctx: &mut Ctx, ns: SexpId, pos: SexpId, env: SexpId) -> EvalResult<SexpId> {
    if ns != SEXP_NIL {
        return as_ns(ctx, ns);
    }
    let env = if env != SEXP_NIL {
        env
    } else {
        crate::library::environment_at(ctx, pos)?
    };
    if ctx.is_namespace_env(env) {
        return Ok(env);
    }
    if let Some(record) = ctx
        .ns_records()
        .iter()
        .find(|rec| rec.attached == Some(env))
    {
        return Ok(record.ns);
    }
    Err(ctx.stop(SEXP_NIL, "environment is not a package environment"))
}

fn from_namespace(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let name = string(ctx, args.get(0))?;
    let ns = namespace_from_env(ctx, args.get(1), args.get(2), args.get(3))?;
    local(ctx, ns, &name)?
        .ok_or_else(|| ctx.stop(call.err_call(), format!("object '{name}' not found")))
}

fn assign_namespace(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let name = string(ctx, args.get(0))?;
    let ns = namespace_from_env(ctx, args.get(2), args.get(3), args.get(4))?;
    let sym = ctx.session_mut().intern(&name)?;
    let locked = ctx
        .session()
        .env_binding_is_locked(ns, sym)?
        .ok_or_else(|| ctx.stop(call.err_call(), format!("no binding for '{name}'")))?;
    if locked {
        ctx.session_mut().env_lock_binding(ns, sym, false)?;
    }
    let result = ctx.assign(sym, args.get(1), ns);
    if locked {
        ctx.session_mut().env_lock_binding(ns, sym, true)?;
    }
    result?;
    Ok(SEXP_NIL)
}

fn namespace_import(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let self_env = args.get(0);
    let ns = ctx.is_namespace_env(self_env);
    let target = if ns {
        ctx.session().env_enclos(self_env)?
    } else {
        self_env
    };
    let info = if ns && self_env != BASE_NAMESPACE {
        Some(namespace_info(ctx, self_env)?)
    } else {
        None
    };
    let mut providers = Vec::new();
    if let Some(info) = info
        && let Some(imports) = local(ctx, info, "imports")?
    {
        let keys = attr(ctx, imports, "names")?;
        providers.extend(
            chars(ctx, keys)?
                .into_iter()
                .zip(ctx.session().list_slice(imports)?.iter().copied()),
        );
    }
    let exclude = chars(ctx, args.get(3))?;
    let sources = ctx.session().list_slice(args.get(1))?.to_vec();
    for source in sources {
        let provider = as_ns(ctx, source)?;
        let package = ctx.namespace_name(provider).unwrap_or_default();
        let import = Import {
            package,
            selection: Selection::All(exclude.clone()),
        };
        if ctx.session().type_of(target)? != SexpType::Env {
            return Err(ctx.stop(SEXP_NIL, "invalid import environment"));
        }
        let bindings = super::imports::import_from(ctx, target, provider, &import, "")?;
        if info.is_some() {
            super::imports::record(ctx, &mut providers, &import.package, bindings)?;
        }
    }
    if let Some(info) = info {
        super::imports::store(ctx, info, &providers)?;
    }
    Ok(SEXP_NIL)
}

//! Load, attach, inspect, and unload package namespaces.

mod imports;
mod interface;
pub(crate) mod values;

use std::path::{Path, PathBuf};

use roxide_eval::{Ctx, CtxExt, EvalResult, Module, NamespaceLoader, NsRecord};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{BASE_ENV, BASE_NAMESPACE, EMPTY_ENV, SEXP_NIL, ValueApi};

use crate::description::{Description, read_text, valid_package};
use crate::library::{find, libraries, package_path, version_check};
use crate::nsfile::NsSpec;

use self::values::*;

/// Base functions for loading and inspecting namespaces.
pub const MODULE: Module = interface::MODULE;

/// Stateless package loader installed in an evaluator context.
pub struct PkgNamespaceLoader;

/// Package loader for namespace operators.
pub static LOADER: PkgNamespaceLoader = PkgNamespaceLoader;

impl NamespaceLoader for PkgNamespaceLoader {
    fn load(&self, ctx: &mut Ctx, name: &str, call: SexpId) -> EvalResult<SexpId> {
        let paths = libraries(ctx)?;
        load(ctx, name, &paths, SEXP_NIL, call)
    }
}

#[derive(Default)]
struct Loading(Vec<String>);

impl Trace for Loading {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for Loading {}

pub(crate) fn load(
    ctx: &mut Ctx,
    name: &str,
    paths: &[PathBuf],
    check: SexpId,
    call: SexpId,
) -> EvalResult<SexpId> {
    load_with_options(ctx, name, paths, check, call, false)
}

fn load_with_options(
    ctx: &mut Ctx,
    name: &str,
    paths: &[PathBuf],
    check: SexpId,
    call: SexpId,
    partial: bool,
) -> EvalResult<SexpId> {
    if let Some(ns) = ctx.ns_lookup(name) {
        if check == SEXP_NIL {
            return Ok(ns);
        }
        let version = namespace_version(ctx, ns)?;
        version_check(ctx, name, &version, check, true)?;
        return Ok(ns);
    }
    let depth = ctx.host().limits().max_expressions.min(128);
    if ctx.ext::<Loading>().0.len() >= depth {
        return Err(ctx.stop(call, "namespace dependency chain is too deeply nested"));
    }
    let loading = &mut ctx.ext::<Loading>().0;
    if loading.iter().any(|package| package == name) {
        let chain = loading
            .iter()
            .rev()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("’, ‘");
        return Err(ctx.stop(call, format!("cyclic namespace dependency detected when loading ‘{name}’, already loading ‘{chain}’")));
    }
    loading.push(name.to_owned());
    let result = ctx.scoped(|ctx| load_new(ctx, name, paths, check, call, partial));
    ctx.ext::<Loading>().0.pop();
    result
}

fn load_new(
    ctx: &mut Ctx,
    name: &str,
    paths: &[PathBuf],
    check: SexpId,
    call: SexpId,
    partial: bool,
) -> EvalResult<SexpId> {
    let path = valid_package(name)
        .then(|| find(ctx, name, paths))
        .flatten()
        .ok_or_else(|| {
            ctx.stop_with_class(
                call,
                format!("there is no package called ‘{name}’"),
                &["packageNotFoundError"],
            )
        })?;
    let description = Description::read(ctx, &path)?;
    if description.package != name {
        return Err(ctx.stop(call, format!("package ‘{name}’ has an invalid DESCRIPTION")));
    }
    version_check(ctx, name, &description.version, check, false)?;
    let filebase = path.join("R").join(name);
    let [index, data] = [".rdx", ".rdb"].map(|suffix| {
        ctx.host()
            .metadata(&path.join("R").join(format!("{name}{suffix}")), true)
            .is_ok_and(|meta| meta.is_file)
    });
    if index != data {
        return Err(ctx.stop(call, "incomplete package lazy-load database"));
    }
    let installed = index;
    let cached = path.join("Meta/nsInfo.rds");
    let spec = if ctx
        .host()
        .metadata(&cached, true)
        .is_ok_and(|meta| meta.is_file)
    {
        NsSpec::from_cache(ctx, &cached)?
    } else {
        NsSpec::read(ctx, &path.join("NAMESPACE"))?
    };
    let (ns, imports) = ctx.ns_create(name, &description.version, Some(&path.to_string_lossy()))?;
    ctx.protect(ns);
    let info = namespace_info(ctx, ns)?;
    imports::populate(ctx, info, imports, &spec, paths, &description)?;
    for dependency in description.depends.iter().filter(|dep| dep.package == "R") {
        crate::library::check_dependency(ctx, dependency, crate::library::R_VERSION)?;
    }
    if !installed {
        crate::data::load_source_sysdata(ctx, &path, ns)?;
        source(ctx, name, &path, &description.collate, ns)?;
        if partial {
            return Ok(ns);
        }
    }
    let record = NsRecord {
        name: name.to_owned(),
        ns,
        imports,
        attached: None,
        path: Some(path.clone()),
        builtin: false,
    };
    let mut cached_meta = false;
    let result = (|| {
        if installed {
            // The byte stream can refer to the namespace that owns it.
            ctx.ns_register(record.clone())?;
            crate::data::load_database(ctx, &path.join("R/sysdata"), ns)?;
            crate::lazyload::load_namespace_db(ctx, &filebase, ns)?;
            if partial {
                ctx.ns_unregister(name);
                return Ok(ns);
            }
        }
        let lazydata = crate::data::lazy_env(ctx, info)?;
        crate::data::load_database(ctx, &path.join("data/Rdata"), lazydata)?;
        if let Some(methods) = ctx.ns_lookup("methods")
            && let Some(cache) = local(ctx, methods, ".rx_cacheMetaData")?
        {
            ctx.apply(cache, &[(None, ns)], SEXP_NIL, methods)?;
            cached_meta = true;
        }
        let dynlibs = spec.dynlib_names(ctx)?;
        define(ctx, info, "dynlibs", dynlibs)?;
        let native = spec.native_routines(ctx)?;
        define(ctx, info, "nativeRoutines", native)?;
        let registrations = crate::s3reg::resolve(ctx, ns, &spec)?;
        if !installed {
            ctx.ns_register(record)?;
        }
        crate::s3reg::install(ctx, registrations)?;
        call_hook(
            ctx,
            ns,
            ".onLoad",
            &[
                path.parent().unwrap_or(&path).to_string_lossy().as_ref(),
                name,
            ],
        )?;
        imports::export(ctx, ns, imports, &spec)?;
        ctx.session_mut().env_lock_all_bindings(ns)?;
        ctx.session_mut().env_set_locked(ns, true)?;
        ctx.session_mut().env_lock_all_bindings(imports)?;
        ctx.session_mut().env_set_locked(imports, true)?;
        Ok(ns)
    })();
    if result.is_err() {
        ctx.protect_eval_result(&result);
        if cached_meta {
            let _ = uncache_methods_metadata(ctx, ns);
        }
        ctx.ns_unregister(name);
    }
    result
}

pub(crate) fn build_namespace(
    ctx: &mut Ctx,
    path: &Path,
    description: &Description,
    paths: &[PathBuf],
    sysdata: SexpId,
) -> EvalResult<SexpId> {
    let name = &description.package;
    let spec = NsSpec::read(ctx, &path.join("NAMESPACE"))?;
    let (ns, imports) = ctx.ns_create(name, &description.version, Some(&path.to_string_lossy()))?;
    ctx.protect(ns);
    let info = namespace_info(ctx, ns)?;
    imports::populate(ctx, info, imports, &spec, paths, description)?;
    for dependency in description.depends.iter().filter(|dep| dep.package == "R") {
        crate::library::check_dependency(ctx, dependency, crate::library::R_VERSION)?;
    }
    crate::data::copy_bindings(ctx, sysdata, ns)?;
    source_files(ctx, path, &description.collate, ns)?;
    ctx.ns_register(NsRecord {
        name: name.clone(),
        ns,
        imports,
        attached: None,
        path: Some(path.to_path_buf()),
        builtin: false,
    })?;
    Ok(ns)
}

fn source(
    ctx: &mut Ctx,
    name: &str,
    path: &Path,
    collate: &[String],
    ns: SexpId,
) -> EvalResult<()> {
    let directory = path.join("R");
    let installed = directory.join(name);
    if ctx
        .host()
        .metadata(&installed, true)
        .is_ok_and(|info| info.is_file)
    {
        return source_file(ctx, &installed, ns);
    }
    source_files(ctx, path, collate, ns)
}

fn source_files(ctx: &mut Ctx, path: &Path, collate: &[String], ns: SexpId) -> EvalResult<()> {
    let directory = path.join("R");
    if !collate.is_empty() {
        for name in collate {
            source_file(ctx, &directory.join(name), ns)?;
        }
    } else {
        source_directory(ctx, &directory, ns)?;
    }
    let platform = if crate::description::is_windows(ctx) {
        "windows"
    } else {
        "unix"
    };
    source_directory(ctx, &directory.join(platform), ns)
}

fn source_directory(ctx: &mut Ctx, directory: &Path, ns: SexpId) -> EvalResult<()> {
    if ctx
        .host()
        .metadata(directory, true)
        .is_ok_and(|info| info.is_dir)
    {
        let mut entries = ctx
            .host()
            .read_dir(directory)
            .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries {
            if !entry.is_dir
                && entry
                    .name
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && entry
                    .path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| matches!(ext, "R" | "r" | "q" | "S" | "s"))
            {
                source_file(ctx, &entry.path, ns)?;
            }
        }
    }
    Ok(())
}

fn source_file(ctx: &mut Ctx, path: &Path, ns: SexpId) -> EvalResult<()> {
    let code = read_text(ctx, path)?;
    ctx.run_script(
        &path.to_string_lossy(),
        code.trim_start_matches('\u{feff}'),
        ns,
        false,
    )
}

pub(crate) fn namespace_info(ctx: &mut Ctx, ns: SexpId) -> EvalResult<SexpId> {
    local(ctx, ns, ".__NAMESPACE__.")?.ok_or_else(|| ctx.stop(SEXP_NIL, "not a namespace"))
}

pub(crate) fn namespace_version(ctx: &mut Ctx, ns: SexpId) -> EvalResult<String> {
    if ns == BASE_NAMESPACE {
        return Ok(crate::library::R_VERSION.to_owned());
    }
    let info = namespace_info(ctx, ns)?;
    let spec = local(ctx, info, "spec")?.unwrap_or(SEXP_NIL);
    let spec = chars(ctx, spec)?;
    Ok(spec.get(1).cloned().unwrap_or_default())
}

pub(crate) fn export_names(ctx: &mut Ctx, ns: SexpId) -> EvalResult<Vec<String>> {
    let Some(exports) = ctx
        .ns_exports_env(ns)?
        .or((ns == BASE_NAMESPACE).then_some(BASE_ENV))
    else {
        return Ok(Vec::new());
    };
    env_names(ctx, exports)
}

fn env_names(ctx: &Ctx, env: SexpId) -> EvalResult<Vec<String>> {
    ctx.session()
        .env_entries(env)?
        .into_iter()
        .map(|(name, _)| Ok(ctx.session().sym_name(name)?.to_owned()))
        .collect()
}

pub(crate) fn exported(ctx: &mut Ctx, ns: SexpId, name: &str) -> EvalResult<SexpId> {
    let value = if ns == BASE_NAMESPACE {
        local(ctx, BASE_ENV, name)?
    } else if let Some(exports) = ctx.ns_exports_env(ns)? {
        if let Some(mapped) = local(ctx, exports, name)? {
            let target = string(ctx, mapped)?;
            let imports = ctx.session().env_enclos(ns)?;
            match local(ctx, ns, &target)? {
                Some(value) => Some(value),
                None => local(ctx, imports, &target)?,
            }
        } else {
            None
        }
    } else {
        None
    };
    value.ok_or_else(|| {
        let pkg = ctx.namespace_name(ns).unwrap_or_default();
        ctx.stop(
            SEXP_NIL,
            format!("'{name}' is not an exported object from 'namespace:{pkg}'"),
        )
    })
}

pub(crate) fn attach(
    ctx: &mut Ctx,
    ns: SexpId,
    pos: usize,
    exclude: &[String],
    include: Option<&[String]>,
) -> EvalResult<SexpId> {
    let name = ctx
        .namespace_name(ns)
        .ok_or_else(|| ctx.stop(SEXP_NIL, "not a namespace"))?;
    if ns == BASE_NAMESPACE
        || ctx
            .ns_record(&name)
            .is_some_and(|record| record.attached.is_some())
    {
        return Err(ctx.stop(SEXP_NIL, "namespace is already attached"));
    }
    if include.is_some() && !exclude.is_empty() {
        return Err(ctx.stop(
            SEXP_NIL,
            "only one of 'include.only' and 'exclude' can be specified",
        ));
    }
    let mut exports = export_names(ctx, ns)?;
    if let Some(include) = include {
        let absent: Vec<_> = include
            .iter()
            .filter(|item| !exports.contains(item))
            .cloned()
            .collect();
        if !absent.is_empty() {
            return Err(ctx.stop(
                SEXP_NIL,
                format!(
                    "not found in namespace ‘{name}’: \n\n    {}\n",
                    absent.join(", ")
                ),
            ));
        }
        exports.retain(|item| include.contains(item));
    }
    for item in exclude.iter().filter(|item| !exports.contains(item)) {
        ctx.warn_now(SEXP_NIL, format!("object '{item}' not found"))?;
    }
    exports.retain(|item| !exclude.contains(item));
    ctx.scoped(|ctx| {
        ctx.protect(ns);
        let env = ctx.session_mut().alloc_env(EMPTY_ENV)?;
        ctx.protect(env);
        for item in exports {
            let value = exported(ctx, ns, &item)?;
            define(ctx, env, &item, value)?;
        }
        let info = namespace_info(ctx, ns)?;
        let lazydata = crate::data::lazy_env(ctx, info)?;
        for (sym, binding) in ctx.session().env_entries(lazydata)? {
            if ctx.session().env_get(env, sym).is_none() {
                ctx.session_mut().env_define(env, sym, binding.value)?;
            }
        }
        let path = package_path(ctx, &name);
        let path_value = path
            .as_ref()
            .map(|path| ctx.session_mut().scalar_str(&path.to_string_lossy()))
            .transpose()?;
        if let Some(value) = path_value {
            ctx.protect(value);
        }
        ctx.search_attach(env, pos, &format!("package:{name}"))?;
        if let Some(path) = path_value {
            set_attr(ctx, env, "path", path)?;
        }
        ctx.ns_set_attached(&name, Some(env));
        let lib = path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or(Path::new(""))
            .to_string_lossy();
        if let Err(error) = call_hook(ctx, ns, ".onAttach", &[lib.as_ref(), &name]) {
            if let Some(index) = ctx.search_path()?.iter().position(|&item| item == env) {
                ctx.search_detach(index + 1)?;
            }
            return Err(error);
        }
        ctx.session_mut().env_lock_all_bindings(env)?;
        ctx.session_mut().env_set_locked(env, true)?;
        Ok(env)
    })
}

fn call_hook(ctx: &mut Ctx, ns: SexpId, hook: &str, args: &[&str]) -> EvalResult<()> {
    ctx.scoped(|ctx| {
        let Some(fun) = local(ctx, ns, hook)? else {
            return Ok(());
        };
        ctx.protect(fun);
        let mut values = Vec::new();
        for arg in args {
            let value = ctx.session_mut().scalar_str(arg)?;
            ctx.protect(value);
            values.push((None, value));
        }
        if hook == ".onLoad" {
            let args: Vec<_> = std::iter::once(fun)
                .chain(values.iter().map(|&(_, value)| value))
                .collect();
            base_call(ctx, ".rx_on_load", &args)?;
        } else {
            ctx.apply(fun, &values, SEXP_NIL, ns)?;
        }
        Ok(())
    })
}

fn call_cleanup_hook(
    ctx: &mut Ctx,
    ns: SexpId,
    hook: &str,
    package: &str,
    path: &str,
) -> EvalResult<()> {
    ctx.scoped(|ctx| {
        let Some(fun) = local(ctx, ns, hook)? else {
            return Ok(());
        };
        ctx.protect(fun);
        let mut args = vec![fun];
        for value in [path, hook, package] {
            let value = ctx.session_mut().scalar_str(value)?;
            ctx.protect(value);
            args.push(value);
        }
        base_call(ctx, ".rx_cleanup_hook", &args)?;
        Ok(())
    })
}

pub(crate) fn detach_at(ctx: &mut Ctx, pos: usize) -> EvalResult<Option<String>> {
    let search = ctx.search_path()?;
    let env = search.get(pos.wrapping_sub(1)).copied();
    let name = ctx
        .ns_records()
        .iter()
        .find(|rec| rec.attached.is_some() && rec.attached == env)
        .map(|rec| rec.name.clone());
    if let Some(name) = &name
        && let Some(ns) = ctx.ns_lookup(name)
    {
        let path = package_path(ctx, name);
        let path = path.as_deref().unwrap_or(Path::new("")).to_string_lossy();
        call_cleanup_hook(ctx, ns, ".onDetach", name, path.as_ref())?;
        if let Some(env) = env {
            let Some(index) = ctx.search_path()?.iter().position(|&item| item == env) else {
                return Ok(Some(name.clone()));
            };
            ctx.search_detach(index + 1)?;
        }
    } else {
        ctx.search_detach(pos)?;
    }
    Ok(name)
}

pub(crate) fn unload(ctx: &mut Ctx, name: &str) -> EvalResult<()> {
    if name == "base" {
        return Err(ctx.stop(SEXP_NIL, "cannot unload base namespace"));
    }
    let Some(record) = ctx.ns_record(name).cloned() else {
        return Ok(());
    };
    ctx.scoped(|ctx| {
        ctx.protect(record.ns);
        if let Some(env) = record.attached
            && let Some(index) = ctx.search_path()?.iter().position(|&item| item == env)
        {
            ctx.protect(env);
            detach_at(ctx, index + 1)?;
        }
        let other: Vec<_> = ctx
            .ns_records()
            .iter()
            .filter(|rec| rec.name != name)
            .map(|rec| (rec.name.clone(), rec.ns))
            .collect();
        let mut users = Vec::new();
        for (user, ns) in other {
            let info = namespace_info(ctx, ns)?;
            let imports = local(ctx, info, "imports")?.unwrap_or(SEXP_NIL);
            if field(ctx, imports, name)?.is_some() {
                users.push(user);
            }
        }
        if !users.is_empty() {
            let users = users.join("’, ‘");
            return Err(ctx.stop(
                SEXP_NIL,
                format!("namespace ‘{name}’ is imported by ‘{users}’ so cannot be unloaded"),
            ));
        }
        let path = package_path(ctx, name);
        let path = path.as_deref().unwrap_or(Path::new("")).to_string_lossy();
        call_cleanup_hook(ctx, record.ns, ".onUnload", name, path.as_ref())?;
        uncache_methods_metadata(ctx, record.ns)?;
        ctx.ns_unregister(name);
        Ok(())
    })
}

fn uncache_methods_metadata(ctx: &mut Ctx, ns: SexpId) -> EvalResult<()> {
    if let Some(methods) = ctx.ns_lookup("methods")
        && let Some(cache) = local(ctx, methods, ".rx_cacheMetaData")?
    {
        ctx.apply(
            cache,
            &[(None, ns), (None, roxide_value::FALSE_SCALAR)],
            SEXP_NIL,
            methods,
        )?;
    }
    Ok(())
}

//! Check the native ABI stamp and bind DLLs to a package namespace.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use roxide_eval::{Ctx, EvalResult, api::NativeHooks};
use roxide_gc::SexpId;
use roxide_host::HostErrorKind;
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use super::values::*;
use crate::install_compiled::abi_revision;
use crate::makeconf::{Makeconf, r_home};
use crate::shlib::shared_extension;

pub(super) fn check_stamp(ctx: &mut Ctx, path: &Path, package: &str) -> EvalResult<()> {
    match ctx.host().metadata(&path.join("libs"), true) {
        Ok(meta) if !meta.is_dir => return Ok(()),
        Err(error) if error.kind == HostErrorKind::NotFound => return Ok(()),
        _ => {}
    }
    let valid = ctx
        .scoped(|ctx| {
            let abi = ctx.with_native_error_handler(
                |ctx| {
                    let file = ctx
                        .session_mut()
                        .scalar_str(&path.join("Meta/Roxide.rds").to_string_lossy())?;
                    ctx.protect(file);
                    let stamp = base_call(ctx, "readRDS", &[file])?;
                    ctx.protect(stamp);
                    if ctx.session().type_of(stamp)? != SexpType::Vec {
                        return Ok(SEXP_NIL);
                    }
                    Ok(field(ctx, stamp, "capi_abi")?.unwrap_or(SEXP_NIL))
                },
                |_, _| Ok(SEXP_NIL),
            )?;
            Ok(ctx
                .session()
                .int(abi)
                .is_ok_and(|values| values.len() == 1 && Some(values.get(0)) == abi_revision()))
        })
        .unwrap_or(false);
    if valid {
        Ok(())
    } else {
        Err(ctx.stop(
            SEXP_NIL,
            format!("package '{package}' was installed by a different R implementation; reinstall it from source with Roxide"),
        ))
    }
}

pub(super) fn load(
    ctx: &mut Ctx,
    ns: SexpId,
    info: SexpId,
    path: &Path,
    dynlibs: SexpId,
    native: SexpId,
    newly_loaded: &mut Vec<PathBuf>,
) -> EvalResult<()> {
    let libraries = chars(ctx, dynlibs)?;
    if libraries.is_empty() {
        return Ok(());
    }
    let hooks = ctx.native_hooks().ok_or_else(|| {
        ctx.stop(
            SEXP_NIL,
            "compiled code requires the staged Roxide R_HOME; run scripts/stage_rhome.sh",
        )
    })?;
    let extension = match r_home(ctx.host()) {
        Ok(home) => Makeconf::read(ctx.host(), &home).and_then(|conf| shared_extension(&conf)),
        Err(_) => Ok(".so".into()),
    }
    .map_err(|error| ctx.stop(SEXP_NIL, error))?;
    let labels = attr(ctx, dynlibs, "names")?;
    let aliases = chars(ctx, labels)?;
    let loaded = hooks.get_loaded_dlls(ctx)?;
    ctx.protect(loaded);
    let mut loaded = dll_paths(ctx, loaded)?
        .into_iter()
        .zip(ctx.session().list_slice(loaded)?.iter().copied())
        .collect::<HashMap<_, _>>();
    let mut dlls = Vec::new();
    for (index, lib) in libraries.iter().enumerate() {
        if lib.is_empty()
            || matches!(lib.as_str(), "." | "..")
            || !lib
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        {
            return Err(ctx.stop(
                SEXP_NIL,
                format!("invalid library name '{lib}' in useDynLib"),
            ));
        }
        let shared = path.join("libs").join(format!("{lib}{extension}"));
        let shared = ctx
            .host()
            .canonicalize(&shared)
            .map_err(|error| ctx.stop(SEXP_NIL, format!("{}: {error}", shared.display())))?;
        let key = shared.to_string_lossy();
        let dll = if let Some(dll) = loaded.get(key.as_ref()) {
            *dll
        } else {
            let dll = hooks.dyn_load(ctx, &shared, true, true, SEXP_NIL)?;
            ctx.protect(dll);
            loaded.insert(key.into_owned(), dll);
            newly_loaded.push(shared);
            dll
        };
        dlls.push(dll);
        if let Some(map) = field(ctx, native, lib)? {
            bind_routines(ctx, ns, hooks, dll, map)?;
        }
        if let Some(alias) = aliases.get(index).filter(|alias| !alias.is_empty()) {
            define(ctx, ns, alias, dll)?;
        }
    }
    let labels = libraries
        .iter()
        .enumerate()
        .map(|(i, lib)| {
            aliases
                .get(i)
                .filter(|alias| !alias.is_empty())
                .unwrap_or(lib)
                .as_str()
        })
        .collect::<Vec<_>>();
    let value = list(ctx, &dlls)?;
    ctx.protect(value);
    named(ctx, value, &labels)?;
    define(ctx, info, "DLLs", value)
}

fn bind_routines(
    ctx: &mut Ctx,
    ns: SexpId,
    hooks: &dyn NativeHooks,
    dll: SexpId,
    map: SexpId,
) -> EvalResult<()> {
    let registration = field(ctx, map, "useRegistration")?.unwrap_or(SEXP_NIL);
    let registration = flag(ctx, registration)?;
    let symbols = field(ctx, map, "symbolNames")?.unwrap_or(SEXP_NIL);
    let names = chars(ctx, symbols)?;
    let mut registered = HashMap::new();
    if registration || !names.is_empty() {
        let groups = hooks.get_dll_registered_routines(ctx, dll, true, SEXP_NIL)?;
        ctx.protect(groups);
        let fixes = field(ctx, map, "registrationFixes")?.unwrap_or(SEXP_NIL);
        let fixes = chars(ctx, fixes)?;
        let prefix = fixes.first().map_or("", String::as_str);
        let suffix = fixes.get(1).map_or("", String::as_str);
        for group in ctx.session().list_slice(groups)?.to_vec() {
            if group == SEXP_NIL {
                continue;
            }
            let labels = attr(ctx, group, "names")?;
            let labels = chars(ctx, labels)?;
            for (i, name) in labels.iter().enumerate() {
                let symbol = ctx.session().list_get(group, i)?;
                if registration {
                    define(ctx, ns, &format!("{prefix}{name}{suffix}"), symbol)?;
                }
                registered.entry(name.clone()).or_insert(symbol);
            }
        }
    }
    let labels = if symbols == SEXP_NIL {
        SEXP_NIL
    } else {
        attr(ctx, symbols, "names")?
    };
    let labels = chars(ctx, labels)?;
    for (i, name) in names.iter().enumerate() {
        let label = labels
            .get(i)
            .filter(|label| !label.is_empty())
            .unwrap_or(name);
        let symbol = if let Some(&symbol) = registered.get(name) {
            symbol
        } else {
            let dll_name = field(ctx, dll, "name")?.unwrap_or(SEXP_NIL);
            let dll_name = string(ctx, dll_name)?;
            let symbol =
                hooks.get_native_symbol_info(ctx, name, Some(&dll_name), registration, SEXP_NIL)?;
            ctx.protect(symbol);
            if field(ctx, symbol, "dll")? != Some(dll) {
                return Err(ctx.stop(
                    SEXP_NIL,
                    format!("cannot resolve symbol '{name}' in the requested DLL '{dll_name}'"),
                ));
            }
            symbol
        };
        ctx.protect(symbol);
        define(ctx, ns, label, symbol)?;
    }
    Ok(())
}

pub(super) fn rollback(ctx: &mut Ctx, paths: &[PathBuf]) {
    if let Some(hooks) = ctx.native_hooks() {
        for path in paths.iter().rev() {
            let _ = ctx.with_native_error_handler(
                |ctx| {
                    hooks.dyn_unload(ctx, path, SEXP_NIL)?;
                    Ok(SEXP_NIL)
                },
                |_, _| Ok(SEXP_NIL),
            );
        }
    }
}

pub(super) fn unload(ctx: &mut Ctx, info: SexpId) -> EvalResult<()> {
    let Some(hooks) = ctx.native_hooks() else {
        return Ok(());
    };
    let Some(dlls) = local(ctx, info, "DLLs")? else {
        return Ok(());
    };
    if dlls == SEXP_NIL {
        return Ok(());
    }
    let owned = dll_paths(ctx, dlls)?;
    if owned.is_empty() {
        return Ok(());
    }
    let loaded = hooks.get_loaded_dlls(ctx)?;
    ctx.protect(loaded);
    let loaded = dll_paths(ctx, loaded)?;
    let mut unloaded = Vec::new();
    for path in owned.into_iter().rev() {
        if loaded.contains(&path) && !unloaded.contains(&path) {
            hooks.dyn_unload(ctx, Path::new(&path), SEXP_NIL)?;
            unloaded.push(path);
        }
    }
    Ok(())
}

fn dll_paths(ctx: &mut Ctx, dlls: SexpId) -> EvalResult<Vec<String>> {
    ctx.session()
        .list_slice(dlls)?
        .to_vec()
        .into_iter()
        .map(|dll| {
            let path = field(ctx, dll, "path")?.unwrap_or(SEXP_NIL);
            string(ctx, path)
        })
        .collect()
}

//! Find, load, and attach packages on the library and search paths.

mod paths;
mod version;

use std::collections::BTreeSet;

use roxide_eval::{Args, Call, Ctx, EvalResult, Module, Visibility::*};
use roxide_gc::SexpId;
use roxide_value::{EMPTY_ENV, NA_INTEGER, SEXP_NIL, SexpType, ValueApi};

use crate::description::{Description, is_windows};
use crate::loader::{attach as attach_ns, detach_at, load, namespace_version, unload, values::*};

pub(crate) use paths::{find, libraries, package_path, selected_libraries};
pub(crate) use version::{check_dependency, version_check};

pub(crate) const R_VERSION: &str = "4.6.2";

/// Base functions for the library and search paths.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_rx_lib_paths", 3, paths::lib_paths, SetByFn),
        internal("C_rx_home", 1, paths::r_home, On),
        internal("C_rx_find_package", 3, paths::find_package, On),
        internal("C_rx_path_package", 2, paths::path_package, On),
        internal("C_rx_system_file", 4, paths::system_file, On),
        internal("C_rx_package_version", 2, version::installed, On),
        internal("C_rx_version", 3, version::construct, On),
        internal("C_rx_version_ops", 3, version::ops, On),
        internal("C_rx_version_format", 1, version::format, On),
        internal("C_rx_library", 8, library, On),
        internal("C_rx_library_info", 2, paths::library_info, On),
        internal("C_rx_attach", 4, attach, Off),
        internal("C_rx_detach", 4, detach, Off),
        internal("C_rx_sys_info", 0, sys_info, On),
        internal("C_rx_platform_id", 0, platform_id, On),
        internal("C_rx_platform", 0, platform, On),
    ],
    prelude: r#"
R.home <- function(component = "home") .Internal(C_rx_home(component))
.libPaths <- function(new, include.site = TRUE)
    .Internal(C_rx_lib_paths(if (missing(new)) NULL else new, include.site, !missing(new)))
find.package <- function(package = NULL, lib.loc = NULL, quiet = FALSE, verbose = getOption("verbose"))
    .Internal(C_rx_find_package(package, lib.loc, quiet))
path.package <- function(package = NULL, quiet = FALSE)
    .Internal(C_rx_path_package(package, quiet))
system.file <- function(..., package = "base", lib.loc = NULL, mustWork = FALSE)
    .Internal(C_rx_system_file(list(...), package, lib.loc, mustWork))
packageVersion <- function(pkg, lib.loc = NULL) .Internal(C_rx_package_version(pkg, lib.loc))
package_version <- function(x, strict = TRUE) .Internal(C_rx_version(x, strict, "package"))
numeric_version <- function(x, strict = TRUE) .Internal(C_rx_version(x, strict, "numeric"))
getRversion <- function() .Internal(C_rx_version(NULL, TRUE, "system"))
Ops.numeric_version <- function(e1, e2) .Internal(C_rx_version_ops(.Generic, e1, e2))
format.numeric_version <- function(x, ...) .Internal(C_rx_version_format(x))
as.character.numeric_version <- function(x, ...) .Internal(C_rx_version_format(x))
print.numeric_version <- function(x, ...) {
    print(format(x), ...)
    invisible(x)
}
Sys.info <- function() .Internal(C_rx_sys_info())
.Platform <- .Internal(C_rx_platform())
R.version <- list(platform = .Internal(C_rx_platform_id()), arch = Sys.info()[["machine"]],
                  os = Sys.info()[["sysname"]], major = "4", minor = "6.2",
                  version.string = "R version 4.6.2 (2026-10-29)")
R.version.string <- R.version$version.string
packageStartupMessage <- function(..., domain = NULL, appendLF = TRUE)
    message(..., domain = domain, appendLF = appendLF)

library <- function(package, help, pos = 2, lib.loc = NULL, character.only = FALSE,
                    logical.return = FALSE, warn.conflicts = TRUE, quietly = FALSE,
                    verbose = getOption("verbose"), mask.ok, exclude, include.only,
                    attach.required = missing(include.only)) {
    if (missing(package)) {
        topic <- if (missing(help)) NULL else if (character.only) help else as.character(substitute(help))
        return(.Internal(C_rx_library_info(topic, lib.loc)))
    }
    pkg <- if (character.only) package else as.character(substitute(package))
    result <- tryCatch(.Internal(C_rx_library(pkg, pos, lib.loc,
                        if (missing(exclude)) NULL else exclude,
                        if (missing(include.only)) NULL else include.only,
                        attach.required, warn.conflicts,
                        if (missing(mask.ok)) NULL else mask.ok)), error = function(e) {
        detail <- conditionMessage(e)
        if (!identical(detail, paste0("there is no package called ‘", pkg, "’")))
            detail <- paste0("package or namespace load failed for ‘", pkg, "’:\n ", detail)
        if (!logical.return) stop(detail, call. = FALSE)
        if (!quietly) warning(detail, call. = FALSE)
        FALSE
    })
    if (logical.return) return(!identical(result, FALSE))
    invisible(result)
}
require <- function(package, lib.loc = NULL, quietly = FALSE, warn.conflicts = TRUE,
                    character.only = FALSE, mask.ok, exclude, include.only,
                    attach.required = missing(include.only)) {
    pkg <- if (character.only) package else as.character(substitute(package))
    if (any(search() == paste0("package:", pkg))) return(invisible(TRUE))
    if (!quietly) packageStartupMessage("Loading required package: ", pkg)
    invisible(library(pkg, lib.loc = lib.loc, character.only = TRUE,
              logical.return = TRUE, warn.conflicts = warn.conflicts, quietly = quietly,
              mask.ok = mask.ok, exclude = exclude, include.only = include.only, attach.required = attach.required))
}
use <- function(package, include.only) {
    pkg <- package
    if (any(search() == paste0("package:", pkg))) return(invisible(TRUE))
    invisible(tryCatch({
        library(pkg, character.only = TRUE, include.only = include.only, quietly = TRUE)
        TRUE
    }, error = function(e) {
        message("Error: ", conditionMessage(e))
        FALSE
    }))
}
attach <- function(what, pos = 2L, name = deparse(substitute(what)), warn.conflicts = TRUE)
    invisible(.Internal(C_rx_attach(what, pos, name, warn.conflicts)))
detach <- function(name, pos = 2L, unload = FALSE, character.only = FALSE, force = FALSE) {
    target <- if (missing(name)) pos else if (character.only) name else substitute(name)
    invisible(.Internal(C_rx_detach(target, pos, unload, force)))
}
"#,
};

pub(crate) fn integer(ctx: &mut Ctx, value: SexpId) -> EvalResult<i32> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Int)?;
    if let Some(number) = ctx
        .session()
        .int(value)?
        .iter()
        .next()
        .filter(|&x| x != NA_INTEGER)
    {
        return Ok(number);
    }
    Err(ctx.stop(SEXP_NIL, "invalid integer argument"))
}

fn attached(ctx: &Ctx, name: &str) -> bool {
    name == "base"
        || ctx
            .ns_record(name)
            .is_some_and(|record| record.attached.is_some())
}

fn library(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let package = string(ctx, args.get(0))?;
    if !attached(ctx, &package) {
        let pos = usize::try_from(integer(ctx, args.get(1))?).unwrap_or(0);
        let paths = selected_libraries(ctx, args.get(2))?;
        let exclude = chars(ctx, args.get(3))?;
        let include = (args.get(4) != SEXP_NIL)
            .then(|| chars(ctx, args.get(4)))
            .transpose()?;
        let required = flag(ctx, args.get(5))?;
        let ns = load(ctx, &package, &paths, SEXP_NIL, call.err_call())?;
        if required {
            attach_dependencies(ctx, &package, &paths, pos, &mut BTreeSet::new())?;
        }
        let warn = flag(ctx, args.get(6))?;
        let mask_ok = chars(ctx, args.get(7))?;
        let env = attach_ns(ctx, ns, pos, &exclude, include.as_deref())?;
        if warn {
            check_conflicts(ctx, env, Some(&package), &mask_ok)?;
        }
    }
    let mut names = Vec::new();
    for env in ctx.search_path()? {
        if env == roxide_value::BASE_ENV {
            names.push("base".to_owned());
        } else if let Some(name) = search_label(ctx, env)?.strip_prefix("package:") {
            names.push(name.to_owned());
        }
    }
    strings(ctx, names.iter().map(String::as_str))
}

fn attach_dependencies(
    ctx: &mut Ctx,
    package: &str,
    paths: &[std::path::PathBuf],
    pos: usize,
    active: &mut BTreeSet<String>,
) -> EvalResult<()> {
    if !active.insert(package.to_owned()) {
        return Err(ctx.stop(SEXP_NIL, "cyclic namespace dependency detected"));
    }
    if let Some(path) = ctx.ns_record(package).and_then(|rec| rec.path.clone()) {
        let description = Description::read(ctx, &path)?;
        for dep in description.depends.iter().filter(|dep| dep.package != "R") {
            let ns = load(ctx, &dep.package, paths, SEXP_NIL, SEXP_NIL)?;
            let version = namespace_version(ctx, ns)?;
            check_dependency(ctx, dep, &version)?;
            if !attached(ctx, &dep.package) {
                attach_dependencies(ctx, &dep.package, paths, pos, active)?;
                attach_ns(ctx, ns, pos, &[], None)?;
            }
        }
    }
    active.remove(package);
    Ok(())
}

pub(super) fn search_label(ctx: &mut Ctx, env: SexpId) -> EvalResult<String> {
    if env == roxide_value::GLOBAL_ENV {
        return Ok(".GlobalEnv".to_owned());
    }
    if env == roxide_value::BASE_ENV {
        return Ok("package:base".to_owned());
    }
    let value = attr(ctx, env, "name")?;
    if value == SEXP_NIL {
        return Ok(String::new());
    }
    string(ctx, value)
}

fn attach(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let source = args.get(0);
    let pos = usize::try_from(integer(ctx, args.get(1))?).unwrap_or(0);
    let name = string(ctx, args.get(2))?;
    if name.is_empty() {
        return Err(ctx.stop(call.err_call(), "'name' must be non-empty"));
    }
    ctx.scoped(|ctx| {
        let env = ctx.session_mut().alloc_env(EMPTY_ENV)?;
        ctx.protect(env);
        match ctx.session().type_of(source)? {
            SexpType::Env => {
                for (sym, binding) in ctx.session().env_entries(source)? {
                    ctx.session_mut().env_define_binding(env, sym, binding)?;
                }
            }
            SexpType::Str => {
                let mut args = vec![(None, source)];
                let envir = ctx.session_mut().intern("envir")?;
                args.push((Some(envir), env));
                let sym = ctx.session_mut().intern("load")?;
                let fun = ctx.find_fun(sym, roxide_value::BASE_ENV)?;
                ctx.apply(fun, &args, SEXP_NIL, roxide_value::BASE_ENV)?;
            }
            SexpType::Vec => {
                let keys = attr(ctx, source, "names")?;
                let keys = chars(ctx, keys)?;
                let items = ctx.session().list_slice(source)?.to_vec();
                if keys.len() != items.len() || keys.iter().any(String::is_empty) {
                    return Err(ctx.stop(
                        call.err_call(),
                        "'attach' only works for lists or data frames with names",
                    ));
                }
                for (key, value) in keys.iter().zip(items) {
                    define(ctx, env, key, value)?;
                }
            }
            _ => {
                return Err(ctx.stop(
                    call.err_call(),
                    "'attach' only works for lists, data frames and environments",
                ));
            }
        }
        ctx.search_attach(env, pos, &name)?;
        if flag(ctx, args.get(3))? {
            check_conflicts(ctx, env, None, &[])?;
        }
        Ok(env)
    })
}

fn check_conflicts(
    ctx: &mut Ctx,
    attached: SexpId,
    package: Option<&str>,
    mask_ok: &[String],
) -> EvalResult<()> {
    let search = ctx.search_path()?;
    let mut above = true;
    let mut heading = package;
    let bindings = ctx.session().env_entries(attached)?;
    for env in search {
        if env == attached {
            above = false;
            continue;
        }
        let mut conflicts = Vec::new();
        for (sym, new) in &bindings {
            let name = ctx.session().sym_name(*sym)?;
            if name.starts_with('.') || mask_ok.iter().any(|item| item == name) {
                continue;
            }
            let Some(old) = ctx.session().env_get(env, *sym) else {
                continue;
            };
            if old.value != new.value && function(ctx, old.value)? == function(ctx, new.value)? {
                conflicts.push(name.to_owned());
            }
        }
        if conflicts.is_empty() {
            continue;
        }
        conflicts.sort();
        if let Some(package) = heading.take() {
            ctx.message(SEXP_NIL, format!("\nAttaching package: ‘{package}’\n"))?;
        }
        let label = search_label(ctx, env)?;
        let objects = if conflicts.len() == 1 {
            "object is"
        } else {
            "objects are"
        };
        let direction = if above { "_by_" } else { "from" };
        ctx.message(
            SEXP_NIL,
            format!(
                "\nThe following {objects} masked {direction} ‘{label}’:\n\n    {}\n\n",
                conflicts.join(", ")
            ),
        )?;
    }
    Ok(())
}

fn detach(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let target = args.get(0);
    let pos = match ctx.session().type_of(target)? {
        SexpType::Sym | SexpType::Str => {
            let name = match ctx.session().sym_name_of(target) {
                Ok(name) => name.to_owned(),
                Err(_) => string(ctx, target)?,
            };
            let mut index = None;
            for (i, env) in ctx.search_path()?.into_iter().enumerate() {
                if search_label(ctx, env)? == name {
                    index = Some(i + 1);
                    break;
                }
            }
            index.ok_or_else(|| ctx.stop(call.err_call(), "invalid 'name' argument"))?
        }
        _ => usize::try_from(integer(ctx, target)?).unwrap_or(0),
    };
    let unload_requested = flag(ctx, args.get(2))?;
    let force = flag(ctx, args.get(3))?;
    if !force
        && let Some(env) = ctx.search_path()?.get(pos.wrapping_sub(1)).copied()
        && let Some(record) = ctx
            .ns_records()
            .iter()
            .find(|rec| rec.attached == Some(env))
    {
        let package = record.name.clone();
        for other in ctx
            .ns_records()
            .to_vec()
            .into_iter()
            .filter(|rec| rec.attached.is_some() && rec.name != package)
        {
            if let Some(path) = other.path
                && Description::read(ctx, &path)?
                    .depends
                    .iter()
                    .any(|dep| dep.package == package)
            {
                return Err(ctx.stop(
                    call.err_call(),
                    format!(
                        "package ‘{package}’ is required by ‘{}’ so will not be detached",
                        other.name
                    ),
                ));
            }
        }
    }
    let package = detach_at(ctx, pos)?;
    if unload_requested && let Some(name) = package {
        unload(ctx, &name)?;
    }
    Ok(SEXP_NIL)
}

pub(crate) fn environment_at(ctx: &mut Ctx, position: SexpId) -> EvalResult<SexpId> {
    if ctx.session().type_of(position)? == SexpType::Env {
        return Ok(position);
    }
    let path = ctx.search_path()?;
    if ctx.session().type_of(position)? == SexpType::Str {
        let label = string(ctx, position)?;
        for env in path {
            if search_label(ctx, env)? == label {
                return Ok(env);
            }
        }
    } else {
        let position = integer(ctx, position)?;
        if position == -1 {
            return Ok(roxide_value::GLOBAL_ENV);
        }
        if let Some(env) = usize::try_from(position)
            .ok()
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| path.get(i))
        {
            return Ok(*env);
        }
    }
    Err(ctx.stop(SEXP_NIL, "invalid 'pos' argument"))
}

fn platform_id(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult<SexpId> {
    let platform = ctx.host().sys_info().platform;
    Ok(ctx.session_mut().scalar_str(&platform)?)
}

fn sys_info(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult<SexpId> {
    let info = ctx.host().sys_info();
    let sysname = match info.sysname.as_str() {
        "macos" | "darwin" => "Darwin",
        "linux" => "Linux",
        "windows" => "Windows",
        "freebsd" => "FreeBSD",
        name => name,
    };
    let machine = if sysname == "Darwin" && info.machine == "aarch64" {
        "arm64"
    } else {
        &info.machine
    };
    let user = ctx.host().env_var("USER");
    let user = user
        .as_deref()
        .and_then(|value| std::str::from_utf8(value).ok());
    let value = optional_strings(
        ctx,
        [
            Some(sysname),
            Some(info.release.as_str()),
            None,
            None,
            Some(machine),
            user,
            user,
            user,
        ],
    )?;
    named(
        ctx,
        value,
        &[
            "sysname",
            "release",
            "version",
            "nodename",
            "machine",
            "login",
            "user",
            "effective_user",
        ],
    )
}

fn platform(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult<SexpId> {
    let windows = is_windows(ctx);
    let os = if windows { "windows" } else { "unix" };
    let dynlib = if windows { ".dll" } else { ".so" };
    let endian = if cfg!(target_endian = "little") {
        "little"
    } else {
        "big"
    };
    let sep = if windows { ";" } else { ":" };
    let mut values = Vec::new();
    for item in [os, "/", dynlib, "unknown", endian, "source", sep, ""] {
        values.push(ctx.session_mut().scalar_str(item)?);
    }
    let value = list(ctx, &values)?;
    named(
        ctx,
        value,
        &[
            "OS.type",
            "file.sep",
            "dynlib.ext",
            "GUI",
            "endian",
            "pkgType",
            "path.sep",
            "r_arch",
        ],
    )
}

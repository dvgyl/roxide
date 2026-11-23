use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use roxide_eval::{Args, Call, Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{SEXP_NIL, ValueApi};

use crate::description::{is_windows, valid_package};
use crate::loader::values::*;

use super::{R_VERSION, search_label};

#[derive(Default)]
struct LibraryPaths {
    active: Option<Vec<PathBuf>>,
    site: Vec<PathBuf>,
}

impl Trace for LibraryPaths {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for LibraryPaths {}

fn env(ctx: &mut Ctx, key: &str) -> Option<String> {
    ctx.host()
        .env_var(key)
        .and_then(|value| String::from_utf8(value).ok())
        .filter(|value| !value.is_empty())
}

fn r_home_dir(ctx: &mut Ctx) -> PathBuf {
    if let Some(home) = env(ctx, "R_HOME").or_else(|| env(ctx, "ROXIDE_R_HOME")) {
        return PathBuf::from(home);
    }
    let executable = env(ctx, "_")
        .map(PathBuf::from)
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| matches!(name, "R" | "Rscript" | "roxide"))
        })
        .or_else(|| {
            let path = env(ctx, "PATH")?;
            let sep = if is_windows(ctx) { ';' } else { ':' };
            path.split(sep)
                .flat_map(|dir| {
                    ["roxide", "Rscript", "R"]
                        .into_iter()
                        .map(move |name| Path::new(dir).join(name))
                })
                .find(|file| {
                    ctx.host()
                        .metadata(file, true)
                        .is_ok_and(|info| info.is_file)
                })
        });
    let home = executable
        .as_deref()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .unwrap_or(Path::new("."))
        .join("lib/roxide");
    ctx.host().canonicalize(&home).unwrap_or(home)
}

fn expand_path(ctx: &mut Ctx, path: &str) -> PathBuf {
    if let Some(home) = ctx.host().home_dir() {
        if path == "~" {
            return home;
        }
        if let Some(rest) = path.strip_prefix("~/") {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn expand_specifiers(ctx: &mut Ctx, value: &str) -> String {
    let info = ctx.host().sys_info();
    let home = r_home_dir(ctx);
    let user = ctx.host().home_dir().unwrap_or_default();
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            result.push(ch);
            continue;
        }
        match chars.next() {
            Some('V') => result.push_str(R_VERSION),
            Some('v') => result.push_str("4.6"),
            Some('p') => result.push_str(&info.platform),
            Some('o') => result.push_str(&info.sysname.to_ascii_lowercase()),
            Some('a') => result.push_str(&info.machine),
            Some('U') => result.push_str(
                &user
                    .join("R")
                    .join(&info.platform)
                    .join("4.6")
                    .to_string_lossy(),
            ),
            Some('S') => result.push_str(&home.join("site-library").to_string_lossy()),
            Some('%') => result.push('%'),
            Some(other) => {
                result.push('%');
                result.push(other);
            }
            None => result.push('%'),
        }
    }
    result
}

fn env_paths(ctx: &mut Ctx, key: &str, default: &str) -> Vec<PathBuf> {
    let value = env(ctx, key).unwrap_or_else(|| default.to_owned());
    let value = expand_specifiers(ctx, &value);
    let sep = if is_windows(ctx) { ';' } else { ':' };
    value
        .split(sep)
        .filter(|part| !part.is_empty())
        .map(|part| expand_path(ctx, part))
        .collect()
}

fn normalize(
    ctx: &mut Ctx,
    paths: impl IntoIterator<Item = PathBuf>,
    home: PathBuf,
) -> Vec<PathBuf> {
    let mut unique = BTreeSet::new();
    let mut result = Vec::new();
    for path in paths {
        if !ctx
            .host()
            .metadata(&path, true)
            .is_ok_and(|meta| meta.is_dir)
        {
            continue;
        }
        let path = ctx.host().canonicalize(&path).unwrap_or(path);
        if unique.insert(path.clone()) {
            result.push(path);
        }
    }
    let home = ctx.host().canonicalize(&home).unwrap_or(home);
    if unique.insert(home.clone()) {
        result.push(home);
    }
    result
}

pub(crate) fn libraries(ctx: &mut Ctx) -> EvalResult<Vec<PathBuf>> {
    if let Some(paths) = &ctx.ext::<LibraryPaths>().active {
        return Ok(paths.clone());
    }
    let r_libs = env_paths(ctx, "R_LIBS", "");
    let user = env_paths(ctx, "R_LIBS_USER", "%U");
    let site = env_paths(ctx, "R_LIBS_SITE", "%S");
    let home = r_home_dir(ctx).join("library");
    let paths = normalize(
        ctx,
        r_libs.into_iter().chain(user).chain(site.iter().cloned()),
        home,
    );
    *ctx.ext::<LibraryPaths>() = LibraryPaths {
        active: Some(paths.clone()),
        site,
    };
    Ok(paths)
}

pub(crate) fn selected_libraries(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<PathBuf>> {
    if value == SEXP_NIL {
        return libraries(ctx);
    }
    Ok(chars(ctx, value)?
        .iter()
        .map(|path| expand_path(ctx, path))
        .collect())
}

pub(crate) fn find(ctx: &mut Ctx, package: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    if !valid_package(package) {
        return None;
    }
    paths
        .iter()
        .map(|path| path.join(package))
        .find_map(|path| {
            ctx.host()
                .metadata(&path.join("DESCRIPTION"), true)
                .ok()
                .filter(|meta| meta.is_file)?;
            Some(ctx.host().canonicalize(&path).unwrap_or(path))
        })
}

pub(crate) fn package_path(ctx: &mut Ctx, package: &str) -> Option<PathBuf> {
    if let Some(path) = ctx.ns_record(package).and_then(|rec| rec.path.clone()) {
        return Some(path);
    }
    ctx.ns_lookup(package)?;
    Some(r_home_dir(ctx).join("library").join(package))
}

pub(super) fn r_home(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let component = string(ctx, args.get(0))?;
    let home = r_home_dir(ctx);
    let path = match component.as_str() {
        "home" => home,
        _ => home.join(component),
    };
    Ok(ctx.session_mut().scalar_str(&path.to_string_lossy())?)
}

pub(super) fn lib_paths(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let mut paths = libraries(ctx)?;
    let set = flag(ctx, args.get(2))?;
    if set {
        let new: Vec<_> = chars(ctx, args.get(0))?
            .iter()
            .map(|path| expand_path(ctx, path))
            .collect();
        let site = if flag(ctx, args.get(1))? {
            ctx.ext::<LibraryPaths>().site.clone()
        } else {
            Vec::new()
        };
        let home = r_home_dir(ctx).join("library");
        paths = normalize(ctx, new.into_iter().chain(site), home);
        ctx.ext::<LibraryPaths>().active = Some(paths.clone());
    }
    let value = strings_from_paths(ctx, &paths)?;
    ctx.set_visible(!set);
    Ok(value)
}

fn strings_from_paths(ctx: &mut Ctx, paths: &[PathBuf]) -> EvalResult<SexpId> {
    strings(
        ctx,
        paths
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .iter()
            .map(|path| path.as_ref()),
    )
}

fn requested_path(
    ctx: &mut Ctx,
    package: &str,
    paths: &[PathBuf],
    loaded: bool,
) -> Option<PathBuf> {
    if loaded && let Some(path) = package_path(ctx, package) {
        return Some(path);
    }
    find(ctx, package, paths)
}

pub(super) fn find_package(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let paths = selected_libraries(ctx, args.get(1))?;
    if args.get(0) == SEXP_NIL {
        return path_package(
            ctx,
            call,
            &mut Args::from_pairs(vec![(None, SEXP_NIL), (None, args.get(2))]),
        );
    }
    let packages = chars(ctx, args.get(0))?;
    let quiet = flag(ctx, args.get(2))?;
    let mut found = Vec::new();
    let mut absent = Vec::new();
    for package in &packages {
        if let Some(path) = requested_path(ctx, package, &paths, args.get(1) == SEXP_NIL) {
            found.push(path);
        } else {
            absent.push(package.as_str());
        }
    }
    if !quiet && !absent.is_empty() {
        let message = if absent.len() == 1 {
            format!("there is no package called ‘{}’", absent[0])
        } else {
            format!("there are no packages called ‘{}’", absent.join("’, ‘"))
        };
        if found.is_empty() {
            return Err(ctx.stop(call.err_call(), message));
        }
        ctx.warn_now(call.err_call(), message)?;
    }
    strings_from_paths(ctx, &found)
}

pub(super) fn path_package(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let requested = chars(ctx, args.get(0))?;
    let mut attached = Vec::new();
    for env in ctx.search_path()? {
        let label = search_label(ctx, env)?;
        if let Some(package) = label.strip_prefix("package:") {
            attached.push(package.to_owned());
        }
    }
    let packages = if args.get(0) == SEXP_NIL {
        attached.clone()
    } else {
        requested
            .into_iter()
            .filter(|package| attached.contains(package))
            .collect()
    };
    let found: Vec<_> = packages
        .iter()
        .filter_map(|package| package_path(ctx, package))
        .collect();
    if found.is_empty() && args.get(0) != SEXP_NIL {
        if flag(ctx, args.get(1))? {
            return Ok(SEXP_NIL);
        }
        return Err(ctx.stop(call.err_call(), "none of the packages are loaded"));
    }
    strings_from_paths(ctx, &found)
}

pub(super) fn system_file(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let package = string(ctx, args.get(1))?;
    let paths = selected_libraries(ctx, args.get(2))?;
    let mut found = Vec::new();
    if let Some(root) = requested_path(ctx, &package, &paths, args.get(2) == SEXP_NIL) {
        let parts = ctx.session().list_slice(args.get(0))?.to_vec();
        let mut paths = vec![root];
        if !parts.is_empty() {
            let result = base_call(ctx, "file.path", &parts)?;
            let components = chars(ctx, result)?;
            let root = &paths[0];
            paths = components
                .iter()
                .map(|path| root.join(path.trim_start_matches('/')))
                .collect();
        }
        for path in paths {
            if ctx.host().metadata(&path, true).is_ok() {
                found.push(path);
            }
        }
    }
    if found.is_empty() {
        if flag(ctx, args.get(3))? {
            return Err(ctx.stop(call.err_call(), "no file found"));
        }
        return Ok(ctx.session_mut().scalar_str("")?);
    }
    strings_from_paths(ctx, &found)
}

pub(super) fn library_info(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let paths = selected_libraries(ctx, args.get(1))?;
    if args.get(0) != SEXP_NIL {
        let name = string(ctx, args.get(0))?;
        let path =
            requested_path(ctx, &name, &paths, args.get(1) == SEXP_NIL).ok_or_else(|| {
                ctx.stop(
                    call.err_call(),
                    format!("there is no package called ‘{name}’"),
                )
            })?;
        let fields = description_fields(ctx, &path)?;
        let lines: Vec<_> = fields
            .iter()
            .map(|(key, value)| {
                let label = format!("{key}:");
                format!("{label:11}{}", value.replace('\n', "\n           "))
            })
            .collect();
        let lines = strings(ctx, lines.iter().map(String::as_str))?;
        let info = list(ctx, &[lines, SEXP_NIL, SEXP_NIL])?;
        let name = ctx.session_mut().scalar_str(&name)?;
        let path = ctx.session_mut().scalar_str(&path.to_string_lossy())?;
        let result = list(ctx, &[name, path, info])?;
        return info_object(ctx, result, &["name", "path", "info"], "packageInfo");
    }
    let mut packages = Vec::new();
    for path in paths {
        let Ok(entries) = ctx.host().read_dir(&path) else {
            continue;
        };
        for entry in entries {
            if !valid_package(&entry.name)
                || !ctx
                    .host()
                    .metadata(&entry.path.join("DESCRIPTION"), true)
                    .is_ok_and(|meta| meta.is_file)
            {
                continue;
            }
            let fields = description_fields(ctx, &entry.path)?;
            if let Some((_, name)) = fields.iter().find(|(key, _)| key == "Package") {
                let title = fields
                    .iter()
                    .find(|(key, _)| key == "Title")
                    .map(|(_, title)| title.as_str())
                    .unwrap_or_default();
                packages.push([
                    name.clone(),
                    path.to_string_lossy().into_owned(),
                    title.replace('\n', " "),
                ]);
            }
        }
    }
    packages.sort();
    let result = strings(
        ctx,
        (0..3).flat_map(|column| packages.iter().map(move |row| row[column].as_str())),
    )?;
    let count = i32::try_from(packages.len())
        .map_err(|_| ctx.stop(call.err_call(), "too many packages"))?;
    let dim = ctx.session_mut().alloc_int_from(&[count, 3])?;
    set_attr(ctx, result, "dim", dim)?;
    let columns = strings(ctx, ["Package", "LibPath", "Title"])?;
    let dimnames = list(ctx, &[SEXP_NIL, columns])?;
    set_attr(ctx, result, "dimnames", dimnames)?;
    let result = list(ctx, &[SEXP_NIL, result, SEXP_NIL])?;
    info_object(ctx, result, &["header", "results", "footer"], "libraryIQR")
}

fn description_fields(ctx: &mut Ctx, path: &Path) -> EvalResult<Vec<(String, String)>> {
    let path = ctx
        .session_mut()
        .scalar_str(&path.join("DESCRIPTION").to_string_lossy())?;
    let data = base_call(ctx, "read.dcf", &[path])?;
    let dimnames = attr(ctx, data, "dimnames")?;
    let labels = ctx.session().list_get(dimnames, 1)?;
    let dim = attr(ctx, data, "dim")?;
    let rows = ctx.session().int(dim)?.get(0);
    if rows == 0 {
        return Ok(Vec::new());
    }
    let labels = chars(ctx, labels)?;
    let values = names(ctx, data)?;
    Ok(labels
        .into_iter()
        .zip(values.into_iter().step_by(rows as usize))
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .collect())
}

fn info_object(ctx: &mut Ctx, value: SexpId, keys: &[&str], class: &str) -> EvalResult<SexpId> {
    named(ctx, value, keys)?;
    let class = ctx.session_mut().scalar_str(class)?;
    set_attr(ctx, value, "class", class)?;
    Ok(value)
}

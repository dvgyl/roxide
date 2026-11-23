//! Build and stage pure-R packages in the current process.

use std::io::Write;
use std::path::{Path, PathBuf};

use roxide_eval::{Ctx, EvalResult, InternalError, Unwind};
use roxide_gc::SexpId;
use roxide_host::{Host, HostErrorKind, NativeHost, OpenMode};
use roxide_value::{SEXP_NIL, ValueApi};

use crate::description::{Dependency, Description, is_windows, read_text, version_parts};
use crate::lazyload::{cache_rds, write_namespace_db};
use crate::library::{R_VERSION, libraries};
use crate::loader::{build_namespace, load, values::*};

type Boot<'a> = &'a dyn Fn(Box<dyn Host>) -> Result<Ctx, InternalError>;

/// Execute `R CMD INSTALL` using fresh evaluator contexts from `boot`.
pub fn run(args: &[String], boot: Boot<'_>) -> i32 {
    run_with_host(args, &mut || Box::new(NativeHost::new()), boot)
}

/// Execute `R CMD INSTALL` with a host factory whose hosts share a filesystem.
pub fn run_with_host(
    args: &[String],
    host: &mut dyn FnMut() -> Box<dyn Host>,
    boot: Boot<'_>,
) -> i32 {
    let options = match Options::parse(args) {
        Ok(options) => options,
        Err(error) => {
            let _ = host().write_err(format!("Error: {error}\n").as_bytes());
            return 1;
        }
    };
    let mut status = 0;
    for source in &options.sources {
        let result = match boot(host()) {
            Ok(mut ctx) => {
                let result = install(&mut ctx, source, &options, host, boot);
                if let Err(error) = &result
                    && !matches!(error, Unwind::TopLevel)
                {
                    let _ = ctx
                        .host()
                        .write_err(format!("Error: {error:?}\n").as_bytes());
                }
                result
            }
            Err(error) => {
                let _ = host().write_err(format!("Error: {error}\n").as_bytes());
                Err(Unwind::Internal(error))
            }
        };
        if result.is_err() {
            status = 1;
        }
    }
    status
}

#[derive(Default)]
struct Options {
    sources: Vec<PathBuf>,
    library: Option<PathBuf>,
    no_test: bool,
    no_docs: bool,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.iter().map(String::as_str);
        let mut positional = false;
        while let Some(arg) = args.next() {
            match arg {
                "--" if !positional => positional = true,
                "-l" | "--library" if !positional => {
                    options.library = Some(
                        args.next()
                            .filter(|arg| !arg.is_empty())
                            .ok_or("missing library directory")?
                            .into(),
                    );
                }
                arg if !positional && arg.starts_with("--library=") => {
                    let value = &arg["--library=".len()..];
                    if value.is_empty() {
                        return Err("missing library directory".into());
                    }
                    options.library = Some(value.into());
                }
                "--no-test-load" if !positional => options.no_test = true,
                "--no-docs" if !positional => options.no_docs = true,
                "--no-byte-compile" | "--no-multiarch" | "--clean" | "--preclean"
                    if !positional => {}
                arg if !positional && arg.starts_with('-') => {
                    return Err(format!("unknown INSTALL option '{arg}'"));
                }
                arg => options.sources.push(arg.into()),
            }
        }
        if options.sources.is_empty() {
            return Err("no packages specified".into());
        }
        Ok(options)
    }
}

fn install(
    ctx: &mut Ctx,
    source: &Path,
    options: &Options,
    host: &mut dyn FnMut() -> Box<dyn Host>,
    boot: Boot<'_>,
) -> EvalResult<()> {
    let mut paths = libraries(ctx)?;
    let library = options
        .library
        .clone()
        .or_else(|| paths.first().cloned())
        .ok_or_else(|| ctx.stop(SEXP_NIL, "no library directory"))?;
    let library = ctx
        .host()
        .canonicalize(&library)
        .map_err(|e| ctx.stop(SEXP_NIL, format!("{}: {e}", library.display())))?;
    paths.retain(|path| path != &library);
    paths.insert(0, library.clone());
    let meta = ctx
        .host()
        .metadata(source, true)
        .map_err(|e| ctx.stop(SEXP_NIL, format!("{}: {e}", source.display())))?;
    if meta.is_dir {
        return install_source(ctx, source, &paths, options, host, boot);
    }
    if !meta.is_file {
        return Err(ctx.stop(
            SEXP_NIL,
            "package source must be a directory or tar archive",
        ));
    }
    let pid = ctx.host().pid();
    let mut nonce = [0; 8];
    ctx.host().fill_entropy(&mut nonce);
    let unpack = library.join(format!(
        "00LOCK-source-{pid}-{:016x}",
        u64::from_ne_bytes(nonce)
    ));
    create_dir(ctx, &unpack, false)?;
    let result = crate::tar::extract(ctx.host(), source, &unpack)
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))
        .and_then(|path| install_source(ctx, &path, &paths, options, host, boot));
    let cleanup = remove(ctx, &unpack);
    result.and(cleanup)
}

fn install_source(
    ctx: &mut Ctx,
    source: &Path,
    paths: &[PathBuf],
    options: &Options,
    host: &mut dyn FnMut() -> Box<dyn Host>,
    boot: Boot<'_>,
) -> EvalResult<()> {
    let text = read_text(ctx, &source.join("DESCRIPTION"))?;
    let description =
        Description::parse(&text, is_windows(ctx)).map_err(|error| ctx.stop(SEXP_NIL, error))?;
    if description.needs_compilation || ctx.host().metadata(&source.join("src"), true).is_ok() {
        return Err(ctx.stop(SEXP_NIL, "compiled code requires Milestone 4"));
    }
    let package = &description.package;
    let Some(library) = paths.first() else {
        return Err(ctx.stop(SEXP_NIL, "no library directory"));
    };
    let lock = library.join(format!("00LOCK-{package}"));
    let stage = lock.join("00new");
    let path = stage.join(package);
    let target = library.join(package);
    let backup = lock.join("previous");
    create_dir(ctx, &lock, false)?;
    let mut preserve_backup = false;
    let result = (|| {
        create_dir(ctx, &path.join("R"), true)?;
        create_dir(ctx, &path.join("Meta"), false)?;
        let mut stage_paths = vec![stage.clone()];
        stage_paths.extend_from_slice(paths);
        build(
            ctx,
            source,
            &path,
            &text,
            &description,
            &stage_paths,
            options.no_docs,
        )?;
        if !options.no_test {
            test_load(ctx, package, &stage_paths, host, boot)?;
        }

        let previous = match ctx.host().metadata(&target, false) {
            Ok(meta) if meta.is_dir && !meta.is_symlink => {
                rename(ctx, &target, &backup)?;
                true
            }
            Ok(_) => return Err(ctx.stop(SEXP_NIL, "installed package path is not a directory")),
            Err(error) if error.kind == HostErrorKind::NotFound => false,
            Err(error) => return Err(ctx.stop(SEXP_NIL, error.to_string())),
        };

        let result = rename(ctx, &path, &target);
        let published = result.is_ok();
        let result = result.and_then(|()| {
            if options.no_test {
                Ok(())
            } else {
                test_load(ctx, package, paths, host, boot)
            }
        });
        if result.is_err() {
            if published && let Err(error) = rename(ctx, &target, &path) {
                preserve_backup = previous;
                return Err(error);
            }
            if previous && let Err(error) = rename(ctx, &backup, &target) {
                preserve_backup = true;
                return Err(error);
            }
        }
        result
    })();
    if preserve_backup {
        let _ = ctx
            .host()
            .write_err(format!("Previous package is in {}\n", backup.display()).as_bytes());
        return result;
    }
    let cleanup = remove(ctx, &lock);
    result.and(cleanup)?;
    let _ = ctx
        .host()
        .write_err(format!("* DONE ({package})\n").as_bytes());
    Ok(())
}

fn build(
    ctx: &mut Ctx,
    source: &Path,
    dest: &Path,
    text: &str,
    description: &Description,
    paths: &[PathBuf],
    no_docs: bool,
) -> EvalResult<()> {
    ctx.scoped(|ctx| {
        let namespace = read_text(ctx, &source.join("NAMESPACE"))?;
        let seconds = ctx.host().now().unix_us as f64 / 1_000_000.0;
        let time = ctx.session_mut().scalar_real(seconds)?;
        ctx.protect(time);
        let class = strings(ctx, ["POSIXct", "POSIXt"])?;
        set_attr(ctx, time, "class", class)?;
        let format = ctx.session_mut().scalar_str("%Y-%m-%d %H:%M:%S UTC")?;
        let utc = ctx.session_mut().scalar_str("UTC")?;
        let date = base_call(ctx, "format.POSIXct", &[time, format, utc])?;
        let date = string(ctx, date)?;
        let os = if is_windows(ctx) { "windows" } else { "unix" };
        let text = with_built(text, &format!("{R_VERSION}; ; {date}; {os}"));
        write_file(ctx, &dest.join("DESCRIPTION"), text.as_bytes())?;
        write_file(ctx, &dest.join("NAMESPACE"), namespace.as_bytes())?;
        copy_inst(ctx, &source.join("inst"), dest, no_docs)?;

        set_library_paths(ctx, paths)?;
        let ns = build_namespace(ctx, source, description, paths)?;
        let package = &description.package;
        let filebase = dest.join("R").join(package);
        write_namespace_db(ctx, &filebase, ns)?;
        write_file(ctx, &filebase, STUB.as_bytes())?;

        let name = ctx.session_mut().scalar_str(package)?;
        let lib = dest
            .parent()
            .ok_or_else(|| ctx.stop(SEXP_NIL, "missing staging directory"))?;
        let lib = ctx.session_mut().scalar_str(&lib.to_string_lossy())?;
        let info = base_call(ctx, "parseNamespaceFile", &[name, lib])?;
        cache_rds(ctx, &dest.join("Meta/nsInfo.rds"), info)?;
        write_metadata(ctx, dest, &date, os, description)
    })
}

const STUB: &str = r#"local({
    info <- loadingNamespaceInfo()
    package <- info$pkgname
    db <- file.path(info$libname, package, "R", package)
    lazyLoad(db, getNamespace(package), function(names) {
        !names %in% c(".__NAMESPACE__.", ".__S3MethodsTable__.")
    })
})
"#;

fn with_built(text: &str, built: &str) -> String {
    let mut result = String::new();
    let mut skip = false;
    for line in text.lines() {
        if line.trim().is_empty() && !result.is_empty() {
            break;
        }
        if !line.starts_with([' ', '\t']) {
            skip = line.starts_with("Built:");
        }
        if !skip {
            result.push_str(line);
            result.push('\n');
        }
    }
    result.push_str("Built: R ");
    result.push_str(built);
    result.push('\n');
    result
}

fn write_metadata(
    ctx: &mut Ctx,
    package: &Path,
    date: &str,
    os: &str,
    dependencies: &Description,
) -> EvalResult<()> {
    let path = ctx
        .session_mut()
        .scalar_str(&package.join("DESCRIPTION").to_string_lossy())?;
    let dcf = base_call(ctx, "read.dcf", &[path])?;
    ctx.protect(dcf);
    let dimnames = attr(ctx, dcf, "dimnames")?;
    let columns = ctx.session().list_get(dimnames, 1)?;
    let labels = chars(ctx, columns)?;
    let values = ctx.session().str_slice(dcf)?.to_vec();
    let description = ctx.session_mut().alloc_str_from(&values)?;
    let labels = labels.iter().map(String::as_str).collect::<Vec<_>>();
    named(ctx, description, &labels)?;

    let r = ctx.session_mut().alloc_int_from(&[4, 6, 2])?;
    let version = list(ctx, &[r])?;
    let class = strings(
        ctx,
        ["R_system_version", "package_version", "numeric_version"],
    )?;
    set_attr(ctx, version, "class", class)?;
    let platform = ctx.session_mut().scalar_str("")?;
    let date = ctx.session_mut().scalar_str(date)?;
    let os = ctx.session_mut().scalar_str(os)?;
    let built = named_list(
        ctx,
        &[version, platform, date, os],
        &["R", "Platform", "Date", "OStype"],
    )?;
    let r_versions = dependencies
        .depends
        .iter()
        .filter(|dep| dep.package == "R")
        .map(|dep| dependency(ctx, dep))
        .collect::<EvalResult<Vec<_>>>()?;
    let r_dep = r_versions.first().copied().unwrap_or(SEXP_NIL);
    let all_r = if r_versions.is_empty() {
        SEXP_NIL
    } else {
        list(ctx, &r_versions)?
    };
    let depends = dependency_list(
        ctx,
        dependencies.depends.iter().filter(|dep| dep.package != "R"),
    )?;
    let suggests = dependency_list(ctx, dependencies.suggests.iter())?;
    let imports = dependency_list(ctx, dependencies.imports.iter())?;
    let linking = list(ctx, &[])?;
    let meta = named_list(
        ctx,
        &[
            description,
            built,
            r_dep,
            all_r,
            depends,
            suggests,
            imports,
            linking,
        ],
        &[
            "DESCRIPTION",
            "Built",
            "Rdepends",
            "Rdepends2",
            "Depends",
            "Suggests",
            "Imports",
            "LinkingTo",
        ],
    )?;
    cache_rds(ctx, &package.join("Meta/package.rds"), meta)
}

fn version_object(ctx: &mut Ctx, version: &str, system: bool) -> EvalResult {
    let parts =
        version_parts(version).ok_or_else(|| ctx.stop(SEXP_NIL, "invalid package version"))?;
    let parts = ctx.session_mut().alloc_int_from(&parts)?;
    let result = list(ctx, &[parts])?;
    let types = ["R_system_version", "package_version", "numeric_version"];
    let class = strings(ctx, types[usize::from(!system)..].iter().copied())?;
    set_attr(ctx, result, "class", class)?;
    Ok(result)
}

fn dependency(ctx: &mut Ctx, dep: &Dependency) -> EvalResult {
    let name = ctx.session_mut().scalar_str(&dep.package)?;
    if let Some((op, version)) = &dep.version {
        let op = ctx.session_mut().scalar_str(op)?;
        let version = version_object(ctx, version, false)?;
        named_list(ctx, &[name, op, version], &["name", "op", "version"])
    } else {
        named_list(ctx, &[name], &["name"])
    }
}

fn dependency_list<'a>(ctx: &mut Ctx, deps: impl Iterator<Item = &'a Dependency>) -> EvalResult {
    let (mut names, mut values) = (Vec::new(), Vec::new());
    for dep in deps {
        names.push(dep.package.as_str());
        values.push(dependency(ctx, dep)?);
    }
    if names.is_empty() {
        list(ctx, &values)
    } else {
        named_list(ctx, &values, &names)
    }
}

fn named_list(ctx: &mut Ctx, values: &[SexpId], labels: &[&str]) -> EvalResult {
    let value = list(ctx, values)?;
    named(ctx, value, labels)
}

fn test_load(
    ctx: &mut Ctx,
    package: &str,
    paths: &[PathBuf],
    host: &mut dyn FnMut() -> Box<dyn Host>,
    boot: Boot<'_>,
) -> EvalResult<()> {
    let mut test = boot(host()).map_err(Unwind::Internal)?;
    set_library_paths(&mut test, paths)
        .and_then(|()| load(&mut test, package, paths, SEXP_NIL, SEXP_NIL).map(|_| ()))
        .map_err(|_| {
            ctx.stop(
                SEXP_NIL,
                format!("test load failed for package '{package}'"),
            )
        })
}

fn set_library_paths(ctx: &mut Ctx, paths: &[PathBuf]) -> EvalResult<()> {
    let paths: Vec<_> = paths.iter().map(|path| path.to_string_lossy()).collect();
    let paths = strings(ctx, paths.iter().map(|path| path.as_ref()))?;
    base_call(ctx, ".libPaths", &[paths])?;
    Ok(())
}

fn copy_inst(ctx: &mut Ctx, source: &Path, dest: &Path, no_docs: bool) -> EvalResult<()> {
    match ctx.host().metadata(source, false) {
        Ok(meta) if meta.is_dir && !meta.is_symlink => {}
        Err(error) if error.kind == HostErrorKind::NotFound => return Ok(()),
        _ => return Err(ctx.stop(SEXP_NIL, "invalid package inst directory")),
    }
    let mut stack = vec![(source.to_path_buf(), dest.to_path_buf())];
    let limit = ctx.host().limits().max_decompress_bytes as u64;
    let mut copied = 0;
    let mut count = 0;
    while let Some((source, dest)) = stack.pop() {
        let entries = ctx
            .host()
            .read_dir(&source)
            .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
        for entry in entries {
            if entry.name.starts_with('.')
                || (source.ends_with("inst")
                    && matches!(
                        entry.name.as_str(),
                        "R" | "Meta" | "DESCRIPTION" | "NAMESPACE"
                    ))
                || (no_docs && entry.name == "doc")
            {
                continue;
            }
            count += 1;
            if count > 100_000 {
                return Err(ctx.stop(SEXP_NIL, "too many package files"));
            }
            let meta = ctx
                .host()
                .metadata(&entry.path, false)
                .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
            let dest = dest.join(&entry.name);
            if meta.is_symlink {
                return Err(ctx.stop(SEXP_NIL, "package inst contains a symbolic link"));
            }
            if meta.is_dir {
                create_dir(ctx, &dest, false)?;
                stack.push((entry.path, dest));
            } else if meta.is_file {
                let from = ctx
                    .host()
                    .open(&entry.path, OpenMode::read_only())
                    .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
                let mut to = ctx
                    .host()
                    .open(&dest, OpenMode::write_truncate())
                    .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
                use std::io::Read;
                let size =
                    std::io::copy(&mut from.take((limit - copied).saturating_add(1)), &mut to)
                        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
                copied += size;
                if copied > limit {
                    return Err(ctx.stop(SEXP_NIL, "package files exceed the size limit"));
                }
            }
        }
    }
    Ok(())
}

fn write_file(ctx: &mut Ctx, path: &Path, data: &[u8]) -> EvalResult<()> {
    let mut file = ctx
        .host()
        .open(path, OpenMode::write_truncate())
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))?;
    file.write_all(data)
        .map_err(|e| ctx.stop(SEXP_NIL, e.to_string()))
}

fn create_dir(ctx: &mut Ctx, path: &Path, recursive: bool) -> EvalResult<()> {
    ctx.host()
        .create_dir(path, recursive, 0o755)
        .map_err(|e| ctx.stop(SEXP_NIL, format!("{}: {e}", path.display())))
}

fn rename(ctx: &mut Ctx, from: &Path, to: &Path) -> EvalResult<()> {
    ctx.host()
        .rename(from, to)
        .map_err(|e| ctx.stop(SEXP_NIL, format!("{}: {e}", from.display())))
}

fn remove(ctx: &mut Ctx, path: &Path) -> EvalResult<()> {
    ctx.host()
        .remove(path, true)
        .map_err(|e| ctx.stop(SEXP_NIL, format!("{}: {e}", path.display())))
}

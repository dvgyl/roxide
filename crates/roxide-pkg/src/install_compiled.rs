//! Build native package code in the installation lock.

use std::path::{Path, PathBuf};

use roxide_eval::{Ctx, EvalResult, api::CAPI_ABI_VERSION};
use roxide_host::HostErrorKind;
use roxide_value::{SEXP_NIL, ValueApi};

use crate::description::{Dependency, Description, valid_package, version_parts};
use crate::install::{PackageTree, copy_tree, create_dir};
use crate::lazyload::cache_rds;
use crate::library::{check_dependency, find};
use crate::loader::values::{base_call, list, named, names, strings};
use crate::makeconf::{Makeconf, env_text, is_file, r_home};
use crate::shlib::{
    self, ShlibOptions, make_args, make_program, path_text, run_process, shared_extension,
};

pub(super) struct CompiledPackage {
    pub(super) source: PathBuf,
    pub(super) linking_to: Vec<Dependency>,
    cxx_standard: Option<&'static str>,
    linking_flags: String,
    home: PathBuf,
    conf: Makeconf,
}

impl CompiledPackage {
    pub(super) fn prepare(
        ctx: &mut Ctx,
        source: &Path,
        lock: &Path,
        paths: &[PathBuf],
        no_configure: bool,
    ) -> EvalResult<Self> {
        let home = r_home(ctx.host()).map_err(|error| ctx.stop(SEXP_NIL, error))?;
        let conf = Makeconf::read(ctx.host(), &home).map_err(|error| ctx.stop(SEXP_NIL, error))?;
        let stage = lock.join("source");
        copy_tree(ctx, source, &stage, PackageTree::Source { lock })?;
        if !no_configure && is_file(ctx.host(), &stage.join("configure")) {
            let configure = (|| {
                let mut args = build_env(&home, &conf)?;
                args.extend([
                    "sh".into(),
                    "-c".into(),
                    "cd \"$1\" && exec sh ./configure".into(),
                    "configure".into(),
                    path_text(&stage)?,
                ]);
                run_process(ctx.host(), "env", args)
            })();
            configure.map_err(|error| ctx.stop(SEXP_NIL, error))?;
        }
        let (linking_to, cxx_standard) = build_fields(ctx, &stage)?;
        let mut flags = Vec::new();
        for dependency in &linking_to {
            let path = find(ctx, &dependency.package, paths).ok_or_else(|| {
                ctx.stop_package_not_found(SEXP_NIL, &dependency.package, SEXP_NIL)
            })?;
            let installed = Description::read(ctx, &path)?;
            check_dependency(ctx, dependency, &installed.version)?;
            let include = path.join("include");
            if ctx
                .host()
                .metadata(&include, true)
                .is_ok_and(|meta| meta.is_dir)
            {
                let include = path_text(&include).map_err(|error| ctx.stop(SEXP_NIL, error))?;
                flags.push(shlib::make_shell_word(&format!("-I{include}")));
            }
        }
        Ok(Self {
            source: stage,
            linking_to,
            cxx_standard,
            linking_flags: flags.join(" "),
            home,
            conf,
        })
    }

    pub(super) fn build(
        &self,
        ctx: &mut Ctx,
        dest: &Path,
        package: &str,
        built: &str,
    ) -> EvalResult<()> {
        let extension = shared_extension(&self.conf).map_err(|error| ctx.stop(SEXP_NIL, error))?;
        let name = format!("{package}{extension}");
        let src = self.source.join("src");
        let binary = src.join(&name);
        if is_file(ctx.host(), &binary) {
            ctx.host()
                .remove(&binary, false)
                .map_err(|error| ctx.stop(SEXP_NIL, error.to_string()))?;
        }
        if is_file(ctx.host(), &src.join("Makefile")) {
            self.custom_make(ctx, &src)
                .map_err(|error| ctx.stop(SEXP_NIL, error))?;
        } else {
            let mut inputs: Vec<_> = ctx
                .host()
                .read_dir(&src)
                .map_err(|error| ctx.stop(SEXP_NIL, format!("{}: {error}", src.display())))?
                .into_iter()
                .filter(|entry| {
                    !entry.is_dir && !entry.name.starts_with('.') && shlib::is_source(&entry.path)
                })
                .map(|entry| PathBuf::from(entry.name))
                .collect();
            inputs.sort();
            ShlibOptions::for_package(inputs, PathBuf::from(&name))
                .build(
                    ctx.host(),
                    &src,
                    &self.home,
                    &self.conf,
                    self.cxx_standard,
                    &self.linking_flags,
                )
                .map_err(|error| ctx.stop(SEXP_NIL, error))?;
        }
        if !is_file(ctx.host(), &binary) {
            return Err(ctx.stop(SEXP_NIL, format!("build did not produce '{name}'")));
        }
        let libs = dest.join("libs");
        create_dir(ctx, &libs, true)?;
        ctx.host()
            .rename(&binary, &libs.join(&name))
            .map_err(|error| ctx.stop(SEXP_NIL, error.to_string()))?;
        write_stamp(ctx, dest, built)
    }

    fn custom_make(&self, ctx: &mut Ctx, src: &Path) -> Result<(), String> {
        let host = ctx.host();
        let mut args = build_env(&self.home, &self.conf)?;
        if !self.linking_flags.is_empty() {
            let inherited = env_text(host, "PKG_CPPFLAGS")?.unwrap_or_default();
            args.push(format!("PKG_CPPFLAGS={inherited} {}", self.linking_flags));
            args.push(format!("CLINK_CPPFLAGS={}", self.linking_flags));
        }
        args.push(make_program(host)?);
        args.extend(make_args(src)?);
        run_process(host, "env", args)
    }
}

fn build_env(home: &Path, conf: &Makeconf) -> Result<Vec<String>, String> {
    let include = conf
        .get("R_INCLUDE_DIR")
        .unwrap_or(path_text(&home.join("include"))?);
    let share = conf
        .get("R_SHARE_DIR")
        .unwrap_or(path_text(&home.join("share"))?);
    Ok(vec![
        format!("R_HOME={}", path_text(home)?),
        format!("R_INCLUDE_DIR={include}"),
        format!("R_SHARE_DIR={share}"),
        format!("SHLIB_EXT={}", shared_extension(conf)?),
    ])
}

fn build_fields(
    ctx: &mut Ctx,
    source: &Path,
) -> EvalResult<(Vec<Dependency>, Option<&'static str>)> {
    let path = ctx
        .session_mut()
        .scalar_str(&source.join("DESCRIPTION").to_string_lossy())?;
    ctx.protect(path);
    let fields = strings(ctx, ["LinkingTo", "SystemRequirements"])?;
    let metadata = base_call(ctx, "read.dcf", &[path, fields])?;
    let values = names(ctx, metadata)?;
    let linking = values
        .first()
        .and_then(Option::as_deref)
        .unwrap_or_default();
    let requirements = values.get(1).and_then(Option::as_deref).unwrap_or_default();
    let standard = ["CXX26", "CXX23", "CXX20", "CXX17", "CXX14", "CXX11"]
        .into_iter()
        .zip(["C++26", "C++23", "C++20", "C++17", "C++14", "C++11"])
        .find(|(name, label)| {
            requirements.split_whitespace().any(|word| {
                let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '+');
                word == *label || word == *name
            })
        })
        .map(|(name, _)| name);
    let dependencies = linking
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            parse_linking_to(entry).ok_or_else(|| {
                ctx.stop(SEXP_NIL, format!("invalid LinkingTo dependency '{entry}'"))
            })
        })
        .collect::<EvalResult<Vec<_>>>()?;
    Ok((dependencies, standard))
}

fn parse_linking_to(entry: &str) -> Option<Dependency> {
    let (package, version) = if let Some((package, requirement)) = entry.split_once('(') {
        let requirement = requirement.strip_suffix(')')?.trim();
        let (op, version) = requirement.split_at(requirement.find(|c: char| c.is_ascii_digit())?);
        let op = op.trim();
        if !matches!(op, ">=" | ">" | "<=" | "<" | "==" | "!=") || version_parts(version).is_none()
        {
            return None;
        }
        (package.trim(), Some((op.into(), version.into())))
    } else {
        (entry, None)
    };
    valid_package(package).then(|| Dependency {
        package: package.into(),
        version,
    })
}

pub(crate) fn abi_revision() -> Option<i32> {
    CAPI_ABI_VERSION.rsplit_once("-v")?.1.parse().ok()
}

fn write_stamp(ctx: &mut Ctx, dest: &Path, built: &str) -> EvalResult<()> {
    let abi = abi_revision().ok_or_else(|| ctx.stop(SEXP_NIL, "invalid Roxide ABI revision"))?;
    let abi = ctx.session_mut().scalar_int(abi)?;
    ctx.protect(abi);
    let version = ctx.session_mut().scalar_str(env!("CARGO_PKG_VERSION"))?;
    ctx.protect(version);
    let built = ctx.session_mut().scalar_str(built)?;
    ctx.protect(built);
    let stamp = list(ctx, &[abi, version, built])?;
    ctx.protect(stamp);
    named(ctx, stamp, &["capi_abi", "roxide_version", "built"])?;
    cache_rds(ctx, &dest.join("Meta/Roxide.rds"), stamp)
}

pub(super) fn test_load(ctx: &mut Ctx, package: &str, paths: &[PathBuf]) -> EvalResult<()> {
    let result = (|| {
        let host = ctx.host();
        let home = r_home(host)?;
        let executable = [home.join("bin/R"), home.join("bin/exec/R")]
            .into_iter()
            .find(|path| is_file(host, path))
            .map_or_else(
                || std::env::current_exe().map_err(|error| error.to_string()),
                Ok,
            )?;
        let mut args = vec![
            "--vanilla".into(),
            "--slave".into(),
            "-e".into(),
            "args <- commandArgs(TRUE); .libPaths(args[-1L]); invisible(loadNamespace(args[[1L]]))"
                .into(),
            "--args".into(),
            package.into(),
        ];
        args.extend(
            paths
                .iter()
                .map(|path| path_text(path))
                .collect::<Result<Vec<_>, _>>()?,
        );
        run_process(host, &path_text(&executable)?, args)
    })();
    result.map_err(|error| {
        ctx.stop(
            SEXP_NIL,
            format!("test load failed for package '{package}': {error}"),
        )
    })
}

pub(super) fn needs_build(
    ctx: &mut Ctx,
    description: &Description,
    source: &Path,
) -> EvalResult<bool> {
    if description.needs_compilation {
        return Ok(true);
    }
    match ctx.host().metadata(&source.join("src"), false) {
        Ok(meta) => Ok(meta.is_dir || meta.is_symlink),
        Err(error) if error.kind == HostErrorKind::NotFound => Ok(false),
        Err(error) => Err(ctx.stop(SEXP_NIL, error.to_string())),
    }
}

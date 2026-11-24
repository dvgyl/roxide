//! Package data files and lazy-load databases.

use std::path::Path;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_host::HostErrorKind;
use roxide_value::{EMPTY_ENV, GLOBAL_ENV, SEXP_NIL, ValueApi};

use crate::lazyload::{DataCompression, cache_rds, load_lazyload_db, write_data_db};
use crate::loader::values::*;

pub(crate) fn load_source_sysdata(ctx: &mut Ctx, package: &Path, env: SexpId) -> EvalResult<()> {
    let path = package.join("R/sysdata.rda");
    if is_file(ctx, &path)? {
        load_workspace(ctx, &path, env)?;
    }
    Ok(())
}

fn is_file(ctx: &mut Ctx, path: &Path) -> EvalResult<bool> {
    match ctx.host().metadata(path, true) {
        Ok(info) => Ok(info.is_file),
        Err(error) if error.kind == HostErrorKind::NotFound => Ok(false),
        Err(error) => Err(ctx.stop(SEXP_NIL, format!("{}: {error}", path.display()))),
    }
}

fn load_workspace(ctx: &mut Ctx, path: &Path, env: SexpId) -> EvalResult {
    let path = ctx.session_mut().scalar_str(&path.to_string_lossy())?;
    base_call(ctx, "load", &[path, env])
}

pub(crate) fn load_database(ctx: &mut Ctx, base: &Path, env: SexpId) -> EvalResult<()> {
    let index = is_file(ctx, &base.with_extension("rdx"))?;
    let data = is_file(ctx, &base.with_extension("rdb"))?;
    if index != data {
        return Err(ctx.stop(
            SEXP_NIL,
            format!("incomplete lazy-load database '{}'", base.display()),
        ));
    }
    if index {
        load_lazyload_db(ctx, base, env)?;
    }
    Ok(())
}

pub(crate) fn lazy_env(ctx: &mut Ctx, info: SexpId) -> EvalResult {
    local(ctx, info, "lazydata")?.ok_or_else(|| ctx.stop(SEXP_NIL, "missing lazydata environment"))
}

pub(crate) fn copy_bindings(ctx: &mut Ctx, from: SexpId, to: SexpId) -> EvalResult<()> {
    for (name, binding) in ctx.session().env_entries(from)? {
        ctx.session_mut().env_define(to, name, binding.value)?;
    }
    Ok(())
}

pub(crate) fn install_lazy_data(
    ctx: &mut Ctx,
    source: &Path,
    dest: &Path,
    compression: DataCompression,
) -> EvalResult<()> {
    ctx.scoped(|ctx| {
        let mut files = match ctx.host().read_dir(&source.join("data")) {
            Ok(files) => files,
            Err(error) if error.kind == HostErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(ctx.stop(SEXP_NIL, error.to_string())),
        };
        files.sort_by(|left, right| left.name.cmp(&right.name));
        let data = ctx.session_mut().alloc_env(EMPTY_ENV)?;
        ctx.protect(data);
        let mut entries = Vec::new();
        let mut labels = Vec::new();
        for file in files {
            let Some((topic, extension)) = file.name.rsplit_once('.') else {
                continue;
            };
            if file.is_dir
                || topic.starts_with('.')
                || !matches!(
                    extension,
                    "rda" | "RData" | "R" | "r" | "csv" | "tab" | "txt"
                )
            {
                continue;
            }
            let names = ctx.scoped(|ctx| {
                let env = ctx.session_mut().alloc_env(GLOBAL_ENV)?;
                ctx.protect(env);
                match extension {
                    "rda" | "RData" => {
                        load_workspace(ctx, &file.path, env)?;
                    }
                    "R" | "r" => {
                        let text = crate::description::read_text(ctx, &file.path)?;
                        let previous = ctx.host().cwd().ok();
                        let data_dir = source.join("data");
                        let _ = ctx.host().set_cwd(&data_dir);
                        let run = ctx.run_script(
                            &file.path.to_string_lossy(),
                            text.trim_start_matches('\u{feff}'),
                            env,
                            false,
                        );
                        if let Some(dir) = previous {
                            let _ = ctx.host().set_cwd(&dir);
                        }
                        run?;
                    }
                    _ => {
                        let path = ctx.session_mut().scalar_str(&file.path.to_string_lossy())?;
                        let csv = if extension == "csv" {
                            roxide_value::TRUE_SCALAR
                        } else {
                            roxide_value::FALSE_SCALAR
                        };
                        let table = base_call(ctx, ".rx_read_package_data", &[path, csv])?;
                        define(ctx, env, topic, table)?;
                    }
                }
                copy_bindings(ctx, env, data)?;
                let mut names = ctx
                    .session()
                    .env_entries(env)?
                    .into_iter()
                    .map(|(name, _)| Ok(ctx.session().sym_name(name)?.to_owned()))
                    .collect::<EvalResult<Vec<_>>>()?;
                names.sort();
                strings(ctx, names.iter().map(String::as_str)).map(|values| (names, values))
            })?;
            for name in names.0 {
                labels.push(if name == topic {
                    name
                } else {
                    format!("{name} ({topic})")
                });
            }
            ctx.protect(names.1);
            entries.push((topic.to_owned(), names.1));
        }
        if entries.is_empty() {
            return Ok(());
        }
        let directory = dest.join("data");
        ctx.host()
            .create_dir(&directory, true, 0o755)
            .map_err(|error| ctx.stop(SEXP_NIL, error.to_string()))?;
        write_data_db(ctx, &directory.join("Rdata"), data, compression)?;
        let index = list(
            ctx,
            &entries.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
        )?;
        let topics = entries
            .iter()
            .map(|(topic, _)| topic.as_str())
            .collect::<Vec<_>>();
        named(ctx, index, &topics)?;
        cache_rds(ctx, &directory.join("Rdata.rds"), index)?;
        labels.sort();
        let count =
            i32::try_from(labels.len()).map_err(|_| ctx.stop(SEXP_NIL, "too many datasets"))?;
        let items = strings(
            ctx,
            labels
                .iter()
                .map(String::as_str)
                .chain(std::iter::repeat_n("", labels.len())),
        )?;
        let dim = ctx.session_mut().alloc_int_from(&[count, 2])?;
        set_attr(ctx, items, "dim", dim)?;
        cache_rds(ctx, &dest.join("Meta/data.rds"), items)
    })
}

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use regex::RegexBuilder;
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_host::{HostErrorKind, HostResult, OpenMode};
use roxide_value::{Encoding, NA_INTEGER, NA_REAL, RLogical, SEXP_NIL, SexpType, ValueApi};

use super::{attr, check_limit, flag, logical, string, strings, text, texts};

pub(super) fn expand(ctx: &mut Ctx, name: &str) -> PathBuf {
    if (name == "~" || name.starts_with("~/"))
        && let Some(home) = ctx.host().home_dir()
    {
        return home.join(name.get(2..).unwrap_or(""));
    }
    PathBuf::from(name)
}

pub(super) fn temp_name(ctx: &mut Ctx, pattern: &str, dir: Option<&str>, ext: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = dir.map_or_else(
        || ctx.host().session_temp_dir().to_string_lossy().into_owned(),
        str::to_owned,
    );
    loop {
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("{dir}/{pattern}{:x}{sequence:016x}{ext}", ctx.host().pid());
        let path = expand(ctx, &name);
        if ctx.host().metadata(&path, false).is_err() {
            return name;
        }
    }
}

pub(super) fn tempfile(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let pattern = texts(ctx, call, args.get(0), "pattern")?;
    let dir = texts(ctx, call, args.get(1), "tmpdir")?;
    let ext = texts(ctx, call, args.get(2), "fileext")?;
    if pattern.is_empty() || dir.is_empty() || ext.is_empty() {
        return Err(ctx.stop(call.err_call(), "invalid argument in tempfile"));
    }
    let count = pattern.len().max(dir.len()).max(ext.len());
    let values: Vec<_> = (0..count)
        .map(|i| {
            Some(temp_name(
                ctx,
                pattern[i % pattern.len()].as_deref().unwrap_or("NA"),
                Some(dir[i % dir.len()].as_deref().unwrap_or("NA")),
                ext[i % ext.len()].as_deref().unwrap_or("NA"),
            ))
        })
        .collect();
    strings(ctx, values, Encoding::Native)
}

pub(super) fn tempdir(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let check = flag(ctx, call, args.get(0), "check")?;
    let dir = ctx.host().session_temp_dir().to_path_buf();
    if check && ctx.host().metadata(&dir, true).is_err() {
        ctx.host()
            .create_dir(&dir, true, 0o700)
            .map_err(|e| ctx.stop(call.err_call(), e.to_string()))?;
    }
    string(ctx, dir.to_string_lossy().as_bytes())
}

pub(super) fn dir_create(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let path = text(ctx, call, args.get(0), "path")?;
    let warn = flag(ctx, call, args.get(1), "showWarnings")?;
    let recursive = flag(ctx, call, args.get(2), "recursive")?;
    let mode = text(ctx, call, args.get(3), "mode")?;
    let mode = u32::from_str_radix(&mode, 8)
        .map_err(|_| ctx.stop(call.err_call(), "invalid 'mode' argument"))?;
    let path = expand(ctx, &path);
    let result = if ctx.host().metadata(&path, true).is_ok() {
        if warn {
            ctx.warn(
                call.err_call(),
                format!("'{}' already exists", path.display()),
            );
        }
        false
    } else {
        let result = ctx.host().create_dir(&path, recursive, mode);
        if let Err(error) = &result
            && warn
        {
            ctx.warn(
                call.err_call(),
                format!("cannot create dir '{}', reason '{error}'", path.display()),
            );
        }
        result.is_ok()
    };
    Ok(ctx
        .session_mut()
        .alloc_lgl_from(&[RLogical::from_option(Some(result))])?)
}

pub(super) fn file_create(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.get(0) == SEXP_NIL {
        return Ok(ctx.session_mut().alloc_lgl_from(&[])?);
    }
    let paths = texts(ctx, call, args.get(0), "file")?;
    let warn = flag(ctx, call, args.get(1), "showWarnings")?;
    let mut values = Vec::with_capacity(paths.len());
    for name in paths {
        let ok = if let Some(name) = name.filter(|s| !s.is_empty()) {
            let path = expand(ctx, &name);
            let result = ctx.host().open(&path, OpenMode::write_truncate());
            if let Err(err) = &result
                && warn
            {
                ctx.warn(
                    call.err_call(),
                    format!("cannot create file '{name}', reason '{err}'"),
                );
            }
            result.is_ok()
        } else {
            false
        };
        values.push(RLogical::from_option(Some(ok)));
    }
    Ok(ctx.session_mut().alloc_lgl_from(&values)?)
}

pub(super) fn unlink(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let paths = texts(ctx, call, args.get(0), "x")?;
    let recursive = flag(ctx, call, args.get(1), "recursive")?;
    let expand_globs = flag(ctx, call, args.get(2), "expand")?;
    let mut status = 0;
    for name in paths.into_iter().flatten().filter(|s| !s.is_empty()) {
        let path = expand(ctx, &name);
        let paths = if expand_globs {
            glob_paths(ctx, &path)
        } else {
            vec![path]
        };
        for path in paths {
            if !recursive
                && ctx
                    .host()
                    .metadata(&path, false)
                    .is_ok_and(|meta| meta.is_dir && !meta.is_symlink)
            {
                continue;
            }
            if let Err(error) = ctx.host().remove(&path, recursive)
                && error.kind != HostErrorKind::NotFound
            {
                status = 1;
            }
        }
    }
    Ok(ctx.session_mut().scalar_int(status)?)
}

fn glob_paths(ctx: &mut Ctx, path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::new()];
    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        if !name.contains(['*', '?']) {
            for path in &mut paths {
                path.push(component);
            }
            continue;
        }
        let expression = format!(
            "^{}$",
            regex::escape(&name)
                .replace("\\*", ".*")
                .replace("\\?", ".")
        );
        let Ok(matcher) = RegexBuilder::new(&expression)
            .dot_matches_new_line(true)
            .build()
        else {
            return Vec::new();
        };
        paths = paths
            .into_iter()
            .filter_map(|path| {
                ctx.host()
                    .read_dir(if path.as_os_str().is_empty() {
                        Path::new(".")
                    } else {
                        &path
                    })
                    .ok()
            })
            .flatten()
            .filter(|entry| {
                (name.starts_with('.') || !entry.name.starts_with('.'))
                    && matcher.is_match(&entry.name)
            })
            .map(|entry| entry.path)
            .collect();
    }
    paths
}

pub(super) fn exists(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.get(0) == SEXP_NIL {
        return Ok(ctx.session_mut().alloc_lgl_from(&[])?);
    }
    let paths = texts(ctx, call, args.get(0), "path")?;
    let dir = flag(ctx, call, args.get(1), "dir")?;
    let mut values = Vec::with_capacity(paths.len());
    for name in paths {
        let exists = name.filter(|s| !s.is_empty()).is_some_and(|name| {
            let path = expand(ctx, &name);
            ctx.host()
                .metadata(&path, true)
                .is_ok_and(|meta| !dir || meta.is_dir)
        });
        values.push(RLogical::from_option(Some(exists)));
    }
    Ok(ctx.session_mut().alloc_lgl_from(&values)?)
}

pub(super) fn file_info(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let paths = if args.get(0) == SEXP_NIL {
        Vec::new()
    } else {
        texts(ctx, call, args.get(0), "path")?
    };
    let extra = flag(ctx, call, args.get(1), "extra_cols")?;
    let mut size = Vec::with_capacity(paths.len());
    let mut is_dir = Vec::with_capacity(paths.len());
    let mut modified = Vec::with_capacity(paths.len());
    for name in &paths {
        let meta = name.as_ref().filter(|s| !s.is_empty()).and_then(|name| {
            let path = expand(ctx, name);
            ctx.host().metadata(&path, true).ok()
        });
        size.push(meta.as_ref().map_or(NA_REAL, |m| m.len as f64));
        is_dir.push(RLogical::from_option(meta.as_ref().map(|m| m.is_dir)));
        modified.push(
            meta.and_then(|m| m.modified_us)
                .map_or(NA_REAL, |us| us as f64 / 1_000_000.0),
        );
    }

    let n = paths.len();
    let size = ctx.session_mut().alloc_real_from(&size)?;
    let is_dir = ctx.session_mut().alloc_lgl_from(&is_dir)?;
    let mode = ctx.session_mut().alloc_int_from(&vec![NA_INTEGER; n])?;
    let octmode = string(ctx, "octmode")?;
    attr(ctx, mode, "class", octmode)?;
    let times = strings(ctx, [Some("POSIXct"), Some("POSIXt")], Encoding::Native)?;
    let modified = ctx.session_mut().alloc_real_from(&modified)?;
    let unknown_time = ctx.session_mut().alloc_real_from(&vec![NA_REAL; n])?;
    attr(ctx, modified, "class", times)?;
    attr(ctx, unknown_time, "class", times)?;

    let mut columns = vec![size, is_dir, mode, modified, unknown_time, unknown_time];
    let mut names = vec!["size", "isdir", "mode", "mtime", "ctime", "atime"];
    if extra {
        let unknown_id = ctx.session_mut().alloc_int_from(&vec![NA_INTEGER; n])?;
        let unknown_name = strings(ctx, std::iter::repeat_n(None::<&str>, n), Encoding::Native)?;
        columns.extend([unknown_id, unknown_id, unknown_name, unknown_name]);
        names.extend(["uid", "gid", "uname", "grname"]);
    }
    let result = ctx.session_mut().alloc_list(columns.len())?;
    for (i, value) in columns.into_iter().enumerate() {
        ctx.session_mut().list_set(result, i, value)?;
    }
    let names = strings(ctx, names.into_iter().map(Some), Encoding::Native)?;
    let row_names = strings(ctx, paths, Encoding::Native)?;
    let class = string(ctx, "data.frame")?;
    attr(ctx, result, "names", names)?;
    attr(ctx, result, "row.names", row_names)?;
    attr(ctx, result, "class", class)?;
    Ok(result)
}

pub(super) fn rename(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let from = texts(ctx, call, args.get(0), "from")?;
    let to = texts(ctx, call, args.get(1), "to")?;
    if from.len() != to.len() {
        return Err(ctx.stop(call.err_call(), "'from' and 'to' are of different lengths"));
    }
    let mut result = Vec::with_capacity(from.len());
    for (from, to) in from.into_iter().zip(to) {
        let ok = if let (Some(from), Some(to)) = (from, to) {
            let src = expand(ctx, &from);
            let dst = expand(ctx, &to);
            let result = ctx.host().rename(&src, &dst);
            if let Err(err) = &result {
                ctx.warn(
                    call.err_call(),
                    format!("cannot rename file '{from}' to '{to}', reason '{err}'"),
                );
            }
            result.is_ok()
        } else {
            false
        };
        result.push(RLogical::from_option(Some(ok)));
    }
    Ok(ctx.session_mut().alloc_lgl_from(&result)?)
}

pub(super) fn list_files(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let paths = texts(ctx, call, args.get(0), "path")?;
    let pattern = if args.get(1) == SEXP_NIL {
        None
    } else {
        Some(text(ctx, call, args.get(1), "pattern")?)
    };
    let hidden = flag(ctx, call, args.get(2), "all.files")?;
    let full = flag(ctx, call, args.get(3), "full.names")?;
    let recursive = flag(ctx, call, args.get(4), "recursive")?;
    let ignore_case = flag(ctx, call, args.get(5), "ignore.case")?;
    let include_dirs = flag(ctx, call, args.get(6), "include.dirs")?;
    let no_dots = flag(ctx, call, args.get(7), "no..")?;
    let fixed = flag(ctx, call, args.get(8), "fixed")?;
    let pattern = pattern
        .map(|s| {
            let expression = if fixed { regex::escape(&s) } else { s };
            RegexBuilder::new(&expression)
                .case_insensitive(ignore_case)
                .build()
        })
        .transpose()
        .map_err(|_| ctx.stop(call.err_call(), "invalid 'pattern' regular expression"))?;
    let matches = |name: &str| pattern.as_ref().is_none_or(|p| p.is_match(name));
    let mut result = Vec::new();
    let mut used = 0usize;
    let mut append = |ctx: &mut Ctx, root: &str, relative: &str| -> EvalResult<()> {
        used = used.saturating_add(relative.len() + size_of::<String>());
        if full {
            used = used.saturating_add(root.len() + 1);
        }
        check_limit(ctx, call, used)?;
        let path = if full {
            format!("{root}/{relative}")
        } else {
            relative.to_owned()
        };
        result.push(Some(path));
        Ok(())
    };
    for root in paths.into_iter().flatten().filter(|path| !path.is_empty()) {
        let path = expand(ctx, &root);
        let expanded = path.to_string_lossy().into_owned();
        let mut dirs = vec![(path, String::new())];
        while let Some((path, relative)) = dirs.pop() {
            let Ok(entries) = ctx.host().read_dir(&path) else {
                continue;
            };
            if !recursive && hidden && !no_dots {
                for dot in [".", ".."].into_iter().filter(|dot| matches(dot)) {
                    append(ctx, &expanded, dot)?;
                }
            }
            for entry in entries {
                if !hidden && entry.name.starts_with('.') {
                    continue;
                }
                let name = join(&relative, &entry.name);
                if (!entry.is_dir || !recursive || include_dirs) && matches(&entry.name) {
                    append(ctx, &expanded, &name)?;
                }
                if entry.is_dir && recursive {
                    dirs.push((entry.path, name));
                }
            }
        }
    }
    result.sort();
    strings(ctx, result, Encoding::Native)
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

pub(super) fn file_path(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let separator = text(ctx, call, args.get(1), "fsep")?;
    let parts = ctx.session().list_slice(args.get(0))?.to_vec();
    let mut count = 0;
    let mut vectors = Vec::with_capacity(parts.len());
    for value in parts {
        let (value, _) = ctx.session_mut().coerce(value, SexpType::Str)?;
        let part = texts(ctx, call, value, "path")?;
        if part.is_empty() {
            return strings(ctx, std::iter::empty::<Option<&str>>(), Encoding::Native);
        }
        count = count.max(part.len());
        vectors.push(part);
    }
    let mut output = Vec::new();
    let mut used = 0usize;
    for i in 0..count {
        let mut path = String::new();
        for (j, part) in vectors.iter().enumerate() {
            let segment = part[i % part.len()].as_deref().unwrap_or("NA");
            used = used.saturating_add(segment.len() + usize::from(j > 0) * separator.len());
            check_limit(ctx, call, used)?;
            if j > 0 {
                path.push_str(&separator);
            }
            path.push_str(segment);
        }
        output.push(Some(path));
    }
    strings(ctx, output, Encoding::Native)
}

pub(super) fn path_parts(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let paths = texts(ctx, call, args.get(0), "path")?;
    let directory = flag(ctx, call, args.get(1), "directory")?;
    let values: Vec<_> = paths
        .into_iter()
        .map(|value| {
            value.map(|value| {
                let expanded = expand(ctx, &value);
                let path = expanded.to_string_lossy();
                let trimmed = path.trim_end_matches('/');
                if directory {
                    match trimmed.rsplit_once('/') {
                        Some((parent, _)) => {
                            let parent = parent.trim_end_matches('/');
                            if parent.is_empty() {
                                "/".to_owned()
                            } else {
                                parent.to_owned()
                            }
                        }
                        None if path.is_empty() => String::new(),
                        None if trimmed.is_empty() => "/".to_owned(),
                        None => ".".to_owned(),
                    }
                } else {
                    trimmed.rsplit('/').next().unwrap_or("").to_owned()
                }
            })
        })
        .collect();
    strings(ctx, values, Encoding::Native)
}

pub(super) fn normalize(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let paths = texts(ctx, call, args.get(0), "path")?;
    let separator = text(ctx, call, args.get(1), "winslash")?;
    let must_work = logical(ctx, args.get(2))?;
    let mut result = Vec::with_capacity(paths.len());
    for (i, name) in paths.into_iter().enumerate() {
        let Some(name) = name else {
            let msg = format!("path[{}]=NA", i + 1);
            match must_work {
                Some(true) => return Err(ctx.stop(call.err_call(), msg)),
                None => ctx.warn(call.err_call(), msg),
                Some(false) => {}
            }
            result.push(None);
            continue;
        };
        let path = expand(ctx, &name);
        let path = match canonical_path(ctx, &path) {
            Ok(path) => {
                let path = path.to_string_lossy();
                if cfg!(windows) && separator == "/" {
                    path.replace('\\', "/")
                } else {
                    path.into_owned()
                }
            }
            Err(err) => {
                let msg = format!("path[{}]=\"{name}\": {err}", i + 1);
                match must_work {
                    Some(true) => return Err(ctx.stop(call.err_call(), msg)),
                    None => ctx.warn(call.err_call(), msg),
                    Some(false) => {}
                }
                name
            }
        };
        result.push(Some(path));
    }
    strings(ctx, result, Encoding::Native)
}

fn canonical_path(ctx: &mut Ctx, path: &Path) -> HostResult<PathBuf> {
    let result = ctx.host().canonicalize(path);
    if result.is_ok() || !path.components().any(|part| part == Component::ParentDir) {
        return result;
    }

    // Resolve each prefix before processing ".." so symbolic links keep their meaning.
    let mut prefix = PathBuf::new();
    for part in path.components() {
        if part == Component::ParentDir {
            if prefix.as_os_str().is_empty() {
                prefix = ctx.host().cwd()?;
            }
            if !ctx.host().metadata(&prefix, true)?.is_dir {
                return result;
            }
            prefix = ctx.host().canonicalize(&prefix)?;
            prefix.pop();
        } else {
            prefix.push(part);
        }
    }
    ctx.host().canonicalize(&prefix)
}

pub(super) fn getwd(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    match ctx.host().cwd() {
        Ok(cwd) => string(ctx, cwd.to_string_lossy().as_bytes()),
        Err(_) => Ok(SEXP_NIL),
    }
}

pub(super) fn setwd(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let dir = text(ctx, call, args.get(0), "dir")?;
    let old = getwd(ctx, call, args)?;
    let path = expand(ctx, &dir);
    ctx.host()
        .set_cwd(&path)
        .map_err(|_| ctx.stop(call.err_call(), "cannot change working directory"))?;
    Ok(old)
}

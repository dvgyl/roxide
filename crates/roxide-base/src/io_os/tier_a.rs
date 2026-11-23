use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind,
    Unwind, Visibility,
};
use roxide_gc::SexpId;
use roxide_host::{OpenMode, StdStream};
use roxide_value::{CharId, RLogical, SEXP_NIL, ValueApi};

use crate::ops::common::{ch, error, flag, names, number, set_names, string, text, vector_limit};

pub(crate) const MODULE: Module = Module {
    prims: &[
        internal("rox_path_expand", path_expand, 1, Visibility::On),
        internal("rox_file_copy", file_copy, 4, Visibility::On),
        internal("rox_file_remove", file_remove, 1, Visibility::On),
        internal("rox_file_append", file_append, 2, Visibility::On),
        internal("rox_file_access", file_access, 2, Visibility::On),
        internal("rox_sys_time", sys_time, 0, Visibility::On),
        internal("rox_sys_sleep", sys_sleep, 1, Visibility::Off),
        internal("rox_sys_pid", sys_pid, 0, Visibility::On),
        internal("rox_sys_locale", sys_locale, 0, Visibility::On),
        internal("rox_sys_which", sys_which, 1, Visibility::On),
        internal("rox_isatty", isatty, 1, Visibility::On),
        internal("rox_interactive", interactive, 0, Visibility::On),
        internal("rox_readline", readline, 1, Visibility::On),
        internal("rox_proc_time", proc_time, 0, Visibility::On),
        internal("rox_time_safepoint", time_safepoint, 0, Visibility::Off),
        internal("rox_quit", quit, 2, Visibility::Off),
    ],
    prelude: include_str!("tier_a.R"),
};

const fn internal(name: &'static str, fun: PrimFn, n: usize, visibility: Visibility) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(n),
        dispatch: Dispatch::None,
        visibility,
    }
}

fn paths(ctx: &mut Ctx, x: SexpId) -> EvalResult<Vec<Option<PathBuf>>> {
    if x == SEXP_NIL {
        return Ok(Vec::new());
    }
    let cwd_home = ctx.host().home_dir();
    let s = ctx.session();
    Ok(s.str_slice(x)?
        .iter()
        .map(|&id| {
            if id == CharId::NA {
                return None;
            }
            let value = text(s, id);
            if let Some(home) = &cwd_home {
                if value == "~" {
                    return Some(home.clone());
                }
                if let Some(rest) = value.strip_prefix("~/") {
                    return Some(home.join(rest));
                }
            }
            Some(PathBuf::from(value))
        })
        .collect())
}

fn path_expand(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let paths = paths(ctx, args.get(0))?;
    let s = ctx.session_mut();
    let ids = paths
        .iter()
        .map(|path| match path {
            Some(path) => ch(s, &path.to_string_lossy()),
            None => Ok(CharId::NA),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(s.alloc_str_from(&ids)?)
}

fn file_remove(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let paths = paths(ctx, args.get(0))?;
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let deleted = if let Some(path) = path {
            match ctx.host().remove(&path, false) {
                Ok(()) => true,
                Err(e) => {
                    ctx.warn_now(
                        call.err_call(),
                        format!("cannot remove file '{}', reason '{e}'", path.display()),
                    )?;
                    false
                }
            }
        } else {
            false
        };
        out.push(RLogical::from_option(Some(deleted)));
    }
    Ok(ctx.session_mut().alloc_lgl_from(&out)?)
}

fn file_copy(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let from = paths(ctx, args.get(0))?;
    let to = paths(ctx, args.get(1))?;
    let overwrite = flag(ctx, args.get(2), false)?;
    let recursive = flag(ctx, args.get(3), false)?;
    if to.is_empty() || (to.len() != 1 && to.len() != from.len()) {
        return Err(error(ctx, call, "more 'from' files than 'to' files"));
    }
    let to_dir = to.len() == 1
        && to[0].as_ref().is_some_and(|path| {
            ctx.host()
                .metadata(path, true)
                .is_ok_and(|meta| meta.is_dir)
        });
    if from.len() > 1 && to.len() == 1 && !to_dir {
        return Err(error(ctx, call, "'to' must be an existing directory"));
    }
    if recursive && !to_dir {
        ctx.warn_now(
            call.err_call(),
            "'recursive' will be ignored as 'to' is not a single existing directory",
        )?;
    }
    let mut out = Vec::with_capacity(from.len());
    for (i, from) in from.iter().enumerate() {
        let success = if let (Some(from), Some(to)) = (from, &to[i % to.len()]) {
            let to = if to_dir {
                if let Some(name) = from.file_name() {
                    to.join(name)
                } else {
                    out.push(RLogical::FALSE);
                    continue;
                }
            } else {
                to.clone()
            };
            match copy_path(ctx, from, &to, overwrite, recursive && to_dir, 0) {
                Ok(()) => true,
                Err(message) => {
                    if !message.is_empty() {
                        ctx.warn_now(call.err_call(), message)?;
                    }
                    false
                }
            }
        } else {
            false
        };
        out.push(RLogical::from_option(Some(success)));
    }
    Ok(ctx.session_mut().alloc_lgl_from(&out)?)
}

fn copy_path(
    ctx: &mut Ctx,
    from: &Path,
    to: &Path,
    overwrite: bool,
    recursive: bool,
    depth: usize,
) -> Result<(), String> {
    if depth >= 128 || ctx.host().poll_interrupt() {
        return Err("file copy interrupted or directory nesting limit exceeded".to_owned());
    }
    let source = ctx
        .host()
        .metadata(from, false)
        .map_err(|_| String::new())?;
    let dest = ctx.host().metadata(to, true).ok();
    if same_path(ctx, from, to) {
        return Err(format!(
            "file can not be copied both 'from' and 'to': '{}'",
            from.display()
        ));
    }
    if source.is_dir {
        if !recursive {
            return Err(String::new());
        }
        if depth == 0
            && let (Ok(origin), Some(parent)) = (ctx.host().canonicalize(from), to.parent())
            && ctx
                .host()
                .canonicalize(parent)
                .is_ok_and(|child| child.starts_with(origin))
        {
            return Err("cannot copy a directory into itself".to_owned());
        }
        if !dest.is_some_and(|meta| meta.is_dir) {
            ctx.host()
                .create_dir(to, false, 0o777)
                .map_err(|err| err.to_string())?;
        }
        let entries = ctx.host().read_dir(from).map_err(|err| err.to_string())?;
        for entry in entries {
            copy_path(
                ctx,
                &entry.path,
                &to.join(entry.name),
                overwrite,
                recursive,
                depth + 1,
            )?;
        }
    } else {
        if dest.is_some() && !overwrite {
            return Err(String::new());
        }
        transfer(ctx, from, to, false).map_err(|reason| {
            format!(
                "problem copying {} to {}: {reason}",
                from.display(),
                to.display()
            )
        })?;
    }
    Ok(())
}

fn same_path(ctx: &mut Ctx, a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    if let Ok(a) = ctx.host().canonicalize(a) {
        ctx.host().canonicalize(b).is_ok_and(|b| a == b)
    } else {
        false
    }
}

fn transfer(ctx: &mut Ctx, from: &Path, to: &Path, append: bool) -> Result<(), String> {
    let meta = ctx
        .host()
        .metadata(from, true)
        .map_err(|err| err.to_string())?;
    if !meta.is_file {
        return Err("source is not a regular file".to_owned());
    }
    let mut source = ctx
        .host()
        .open(from, OpenMode::read_only())
        .map_err(|err| err.to_string())?
        .take(meta.len);
    let mut dest = ctx
        .host()
        .open(
            to,
            OpenMode {
                truncate: !append,
                append,
                ..OpenMode::write_truncate()
            },
        )
        .map_err(|err| err.to_string())?;
    io::copy(&mut source, &mut dest).map_err(|err| err.to_string())?;
    Ok(())
}

fn file_append(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let to = paths(ctx, args.get(0))?;
    let from = paths(ctx, args.get(1))?;
    if to.is_empty() || from.is_empty() {
        return Ok(ctx.session_mut().alloc_lgl_from(&[])?);
    }
    let mut out = Vec::new();
    for i in 0..to.len().max(from.len()) {
        let success = match (&from[i % from.len()], &to[i % to.len()]) {
            (Some(from), Some(to)) if !same_path(ctx, from, to) => {
                match transfer(ctx, from, to, true) {
                    Ok(()) => true,
                    Err(reason) => {
                        ctx.warn_now(
                            call.err_call(),
                            format!("cannot append file '{}', reason '{reason}'", from.display()),
                        )?;
                        false
                    }
                }
            }
            _ => false,
        };
        out.push(RLogical::from_option(Some(success)));
    }
    Ok(ctx.session_mut().alloc_lgl_from(&out)?)
}

fn file_access(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mode = number(ctx.session_mut(), args.get(1))?;
    if !(0.0..=7.0).contains(&mode) {
        return Err(error(ctx, call, "invalid 'mode' argument"));
    }
    let mode = mode as u8;
    let paths = paths(ctx, args.get(0))?;
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let ok = path
            .filter(|p| !p.as_os_str().is_empty())
            .is_some_and(|path| {
                let Ok(meta) = ctx.host().metadata(&path, true) else {
                    return false;
                };
                if mode == 0 {
                    return true;
                }
                if meta.is_dir {
                    return ctx.host().read_dir(&path).is_ok();
                }
                let read = mode & 4 != 0;
                let write = mode & 2 != 0;
                if read || write {
                    ctx.host()
                        .open(
                            &path,
                            OpenMode {
                                read,
                                write,
                                ..OpenMode::read_only()
                            },
                        )
                        .is_ok()
                } else {
                    meta.is_file
                }
            });
        out.push(if ok { 0 } else { -1 });
    }
    let s = ctx.session_mut();
    let result = s.alloc_int_from(&out)?;
    let x = args.get(0);
    if let Ok(ids) = s.str_slice(x) {
        let ids = ids.to_vec();
        set_names(s, result, &ids)?;
    }
    Ok(result)
}

fn sys_time(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let seconds = ctx.host().now().unix_us as f64 / 1_000_000.0;
    Ok(ctx.session_mut().scalar_real(seconds)?)
}

fn sys_sleep(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let seconds = number(ctx.session_mut(), args.get(0))?;
    let duration = Duration::try_from_secs_f64(seconds)
        .map_err(|_| error(ctx, call, "invalid 'time' value"))?;
    ctx.host()
        .sleep(duration)
        .map_err(|err| error(ctx, call, err.to_string()))?;
    Ok(SEXP_NIL)
}

fn sys_pid(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let pid = ctx.host().pid();
    Ok(ctx.session_mut().scalar_int(pid as i32)?)
}

fn sys_locale(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let locale = ctx.host().locale();
    let s = ctx.session_mut();
    let ctype = s.scalar_str(&locale.ctype)?;
    let utf8 = s.scalar_lgl(RLogical::from_option(Some(locale.utf8)))?;
    let out = crate::ops::common::list(s, &[ctype, utf8])?;
    Ok(out)
}

fn sys_which(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let ids = ctx.session().str_slice(x)?.to_vec();
    let windows = ctx
        .host()
        .sys_info()
        .sysname
        .eq_ignore_ascii_case("windows");
    let env_path = ctx.host().env_var("PATH").unwrap_or_default();
    let env_path = String::from_utf8_lossy(&env_path);
    let sep = if windows { ';' } else { ':' };
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let name = text(ctx.session(), id);
        let mut found = None;
        if id != CharId::NA && !name.is_empty() {
            let mut candidates = env_path.split(sep).map(|dir| Path::new(dir).join(&name));
            let is_file = |ctx: &mut Ctx, path: &Path| {
                ctx.host()
                    .metadata(path, true)
                    .is_ok_and(|meta| meta.is_file)
            };
            found = if name.contains(['/', '\\']) {
                is_file(ctx, Path::new(&name)).then(|| PathBuf::from(name))
            } else {
                candidates.find(|path| is_file(ctx, path))
            };
        }
        out.push(match found {
            Some(path) => ch(ctx.session_mut(), &path.to_string_lossy())?,
            None => CharId::BLANK,
        });
    }
    let s = ctx.session_mut();
    let result = s.alloc_str_from(&out)?;
    let labels = names(s, x);
    let labels = if labels.is_empty() {
        s.str_slice(x)?.to_vec()
    } else {
        labels.to_vec()
    };
    set_names(s, result, &labels)?;
    Ok(result)
}

fn isatty(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let v = number(ctx.session_mut(), args.get(0))?;
    let stream = match v {
        0.0 => Some(StdStream::Stdin),
        1.0 => Some(StdStream::Stdout),
        2.0 => Some(StdStream::Stderr),
        _ => None,
    };
    let tty = stream.is_some_and(|s| ctx.host().is_tty(s));
    Ok(ctx
        .session_mut()
        .scalar_lgl(RLogical::from_option(Some(tty)))?)
}

fn interactive(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let value = RLogical::from_option(Some(ctx.is_interactive()));
    Ok(ctx.session_mut().scalar_lgl(value)?)
}

fn readline(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let prompt = string(ctx.session(), args.get(0)).unwrap_or_default();
    ctx.host()
        .write_out(prompt.as_bytes())
        .map_err(|err| error(ctx, call, err.to_string()))?;
    if !ctx.is_interactive() {
        ctx.host()
            .write_out(b"\n")
            .map_err(|err| error(ctx, call, err.to_string()))?;
        return Ok(ctx.session_mut().scalar_str("")?);
    }
    let mut buf = Vec::new();
    ctx.host()
        .flush(StdStream::Stdout)
        .map_err(|err| error(ctx, call, err.to_string()))?;
    ctx.host()
        .read_line(&mut buf)
        .map_err(|err| error(ctx, call, err.to_string()))?;
    vector_limit(ctx, call, buf.len(), 1)?;
    Ok(ctx
        .session_mut()
        .scalar_str(String::from_utf8_lossy(&buf).trim())?)
}

fn proc_time(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let times = ctx.host().cpu_times();
    Ok(ctx.session_mut().alloc_real_from(&[
        times.user_secs,
        times.system_secs,
        times.elapsed_secs,
        0.0,
        0.0,
    ])?)
}

fn time_safepoint(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    ctx.safepoint();
    Ok(SEXP_NIL)
}

fn quit(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let status = number(ctx.session_mut(), args.get(0))?;
    if status.is_nan() {
        return Err(error(ctx, call, "invalid 'status'"));
    }
    let run_last = flag(ctx, args.get(1), true)?;
    Err(Unwind::Quit {
        status: status as i32,
        run_last,
    })
}

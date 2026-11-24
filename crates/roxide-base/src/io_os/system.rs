use std::time::Duration;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_host::{CommandSpec, HostErrorKind, ProcessOutput};
use roxide_value::{Encoding, RLogical, SEXP_NIL, SexpType, ValueApi};

use super::{attr, bytes, check_limit, flag, logical, string, strings, text, texts};

pub(super) fn getenv(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let keys = args.get(0);
    let (unset, _) = ctx.session_mut().coerce(args.get(1), SexpType::Str)?;
    let unset = texts(ctx, call, unset, "unset")?;
    let [unset] = unset.as_slice() else {
        return Err(ctx.stop(call.err_call(), "invalid 'unset' argument"));
    };
    let named = logical(ctx, args.get(2))?;
    if keys == SEXP_NIL || ctx.session().xlength(keys)? == 0 {
        let mut entries = ctx.host().env_vars();
        entries.sort();
        if !named.unwrap_or(keys == SEXP_NIL) {
            return strings(
                ctx,
                entries.into_iter().map(|(mut k, v)| {
                    k.push(b'=');
                    k.extend(v);
                    Some(k)
                }),
                Encoding::Native,
            );
        }
        let (labels, values): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        let result = strings(ctx, values.into_iter().map(Some), Encoding::Native)?;
        let names = strings(ctx, labels.into_iter().map(Some), Encoding::Native)?;
        attr(ctx, result, "names", names)?;
        if keys == SEXP_NIL {
            let class = string(ctx, "Dlist")?;
            attr(ctx, result, "class", class)?;
        }
        return Ok(result);
    }

    let names = texts(ctx, call, keys, "x")?;
    let values: Vec<_> = names
        .iter()
        .map(|name| {
            ctx.host()
                .env_var(name.as_deref().unwrap_or("NA"))
                .or_else(|| unset.as_ref().map(|value| value.as_bytes().to_vec()))
        })
        .collect();
    let result = strings(ctx, values, Encoding::Native)?;
    if named.unwrap_or(names.len() > 1) {
        attr(ctx, result, "names", keys)?;
    }
    Ok(result)
}

pub(super) fn setenv(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let list = args.get(0);
    let key = ctx.session_mut().intern("names")?;
    let names = ctx.session().attr(list, key);
    let values = ctx.session().list_slice(list)?.to_vec();
    if values.is_empty() {
        return Ok(ctx.session_mut().alloc_lgl_from(&[])?);
    }
    let names = texts(ctx, call, names, "names")?;
    if names.len() != values.len()
        || names
            .iter()
            .any(|k| k.as_ref().is_none_or(|k| k.is_empty()))
    {
        return Err(ctx.stop(call.err_call(), "all arguments must be named"));
    }
    let mut pending = Vec::with_capacity(names.len());
    for (name, value) in names.into_iter().flatten().zip(values) {
        let (value, _) = ctx.session_mut().coerce(value, SexpType::Str)?;
        if ctx.session().xlength(value)? != 1 {
            return Err(ctx.stop(
                call.err_call(),
                "'names' and 'val' are of different lengths",
            ));
        }
        let mut value = bytes(ctx, call, value)?;
        pending.push((name, value.pop().flatten()));
    }
    let mut result = Vec::with_capacity(pending.len());
    for (name, value) in pending {
        let value = value.as_deref().unwrap_or(b"NA");
        let ok = valid_key(&name) && ctx.host().set_env_var(&name, Some(value)).is_ok();
        result.push(RLogical::from_option(Some(ok)));
    }
    Ok(ctx.session_mut().alloc_lgl_from(&result)?)
}

pub(super) fn unsetenv(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let names = texts(ctx, call, args.get(0), "x")?;
    let mut result = Vec::with_capacity(names.len());
    for name in names {
        let name = name.as_deref().unwrap_or("NA");
        let ok = valid_key(name) && ctx.host().set_env_var(name, None).is_ok();
        result.push(RLogical::from_option(Some(ok)));
    }
    Ok(ctx.session_mut().alloc_lgl_from(&result)?)
}

fn valid_key(key: &str) -> bool {
    !key.is_empty() && !key.contains(['=', '\0'])
}

enum Redirect {
    Console,
    Capture,
    Null,
    File(String),
}

impl Redirect {
    fn parse(ctx: &mut Ctx, call: &Call, value: roxide_gc::SexpId, arg: &str) -> EvalResult<Self> {
        if ctx.session().type_of(value)? == SexpType::Str {
            let path = text(ctx, call, value, arg)?;
            return Ok(if path.is_empty() {
                Self::Console
            } else {
                Self::File(path)
            });
        }
        Ok(if flag(ctx, call, value, arg)? {
            Self::Capture
        } else {
            Self::Null
        })
    }

    fn apply(&self, script: &mut String, fd: u8, wait: bool) {
        let redirect = match self {
            Self::Console | Self::Capture if wait => return,
            Self::Console | Self::Capture | Self::Null => "/dev/null".to_owned(),
            Self::File(path) => quote(path),
        };
        script.push_str(&format!(" {fd}>{redirect}"));
    }
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub(super) fn system(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    if ctx.session().type_of(value)? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "non-empty character argument expected"));
    }
    let command = texts(ctx, call, value, "command")?;
    let command = command
        .first()
        .map(|v| v.as_deref().unwrap_or("NA"))
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ctx.stop(call.err_call(), "non-empty character argument expected"))?;
    let capture = flag(ctx, call, args.get(1), "intern")?;
    let ignore_stdout = flag(ctx, call, args.get(2), "ignore.stdout")?;
    let ignore_stderr = flag(ctx, call, args.get(3), "ignore.stderr")?;
    let wait = flag(ctx, call, args.get(4), "wait")? || capture;
    let timeout = super::integer(ctx, args.get(6))?;
    if timeout < 0 {
        return Err(ctx.stop(call.err_call(), "invalid 'timeout' argument"));
    }
    if !wait && timeout != 0 {
        return Err(ctx.stop(
            call.err_call(),
            "Timeout with background running processes is not supported.",
        ));
    }
    let input = args.get(5);
    if input != SEXP_NIL && ctx.session().type_of(input)? != SexpType::Str {
        return Err(ctx.stop(
            call.err_call(),
            "'input' must be a character vector or 'NULL'",
        ));
    }
    let stdin = shell_input(ctx, call, input)?;
    let mut script = command.to_owned();
    if ignore_stdout {
        script.push_str(" >/dev/null");
    }
    if ignore_stderr {
        script.push_str(" 2>/dev/null");
    }
    let script = if wait {
        script
    } else {
        format!("exec 3<&0\n(\n{script}\n) <&3 3<&- >/dev/null 2>&1 &")
    };
    let spec = CommandSpec {
        program: "sh".to_owned(),
        args: vec!["-c".to_owned(), script],
        stdin,
        timeout: (timeout > 0).then(|| Duration::from_secs(timeout as u64)),
    };
    let (output, timed_out) = match ctx.host().spawn(&spec) {
        Ok(output) => (output, false),
        Err(err) if err.kind == HostErrorKind::TimedOut => {
            ctx.warn_now(
                call.err_call(),
                format!("command '{}' timed out after {timeout}s", spec.args[1]),
            )?;
            (
                ProcessOutput {
                    status: 124,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                },
                true,
            )
        }
        Err(err) => return Err(ctx.stop(call.err_call(), err.to_string())),
    };
    ctx.host()
        .write_err(&output.stderr)
        .map_err(|err| ctx.stop(call.err_call(), err.to_string()))?;
    if output.status == 127 {
        if capture {
            return Err(ctx.stop(call.err_call(), "error in running command"));
        }
        ctx.warn_now(call.err_call(), "error in running command")?;
    }
    if !capture {
        ctx.host()
            .write_out(&output.stdout)
            .map_err(|err| ctx.stop(call.err_call(), err.to_string()))?;
        ctx.set_visible(false);
        return Ok(ctx.session_mut().scalar_int(output.status)?);
    }
    let result = captured_lines(ctx, call, &output.stdout)?;
    if output.status != 0 {
        ctx.protect(result);
        let status = ctx.session_mut().scalar_int(output.status)?;
        attr(ctx, result, "status", status)?;
        if !timed_out {
            ctx.warn_now(
                call.err_call(),
                format!(
                    "running command '{}' had status {}",
                    spec.args[1], output.status
                ),
            )?;
        }
    }
    ctx.set_visible(true);
    Ok(result)
}

fn shell_input(ctx: &mut Ctx, call: &Call, input: SexpId) -> EvalResult<Option<Vec<u8>>> {
    if input == SEXP_NIL {
        return Ok(None);
    }
    let mut data = Vec::new();
    for line in bytes(ctx, call, input)? {
        let line = line.as_deref().unwrap_or(b"NA");
        check_limit(
            ctx,
            call,
            data.len().saturating_add(line.len()).saturating_add(1),
        )?;
        data.extend_from_slice(line);
        data.push(b'\n');
    }
    Ok(Some(data))
}

pub(super) fn system2(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let program = text(ctx, call, args.get(0), "command")?;
    let argv = texts(ctx, call, args.get(1), "args")?;
    let mut stdout = Redirect::parse(ctx, call, args.get(2), "stdout")?;
    let stderr = Redirect::parse(ctx, call, args.get(3), "stderr")?;
    let stdin = text(ctx, call, args.get(4), "stdin")?;
    let input = args.get(5);
    let env = texts(ctx, call, args.get(6), "env")?;
    let wait = flag(ctx, call, args.get(7), "wait")?;
    let (timeout, _) = ctx.session_mut().coerce(args.get(8), SexpType::Real)?;
    let timeout = ctx.session().real(timeout)?.iter().next().unwrap_or(0.0);
    let timeout = Duration::try_from_secs_f64(timeout)
        .map_err(|_| ctx.stop(call.err_call(), "invalid 'timeout' argument"))?;
    if matches!(stderr, Redirect::Capture) {
        if !matches!(stdout, Redirect::Capture) {
            ctx.warn(call.err_call(), "setting stdout = TRUE");
        }
        stdout = Redirect::Capture;
    }
    let capture = matches!(stdout, Redirect::Capture);
    let wait = wait || capture;
    let mut parts: Vec<_> = env.into_iter().flatten().collect();
    parts.push(quote(&program));
    parts.extend(
        argv.into_iter()
            .map(|value| value.unwrap_or_else(|| "NA".to_owned())),
    );
    let mut script = parts.join(" ");
    stdout.apply(&mut script, 1, wait);
    if matches!(stderr, Redirect::Capture) {
        script.push_str(" 2>&1");
    } else {
        stderr.apply(&mut script, 2, wait);
    }
    let input = shell_input(ctx, call, input)?;
    if input.is_none() && !stdin.is_empty() {
        script.push_str(&format!(" <{}", quote(&stdin)));
    }
    if !wait {
        script.push_str(" &");
    }
    let spec = CommandSpec {
        program: "sh".to_owned(),
        args: vec!["-c".to_owned(), script],
        stdin: input,
        timeout: (!timeout.is_zero()).then_some(timeout),
    };
    let output = ctx
        .host()
        .spawn(&spec)
        .map_err(|err| ctx.stop(call.err_call(), err.to_string()))?;
    if matches!(stderr, Redirect::Console) {
        ctx.host()
            .write_err(&output.stderr)
            .map_err(|err| ctx.stop(call.err_call(), err.to_string()))?;
    }
    if !capture {
        if matches!(stdout, Redirect::Console) {
            ctx.host()
                .write_out(&output.stdout)
                .map_err(|err| ctx.stop(call.err_call(), err.to_string()))?;
        }
        ctx.set_visible(false);
        return Ok(ctx.session_mut().scalar_int(output.status)?);
    }
    let result = captured_lines(ctx, call, &output.stdout)?;
    if output.status != 0 {
        let status = ctx.session_mut().scalar_int(output.status)?;
        attr(ctx, result, "status", status)?;
        ctx.warn(
            call.err_call(),
            format!("running command '{program}' had status {}", output.status),
        );
    }
    Ok(result)
}

fn captured_lines(ctx: &mut Ctx, call: &Call, output: &[u8]) -> EvalResult {
    check_limit(ctx, call, output.len())?;
    let lines = output.split_inclusive(|&ch| ch == b'\n').map(|line| {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        Some(line.strip_suffix(b"\r").unwrap_or(line))
    });
    strings(ctx, lines, Encoding::Native)
}

pub(super) fn versions(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let version = string(ctx, zstd::zstd_safe::version_string())?;
    let name = string(ctx, "zstd")?;
    attr(ctx, version, "names", name)?;
    Ok(version)
}

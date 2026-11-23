use roxide_eval::{Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    BASE_ENV, CharId, Encoding, FALSE_SCALAR, SEXP_NIL, SexpType, TRUE_SCALAR, ValueApi,
};

/// Invoke a base function with evaluated arguments.
pub fn base_call(ctx: &mut Ctx, call: &Call, name: &str, args: &[SexpId]) -> EvalResult {
    ctx.scoped(|ctx| {
        for &arg in args {
            ctx.protect(arg);
        }
        let sym = ctx.session_mut().intern(name)?;
        let fun = ctx.find_fun(sym, BASE_ENV)?;
        let args = args.iter().map(|&arg| (None, arg)).collect::<Vec<_>>();
        ctx.apply(fun, &args, call.err_call(), call.env)
    })
}

/// Decode a character vector. Preserve missing elements.
pub fn r_strings(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<Option<String>>> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Str)?;
    let limit = ctx.host().limits().max_vector_bytes;
    let s = ctx.session();
    let values = s.str_slice(value)?;
    let mut used = values.len().saturating_mul(size_of::<Option<String>>());
    if used > limit {
        return Err(ctx.stop(SEXP_NIL, "vector size limit exceeded"));
    }
    let mut strings = Vec::with_capacity(values.len());
    for &id in values {
        if id == CharId::NA {
            strings.push(None);
            continue;
        }
        let bytes = s.char_bytes(id)?;
        let decoded: String = if s.char_encoding(id)? == Encoding::Latin1 {
            bytes.iter().map(|&b| char::from(b)).collect()
        } else {
            String::from_utf8_lossy(bytes).into_owned()
        };
        used = used.saturating_add(decoded.len());
        if used > limit {
            return Err(ctx.stop(SEXP_NIL, "vector size limit exceeded"));
        }
        strings.push(Some(decoded));
    }
    Ok(strings)
}

pub(super) fn string(ctx: &mut Ctx, call: &Call, value: SexpId, name: &str) -> EvalResult<String> {
    if ctx.session().type_of(value)? == SexpType::Str
        && let [Some(value)] = r_strings(ctx, value)?.as_mut_slice()
    {
        return Ok(std::mem::take(value));
    }
    Err(ctx.stop(call.err_call(), format!("invalid '{name}' argument")))
}

/// Physical input options for a text scan.
#[derive(Clone, Copy)]
pub struct InputOptions<'a> {
    /// Number of physical lines to discard before parsing.
    pub skip: usize,
    /// Declared character encoding of a file or connection.
    pub encoding: &'a str,
    /// Skip embedded NUL bytes.
    pub skip_nul: bool,
}

/// Read bounded text from an R character vector, path, or connection.
pub(super) fn input_text(
    ctx: &mut Ctx,
    call: &Call,
    file: SexpId,
    text: SexpId,
    options: InputOptions<'_>,
) -> EvalResult<String> {
    ctx.scoped(|ctx| {
        let inline = text != SEXP_NIL;
        let value = if inline {
            text
        } else {
            read_lines(ctx, call, file, -1, options)?
        };
        ctx.protect(value);
        let limit = ctx.host().limits().max_vector_bytes;
        let mut input = String::new();
        let s = ctx.session();
        for &id in s.str_slice(value)? {
            let bytes = if id == CharId::NA {
                b"NA"
            } else {
                s.char_bytes(id)?
            };
            if bytes.len().saturating_add(input.len()).saturating_add(1) > limit {
                return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
            }
            if s.char_encoding(id)? == Encoding::Latin1 {
                input.extend(bytes.iter().map(|&b| char::from(b)));
            } else {
                input.push_str(&String::from_utf8_lossy(bytes));
            }
            input.push('\n');
            if input.len() > limit {
                return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
            }
        }
        let input = input.strip_prefix('\u{feff}').unwrap_or(&input);
        let input = if input.contains('\r') {
            input.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            input.to_owned()
        };
        let offset = if options.skip == 0 {
            0
        } else {
            input
                .match_indices('\n')
                .nth(options.skip - 1)
                .map_or(input.len(), |(i, _)| i + 1)
        };
        Ok(input[offset..].to_owned())
    })
}

fn read_lines(
    ctx: &mut Ctx,
    call: &Call,
    file: SexpId,
    count: i32,
    options: InputOptions<'_>,
) -> EvalResult {
    ctx.scoped(|ctx| {
        let n = ctx.session_mut().scalar_int(count)?;
        ctx.protect(n);
        let encoding = match options.encoding {
            "latin1" | "Latin-1" | "ISO-8859-1" => "latin1",
            "UTF-8" | "UTF-8-BOM" => "UTF-8",
            _ => "unknown",
        };
        let enc = ctx.session_mut().scalar_str(encoding)?;
        let skip = if options.skip_nul {
            TRUE_SCALAR
        } else {
            FALSE_SCALAR
        };
        base_call(
            ctx,
            call,
            "readLines",
            &[file, n, TRUE_SCALAR, FALSE_SCALAR, enc, skip],
        )
    })
}

pub(super) struct ConnectionLines<'a> {
    file: SexpId,
    id: i32,
    options: InputOptions<'a>,
}

#[derive(Default)]
struct Remainders(std::collections::HashMap<i32, String>);

impl roxide_gc::Trace for Remainders {
    fn trace(&self, _: &mut roxide_gc::Tracer<'_>) {}
}

impl roxide_eval::CtxExt for Remainders {}

impl<'a> ConnectionLines<'a> {
    pub(super) fn open(
        ctx: &mut Ctx,
        call: &Call,
        file: SexpId,
        options: InputOptions<'a>,
    ) -> EvalResult<Option<Self>> {
        if !crate::io_os::connection::is_connection(ctx, file)? {
            return Ok(None);
        }
        let open = base_call(ctx, call, "isOpen", &[file])?;
        let id = ctx.session().int(file)?.get(0);
        if ctx.session().lgl(open)?.get(0).get() != Some(true) {
            return Ok(None);
        }
        Ok(Some(Self { file, id, options }))
    }

    pub(super) fn take_remainder(&self, ctx: &mut Ctx) -> String {
        ctx.ext::<Remainders>()
            .0
            .remove(&self.id)
            .unwrap_or_default()
    }

    pub(super) fn save_remainder(&self, ctx: &mut Ctx, rest: &str) {
        let limit = ctx.host().limits().max_vector_bytes;
        let remainders = &mut ctx.ext::<Remainders>().0;
        let used: usize = remainders.values().map(String::len).sum();
        if !rest.is_empty() && used.saturating_add(rest.len()) <= limit {
            remainders.insert(self.id, rest.into());
        }
    }

    pub(super) fn next(&self, ctx: &mut Ctx, call: &Call) -> EvalResult<Option<String>> {
        let line = read_lines(ctx, call, self.file, 1, self.options)?;
        let mut strings = r_strings(ctx, line)?;
        Ok(strings.pop().map(|line| {
            let mut text = line.unwrap_or_else(|| String::from("NA"));
            text.push('\n');
            text
        }))
    }
}

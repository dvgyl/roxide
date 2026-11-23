use roxide_base::scan::{base_call, r_strings};
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, SEXP_MISSING, SEXP_NIL, SexpType, TRUE_SCALAR, ValueApi};

use super::{flag, strings};

pub(super) fn capture(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let file = args.get(0);
    let append = flag(ctx, call, args.get(1))?;
    let kind = args.get(2);
    if kind != SEXP_NIL && ctx.session().type_of(kind)? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "'arg' must be NULL or a character vector"));
    }
    let kinds = r_strings(ctx, kind)?;
    let messages = match kinds.as_slice() {
        [] if kind == SEXP_NIL => false,
        [Some(out), Some(msg)] if out == "output" && msg == "message" => false,
        kinds if kinds.len() != 1 => {
            return Err(ctx.stop(call.err_call(), "'arg' must be of length 1"));
        }
        [Some(kind)] if !kind.is_empty() && "output".starts_with(kind) => false,
        [Some(kind)] if !kind.is_empty() && "message".starts_with(kind) => true,
        _ => {
            return Err(ctx.stop(
                call.err_call(),
                "'arg' should be one of “output”, “message”",
            ));
        }
    };
    let split = flag(ctx, call, args.get(3))?;
    if split && messages {
        return Err(ctx.stop(call.err_call(), "cannot split the message connection"));
    }

    let ty = ctx.session().type_of(file)?;
    let mut owned = ty == SexpType::Str;
    let dest = if owned {
        let mode = ctx
            .session_mut()
            .scalar_str(if append { "at" } else { "wt" })?;
        base_call(ctx, call, "file", &[file, mode])?
    } else if file == SEXP_NIL {
        file
    } else {
        let s = ctx.session_mut();
        let class = s.intern("class")?;
        let class = s.attr(file, class);
        if ty != SexpType::Int
            || !s.str_slice(class).is_ok_and(|classes| {
                classes
                    .iter()
                    .any(|&id| s.char_bytes(id).ok() == Some(b"connection"))
            })
        {
            return Err(ctx.stop(
                call.err_call(),
                "'file' must be NULL, a character string or a connection",
            ));
        }
        let open = base_call(ctx, call, "isOpen", &[file])?;
        if flag(ctx, call, open)? {
            let write = ctx.session_mut().scalar_str("write")?;
            let writable = base_call(ctx, call, "isOpen", &[file, write])?;
            if !flag(ctx, call, writable)? {
                let reason = if ctx.session().int(file)?.iter().next() == Some(0) {
                    "cannot switch output to stdin"
                } else {
                    "cannot write to this connection"
                };
                return Err(ctx.stop(call.err_call(), reason));
            }
        } else {
            let mode = ctx
                .session_mut()
                .scalar_str(if append { "at" } else { "wt" })?;
            base_call(ctx, call, "open", &[file, mode])?;
            owned = true;
        }
        file
    };
    ctx.protect(dest);
    let result = collect(ctx, call, args.get(4), dest, messages, split);
    if owned {
        ctx.protect_eval_result(&result);
        let closed = base_call(ctx, call, "close", &[dest]);
        result.and_then(|value| closed.map(|_| value))
    } else {
        result
    }
    .inspect(|_| ctx.set_visible(file == SEXP_NIL))
}

fn collect(
    ctx: &mut Ctx,
    call: &Call,
    caller: SexpId,
    dest: SexpId,
    messages: bool,
    split: bool,
) -> EvalResult {
    let dots = ctx.session_mut().intern("...")?;
    let dots = ctx
        .session()
        .env_get(call.env, dots)
        .map_or(SEXP_NIL, |binding| binding.value);
    ctx.protect(dots);
    let print_call = Call {
        env: caller,
        ..*call
    };
    let (result, data) = ctx.capture_streams(!messages, messages, split, |ctx| {
        let result = ctx.scoped(|ctx| {
            let mut pending = dots;
            while pending != SEXP_NIL && pending != SEXP_MISSING {
                let (expr, next, _) = ctx.session().cons_parts(pending)?;
                ctx.set_visible(true);
                let value = ctx.eval(expr, call.env)?;
                if ctx.is_visible() {
                    base_call(ctx, &print_call, "print", &[value])?;
                }
                pending = next;
            }
            Ok(SEXP_NIL)
        });
        Ok(result)
    })?;
    if dest != SEXP_NIL {
        ctx.protect_eval_result(&result);
        let s = ctx.session_mut();
        let text = s.mk_char(&data, Encoding::Bytes)?;
        let payload = s.alloc_str_from(&[text])?;
        ctx.protect(payload);
        let separator = ctx.session_mut().scalar_str("")?;
        let written = base_call(
            ctx,
            call,
            "writeLines",
            &[payload, dest, separator, TRUE_SCALAR],
        );
        result.and(written)?;
        return Ok(SEXP_NIL);
    }
    result?;
    let lines = data.split_inclusive(|&byte| byte == b'\n').map(|line| {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        line.strip_suffix(b"\r").unwrap_or(line)
    });
    if lines.clone().count().saturating_mul(size_of::<CharId>())
        > ctx.host().limits().max_vector_bytes
    {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    strings(ctx.session_mut(), lines)
}

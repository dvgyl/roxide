use std::io::Cursor;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use super::super::{filesystem, flag, io_result, text};
use super::{
    ConnTable, Connection, FilePath, Mode, check_slot, close_connection, finish, id, is_connection,
    register, take, with_open_stream, write_to,
};

#[derive(Default)]
pub(super) struct Sinks {
    output: Vec<OutputSink>,
    message: Option<i32>,
}

#[derive(Clone, Copy)]
struct OutputSink {
    id: i32,
    close: SinkClose,
}

#[derive(Clone, Copy)]
enum SinkClose {
    Keep,
    Stream(Mode),
    Connection,
}

/// Return the output sink depth or the active message connection number.
pub(crate) fn sink_number(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let message = flag(ctx, call, args.get(0), "type")?;
    let table = ctx.ext::<ConnTable>();
    let number = if message {
        table.sinks.message.unwrap_or(2)
    } else {
        table.sinks.output.len() as i32
    };
    Ok(ctx.session_mut().scalar_int(number)?)
}

/// Set or remove a persistent output diversion.
pub(crate) fn sink(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let file = args.get(0);
    let append = flag(ctx, call, args.get(1), "append")?;
    let message = flag(ctx, call, args.get(2), "type")?;
    let split = flag(ctx, call, args.get(3), "split")?;
    if message {
        set_message_sink(ctx, call, file, split)?;
    } else if file == SEXP_NIL {
        pop_output_sink(ctx, call)?;
    } else {
        push_output_sink(ctx, call, file, append, split)?;
    }
    Ok(SEXP_NIL)
}

fn set_message_sink(ctx: &mut Ctx, call: &Call, file: SexpId, split: bool) -> EvalResult<()> {
    if split {
        return Err(ctx.stop(call.err_call(), "cannot split the message connection"));
    }
    let invalid = "'file' must be NULL or an already open connection";
    let next = if file == SEXP_NIL {
        2
    } else {
        if !is_connection(ctx, file)? {
            return Err(ctx.stop(call.err_call(), invalid));
        }
        let id = id(ctx, call, file)?;
        if id > 2 {
            let Some(connection) = ctx.ext::<ConnTable>().entries.get(&id) else {
                return Err(ctx.stop(call.err_call(), "invalid connection"));
            };
            if connection.stream.is_none() {
                return Err(ctx.stop(call.err_call(), invalid));
            }
            if !connection.mode.flags.write {
                return Err(ctx.stop(call.err_call(), "cannot write to this connection"));
            }
        } else if id != 1 && id != 2 {
            return Err(ctx.stop(call.err_call(), "cannot write to this connection"));
        }
        id
    };
    if ctx.ext::<ConnTable>().sinks.message.take().is_some() {
        ctx.pop_stream_sink(true)?;
    }
    if next != 2 {
        ctx.push_stream_sink(false, true, false, move |ctx, bytes| {
            write_sink(ctx, next, bytes)
        });
        ctx.ext::<ConnTable>().sinks.message = Some(next);
    }
    Ok(())
}

fn push_output_sink(
    ctx: &mut Ctx,
    call: &Call,
    file: SexpId,
    append: bool,
    split: bool,
) -> EvalResult<()> {
    let path = ctx.session().type_of(file)? == SexpType::Str;
    let id = if path {
        None
    } else {
        let id = id(ctx, call, file).map_err(|_| {
            ctx.stop(
                call.err_call(),
                "'file' must be NULL, a connection or a character string",
            )
        })?;
        if id == 0 {
            return Err(ctx.stop(call.err_call(), "cannot switch output to stdin"));
        }
        let table = ctx.ext::<ConnTable>();
        if id == 1 || table.sinks.output.last().is_some_and(|sink| sink.id == id) {
            return Ok(());
        }
        Some(id)
    };
    if ctx.ext::<ConnTable>().sinks.output.len() >= 19 {
        return Err(ctx.stop(call.err_call(), "sink stack is full"));
    }
    let (id, close) = if let Some(id) = id {
        let close = if id == 2 {
            SinkClose::Keep
        } else {
            let Some(connection) = ctx.ext::<ConnTable>().entries.get(&id) else {
                return Err(ctx.stop(call.err_call(), "invalid connection"));
            };
            if connection.stream.is_some() {
                if !connection.mode.flags.write {
                    return Err(ctx.stop(call.err_call(), "cannot write to this connection"));
                }
                SinkClose::Keep
            } else {
                let previous = connection.mode;
                let path = connection
                    .path
                    .clone()
                    .ok_or_else(|| ctx.stop(call.err_call(), "invalid connection"))?;
                let mode = Mode::write(false);
                let stream = path.open(ctx, call, mode)?;
                let mut connection = take(ctx, call.err_call(), id)?;
                connection.mode = mode;
                connection.stream = Some(stream);
                ctx.ext::<ConnTable>().entries.insert(id, connection);
                SinkClose::Stream(previous)
            }
        };
        (id, close)
    } else {
        check_slot(ctx, call)?;
        let mut mode = Mode::write(false);
        mode.flags.append = append;
        mode.flags.truncate = !append;
        let description = text(ctx, call, file, "file")?;
        let path = FilePath {
            name: filesystem::expand(ctx, &description),
            format: None,
            level: 0,
            detect: false,
            temporary: false,
        };
        let stream = path.open(ctx, call, mode)?;
        register(
            ctx,
            call,
            "file",
            Connection {
                description,
                path: Some(path),
                mode,
                stream: Some(stream),
                pushback: Cursor::default(),
            },
        )?;
        (ctx.ext::<ConnTable>().last_id, SinkClose::Connection)
    };
    ctx.push_stream_sink(true, false, split, move |ctx, bytes| {
        write_sink(ctx, id, bytes)
    });
    ctx.ext::<ConnTable>()
        .sinks
        .output
        .push(OutputSink { id, close });
    Ok(())
}

fn pop_output_sink(ctx: &mut Ctx, call: &Call) -> EvalResult<()> {
    let Some(sink) = ctx.ext::<ConnTable>().sinks.output.pop() else {
        return ctx.warn_now(call.err_call(), "no sink to remove");
    };
    let flushed = ctx.pop_stream_sink(false);
    let closed = match sink.close {
        SinkClose::Connection => close_connection(ctx, call, sink.id),
        SinkClose::Stream(mode) => {
            let table = ctx.ext::<ConnTable>();
            let stream = table.entries.get_mut(&sink.id).and_then(|con| {
                con.mode = mode;
                con.stream.take()
            });
            let result = stream.map_or(Ok(()), |stream| finish(ctx, stream));
            io_result(ctx, call, result)
        }
        SinkClose::Keep => Ok(()),
    };
    flushed.and(closed)
}

fn write_sink(ctx: &mut Ctx, id: i32, bytes: &[u8]) -> EvalResult<()> {
    let result = match id {
        1 => ctx.host().write_out(bytes),
        2 => ctx.host().write_err(bytes),
        _ => {
            return with_open_stream(ctx, SEXP_NIL, id, |ctx, stream, _| {
                write_to(ctx, stream, bytes)
            });
        }
    };
    result.map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))
}

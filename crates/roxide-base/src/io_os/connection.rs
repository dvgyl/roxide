use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Cursor, Read, Seek, Write};
use std::path::PathBuf;

use roxide_eval::{Args, Call, Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_host::{HostFile, OpenMode};
use roxide_value::{Encoding, RSession, SEXP_NIL, SexpType, Sym, ValueApi};

use super::codec::{self, Compression};
use super::{
    ByteEncoding, attr, bytes, check_limit, flag, integer, io_result, strings, translated_bytes,
};

#[derive(Default)]
struct ConnTable {
    entries: BTreeMap<i32, Connection>,
    stdin: Cursor<Vec<u8>>,
    last_id: i32,
    output_sinks: Vec<OutputSink>,
    message_sink: Option<i32>,
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

impl Trace for ConnTable {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for connection in self.entries.values() {
            if let Some(Stream::TextWrite(out)) = &connection.stream {
                tracer.edge(out.env);
            }
        }
    }
}

impl CtxExt for ConnTable {}

struct Connection {
    description: String,
    path: Option<FilePath>,
    mode: Mode,
    stream: Option<Stream>,
    pushback: Cursor<Vec<u8>>,
}

#[derive(Clone)]
struct FilePath {
    name: PathBuf,
    format: Option<Compression>,
    level: u32,
    detect: bool,
    temporary: bool,
}

#[derive(Clone, Copy)]
struct Mode {
    flags: OpenMode,
    binary: bool,
}

impl Mode {
    fn parse(mode: &str) -> Option<Self> {
        let mut chars = mode.chars();
        let primary = chars.next()?;
        let mut binary = false;
        let mut text = false;
        let mut plus = false;
        for ch in chars {
            match ch {
                'b' if !text && !binary => binary = true,
                't' if !text && !binary => text = true,
                '+' if !plus => plus = true,
                _ => return None,
            }
        }
        let (read, truncate, append) = match primary {
            'r' => (true, false, false),
            'w' => (false, true, false),
            'a' => (false, false, true),
            _ => return None,
        };
        Some(Self {
            flags: OpenMode {
                read: read || plus,
                write: !read || plus,
                create: !read,
                truncate,
                append,
            },
            binary,
        })
    }

    fn read(binary: bool) -> Self {
        Self {
            flags: OpenMode::read_only(),
            binary,
        }
    }

    fn write(binary: bool) -> Self {
        Self {
            flags: OpenMode::write_truncate(),
            binary,
        }
    }
}

enum Stream {
    File(BufReader<Box<dyn HostFile>>, Option<u64>),
    ReadMemory(Cursor<Vec<u8>>),
    Raw(Cursor<Vec<u8>>),
    CompressedRead(BufReader<codec::LimitedReader>),
    CompressedWrite(Box<codec::Writer>),
    TextWrite(TextOutput),
}

struct TextOutput {
    env: SexpId,
    symbol: Sym,
    lines: Vec<Option<Vec<u8>>>,
    partial: Vec<u8>,
    size: usize,
    encoding: Encoding,
}

impl TextOutput {
    fn value(&self, ctx: &mut Ctx) -> EvalResult {
        strings(ctx, self.lines.iter().map(Option::as_deref), self.encoding)
    }

    fn update(&self, ctx: &mut Ctx) -> io::Result<()> {
        let value = self
            .value(ctx)
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        ctx.session_mut()
            .env_define(self.env, self.symbol, value)
            .map_err(io::Error::other)
    }

    fn write(&mut self, ctx: &mut Ctx, data: &[u8]) -> io::Result<()> {
        let completed = data.iter().filter(|&&byte| byte == b'\n').count();
        let payload = self.size.saturating_add(data.len() - completed);
        let partial = if let Some(&last) = data.last() {
            last != b'\n'
        } else {
            !self.partial.is_empty()
        };
        let count = self
            .lines
            .len()
            .saturating_add(completed)
            .saturating_add(usize::from(partial));
        check_buffer(
            ctx,
            payload.saturating_add(count.saturating_mul(size_of::<Option<Vec<u8>>>())),
        )?;
        self.size = payload;
        for part in data.split_inclusive(|&b| b == b'\n') {
            if let Some(line) = part.strip_suffix(b"\n") {
                self.partial.extend_from_slice(line);
                self.lines.push(Some(std::mem::take(&mut self.partial)));
            } else {
                self.partial.extend_from_slice(part);
            }
        }
        self.update(ctx)
    }
}

impl FilePath {
    fn open(&self, ctx: &mut Ctx, call: &Call, mode: Mode) -> EvalResult<Stream> {
        let result = ctx.host().open(&self.name, mode.flags);
        let file = match result {
            Ok(file) => file,
            Err(error) => {
                ctx.warn(
                    call.err_call(),
                    format!("cannot open file '{}': {error}", self.name.display()),
                );
                return Err(ctx.stop(call.err_call(), "cannot open the connection"));
            }
        };
        if let Some(format) = self.format
            && mode.flags.write
        {
            let writer = io_result(ctx, call, codec::Writer::new(format, file, self.level))?;
            return Ok(Stream::CompressedWrite(Box::new(writer)));
        }
        let mut file = BufReader::new(file);
        if mode.flags.read
            && !mode.flags.write
            && ((self.detect && !mode.binary) || self.format.is_some())
        {
            let header = file.fill_buf();
            let header = io_result(ctx, call, header)?;
            if let Some(format) = Compression::detect(header) {
                let limit = ctx.host().limits().max_decompress_bytes;
                let reader = io_result(ctx, call, codec::reader(format, file, limit))?;
                return Ok(Stream::CompressedRead(reader));
            }
        }
        let write_position = (mode.flags.read && mode.flags.write).then_some(0);
        Ok(Stream::File(file, write_position))
    }
}

fn handle(ctx: &mut Ctx, kind: &str, id: i32) -> EvalResult {
    let value = ctx.session_mut().scalar_int(id)?;
    let class = if kind == "connection" {
        strings(ctx, [Some(kind)], Encoding::Native)?
    } else {
        strings(ctx, [Some(kind), Some("connection")], Encoding::Native)?
    };
    attr(ctx, value, "class", class)?;
    Ok(value)
}

fn check_slot(ctx: &mut Ctx, call: &Call) -> EvalResult<()> {
    let table = ctx.ext::<ConnTable>();
    if table.entries.len() >= 125 || table.last_id == i32::MAX {
        return Err(ctx.stop(call.err_call(), "all 128 connections are in use"));
    }
    Ok(())
}

fn register(ctx: &mut Ctx, call: &Call, kind: &str, connection: Connection) -> EvalResult {
    check_slot(ctx, call)?;
    let next = ctx.ext::<ConnTable>().last_id.max(2) + 1;
    let value = handle(ctx, kind, next)?;
    let table = ctx.ext::<ConnTable>();
    table.last_id = next;
    table.entries.insert(next, connection);
    Ok(value)
}

pub(crate) fn is_connection(ctx: &mut Ctx, value: SexpId) -> EvalResult<bool> {
    let key = ctx.session_mut().intern("class")?;
    let class = ctx.session().attr(value, key);
    Ok(ctx.session().str_slice(class).is_ok_and(|ids| {
        ids.iter().any(|&id| {
            ctx.session()
                .char_bytes(id)
                .is_ok_and(|b| b == b"connection")
        })
    }))
}

fn id(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<i32> {
    if !is_connection(ctx, value)? {
        return Err(ctx.stop(call.err_call(), "'con' is not a connection"));
    }
    Ok(ctx.session().int(value)?.iter().next().unwrap_or(-1))
}

fn take(ctx: &mut Ctx, call: &Call, id: i32) -> EvalResult<Connection> {
    let connection = ctx
        .ext::<ConnTable>()
        .entries
        .remove(&id)
        .ok_or_else(|| ctx.stop(call.err_call(), "invalid connection"))?;
    if let Some(Stream::TextWrite(out)) = &connection.stream {
        ctx.protect(out.env);
    }
    Ok(connection)
}

pub(super) fn file(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    check_slot(ctx, call)?;
    let mut name = super::text(ctx, call, args.get(0), "description")?;
    let open = super::text(ctx, call, args.get(1), "open")?;
    let kind = super::text(ctx, call, args.get(2), "kind")?;
    let level = integer(ctx, args.get(3))?;
    let raw = flag(ctx, call, args.get(4), "raw")?;
    let format = match kind.as_str() {
        "gzfile" => Some(Compression::Gzip),
        "bzfile" => Some(Compression::Bzip2),
        "xzfile" => Some(Compression::Xz),
        "zstdfile" => Some(Compression::Zstd),
        _ => None,
    };
    let temporary = name.is_empty() && kind == "file";
    if temporary {
        name = super::filesystem::temp_name(ctx, "file", None, "");
    }
    let path = FilePath {
        name: super::filesystem::expand(ctx, &name),
        format,
        level: level.unsigned_abs(),
        detect: !raw,
        temporary,
    };
    let open = if temporary && open.is_empty() {
        "w+"
    } else {
        &open
    };
    let mode = if open.is_empty() {
        Mode::read(false)
    } else {
        Mode::parse(open).ok_or_else(|| ctx.stop(call.err_call(), "invalid 'open' argument"))?
    };
    let stream = if open.is_empty() {
        None
    } else {
        Some(path.open(ctx, call, mode)?)
    };
    register(
        ctx,
        call,
        &kind,
        Connection {
            description: name,
            path: Some(path),
            mode,
            stream,
            pushback: Cursor::default(),
        },
    )
}

pub(super) fn raw(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    check_slot(ctx, call)?;
    let object = args.get(0);
    if ctx.session().type_of(object)? != SexpType::Raw {
        return Err(ctx.stop(call.err_call(), "'object' must be a raw vector"));
    }
    let open = super::text(ctx, call, args.get(1), "open")?;
    let first = open.as_bytes().first();
    if open.as_bytes().get(1) == Some(&b't') {
        return Err(ctx.stop(call.err_call(), "invalid 'open' argument"));
    }
    let append = first == Some(&b'a');
    let plus = open.as_bytes().get(1) == Some(&b'+');
    let mode = Mode {
        binary: true,
        flags: OpenMode {
            read: first == Some(&b'r') || (plus && matches!(first, Some(b'w' | b'a'))),
            write: matches!(first, Some(b'w' | b'a')) || (plus && first == Some(&b'r')),
            append,
            create: false,
            truncate: false,
        },
    };
    check_limit(ctx, call, ctx.session().xlength(object)?)?;
    let mut buffer = Cursor::new(ctx.session().raw(object)?.iter().collect::<Vec<_>>());
    if append {
        buffer.set_position(buffer.get_ref().len() as u64);
    }
    let description = super::text(ctx, call, args.get(2), "description")?;
    register(
        ctx,
        call,
        "rawConnection",
        Connection {
            description,
            path: None,
            mode,
            stream: Some(Stream::Raw(buffer)),
            pushback: Cursor::default(),
        },
    )
}

pub(super) fn raw_value(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = id(ctx, call, args.get(0))?;
    let table = ctx.ext::<ConnTable>();
    let Some(Connection {
        stream: Some(Stream::Raw(buffer)),
        mode,
        ..
    }) = table.entries.get(&id)
    else {
        return Err(ctx.stop(call.err_call(), "'con' is not an output rawConnection"));
    };
    if !mode.flags.write {
        return Err(ctx.stop(call.err_call(), "'con' is not an output rawConnection"));
    }
    let bytes = buffer.get_ref().clone();
    Ok(ctx.session_mut().alloc_raw_from(&bytes)?)
}

pub(super) fn text(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    check_slot(ctx, call)?;
    let object = args.get(0);
    let open = super::text(ctx, call, args.get(1), "open")?;
    let encoding = args.get(3);
    let encoding = if encoding == SEXP_NIL {
        ByteEncoding::native(ctx)
    } else {
        let enc = super::texts(ctx, call, encoding, "encoding")?;
        match enc.as_slice() {
            [Some(a), Some(b), Some(c)] if a.is_empty() && b == "bytes" && c == "UTF-8" => {
                ByteEncoding::native(ctx)
            }
            [Some(enc)] if !enc.is_empty() && "bytes".starts_with(enc.as_str()) => {
                ByteEncoding::Bytes
            }
            [Some(enc)] if !enc.is_empty() && "UTF-8".starts_with(enc.as_str()) => {
                ByteEncoding::Utf8
            }
            _ => return Err(ctx.stop(call.err_call(), "'arg' should be one of “bytes”, “UTF-8”")),
        }
    };
    let mode = Mode::parse(&open)
        .filter(|_| matches!(open.as_str(), "r" | "w" | "a"))
        .ok_or_else(|| ctx.stop(call.err_call(), "invalid 'open' argument"))?;
    let description;
    let stream = if mode.flags.read {
        description = super::text(ctx, call, args.get(4), "description")?;
        let mut input = Vec::new();
        for line in translated_bytes(ctx, call, object, encoding)? {
            let line = line.as_deref().unwrap_or(b"NA");
            check_limit(
                ctx,
                call,
                input.len().saturating_add(line.len()).saturating_add(1),
            )?;
            input.extend_from_slice(line);
            input.push(b'\n');
        }
        Stream::ReadMemory(Cursor::new(input))
    } else {
        description = super::text(ctx, call, object, "text")?;
        let symbol = ctx.session_mut().intern(&description)?;
        let env = args.get(2);
        let lines = if mode.flags.append
            && let Some(binding) = ctx.session().env_get(env, symbol)
        {
            let value = if ctx.session().type_of(binding.value)? == SexpType::Prom {
                ctx.force(binding.value)?
            } else {
                binding.value
            };
            if ctx.session().type_of(value)? != SexpType::Str {
                return Err(ctx.stop(call.err_call(), "invalid 'text' argument"));
            }
            bytes(ctx, call, value)?
        } else {
            if mode.flags.append {
                ctx.warn(
                    call.err_call(),
                    "text connection: appending to a non-existent char vector",
                );
            }
            Vec::new()
        };
        let size = lines.iter().flatten().map(Vec::len).sum();
        let out = TextOutput {
            env,
            symbol,
            lines,
            partial: Vec::new(),
            size,
            encoding: if ctx.host().locale().utf8 {
                Encoding::Utf8
            } else {
                Encoding::Native
            },
        };
        let result = out.update(ctx);
        io_result(ctx, call, result)?;
        Stream::TextWrite(out)
    };
    register(
        ctx,
        call,
        "textConnection",
        Connection {
            description,
            path: None,
            mode,
            stream: Some(stream),
            pushback: Cursor::default(),
        },
    )
}

pub(super) fn text_value(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = id(ctx, call, args.get(0))?;
    let connection = take(ctx, call, id)?;
    let result = match &connection.stream {
        Some(Stream::TextWrite(out)) => Some(out.value(ctx)),
        _ => None,
    };
    ctx.ext::<ConnTable>().entries.insert(id, connection);
    result.ok_or_else(|| ctx.stop(call.err_call(), "'con' is not an output textConnection"))?
}

fn finish(ctx: &mut Ctx, stream: Stream) -> io::Result<()> {
    match stream {
        Stream::CompressedWrite(writer) => writer.finish(),
        Stream::File(mut file, _) => file.get_mut().flush(),
        Stream::TextWrite(mut out) if !out.partial.is_empty() => {
            out.lines.push(Some(std::mem::take(&mut out.partial)));
            out.update(ctx)
        }
        _ => Ok(()),
    }
}

pub(super) fn close(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = id(ctx, call, args.get(0))?;
    if (0..=2).contains(&id) {
        return Err(ctx.stop(call.err_call(), "cannot close standard connections"));
    }
    close_connection(ctx, call, id)?;
    Ok(SEXP_NIL)
}

fn close_connection(ctx: &mut Ctx, call: &Call, id: i32) -> EvalResult<()> {
    let connection = take(ctx, call, id)?;
    let result = connection
        .stream
        .map_or(Ok(()), |stream| finish(ctx, stream));
    if let Some(path) = connection.path.filter(|path| path.temporary) {
        let _ = ctx.host().remove(&path.name, false);
    }
    io_result(ctx, call, result)
}

/// Return the output sink depth or the active message connection number.
pub(crate) fn sink_number(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let message = flag(ctx, call, args.get(0), "type")?;
    let table = ctx.ext::<ConnTable>();
    let number = if message {
        table.message_sink.unwrap_or(2)
    } else {
        table.output_sinks.len() as i32
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
    if ctx.ext::<ConnTable>().message_sink.take().is_some() {
        ctx.pop_stream_sink(true)?;
    }
    if next != 2 {
        ctx.push_stream_sink(false, true, false, move |ctx, bytes| {
            write_sink(ctx, next, bytes)
        });
        ctx.ext::<ConnTable>().message_sink = Some(next);
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
        if id == 1 || table.output_sinks.last().is_some_and(|sink| sink.id == id) {
            return Ok(());
        }
        Some(id)
    };
    if ctx.ext::<ConnTable>().output_sinks.len() >= 19 {
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
                let mut connection = take(ctx, call, id)?;
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
        let description = super::text(ctx, call, file, "file")?;
        let path = FilePath {
            name: super::filesystem::expand(ctx, &description),
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
        .output_sinks
        .push(OutputSink { id, close });
    Ok(())
}

fn pop_output_sink(ctx: &mut Ctx, call: &Call) -> EvalResult<()> {
    let Some(sink) = ctx.ext::<ConnTable>().output_sinks.pop() else {
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
            let Some(mut connection) = ctx.ext::<ConnTable>().entries.remove(&id) else {
                return Err(ctx.stop(SEXP_NIL, "invalid connection"));
            };
            if let Some(Stream::TextWrite(out)) = &connection.stream {
                ctx.protect(out.env);
            }
            let result = connection.stream.as_mut().map_or_else(
                || Err(io::Error::other("invalid connection")),
                |stream| write_to(ctx, stream, bytes),
            );
            ctx.ext::<ConnTable>().entries.insert(id, connection);
            return result.map_err(|err| ctx.stop(SEXP_NIL, err.to_string()));
        }
    };
    result.map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))
}

pub(super) fn is_open(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = id(ctx, call, args.get(0))?;
    let rw = super::text(ctx, call, args.get(1), "rw")?;
    let (open, read, write) = if (0..=2).contains(&id) {
        (true, id == 0, id != 0)
    } else {
        let Some(connection) = ctx.ext::<ConnTable>().entries.get(&id) else {
            return Err(ctx.stop(call.err_call(), "invalid connection"));
        };
        (
            connection.stream.is_some(),
            connection.mode.flags.read,
            connection.mode.flags.write,
        )
    };
    let result = match rw.as_str() {
        "" => open,
        "read" => open && read,
        "write" => open && write,
        _ => return Err(ctx.stop(call.err_call(), "invalid 'rw' argument")),
    };
    Ok(ctx
        .session_mut()
        .alloc_lgl_from(&[roxide_value::RLogical::from_option(Some(result))])?)
}

pub(super) fn description(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = id(ctx, call, args.get(0))?;
    let description = match id {
        0 => "stdin".to_owned(),
        1 => "stdout".to_owned(),
        2 => "stderr".to_owned(),
        _ => {
            let Some(connection) = ctx.ext::<ConnTable>().entries.get(&id) else {
                return Err(ctx.stop(call.err_call(), "invalid connection"));
            };
            connection.description.clone()
        }
    };
    Ok(ctx.session_mut().scalar_str(&description)?)
}

pub(super) fn standard(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let id = integer(ctx, args.get(0))?;
    handle(ctx, "terminal", id)
}

struct Console<'a> {
    ctx: &'a mut Ctx,
    pending: Cursor<Vec<u8>>,
}

impl Read for Console<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let len = available.len().min(buffer.len());
        buffer[..len].copy_from_slice(&available[..len]);
        self.consume(len);
        Ok(len)
    }
}

impl BufRead for Console<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.pending.position() >= self.pending.get_ref().len() as u64 {
            self.pending.get_mut().clear();
            self.pending.set_position(0);
            self.ctx
                .host()
                .read_line(self.pending.get_mut())
                .map_err(io::Error::other)?;
        }
        self.pending.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.pending.consume(amount);
    }
}

struct PrefixedReader<'a, R: BufRead + ?Sized> {
    prefix: &'a mut Cursor<Vec<u8>>,
    inner: &'a mut R,
}

impl<R: BufRead + ?Sized> PrefixedReader<'_, R> {
    fn has_prefix(&self) -> bool {
        self.prefix.position() < self.prefix.get_ref().len() as u64
    }
}

impl<R: BufRead + ?Sized> Read for PrefixedReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let len = available.len().min(buf.len());
        buf[..len].copy_from_slice(&available[..len]);
        self.consume(len);
        Ok(len)
    }
}

impl<R: BufRead + ?Sized> BufRead for PrefixedReader<'_, R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.has_prefix() {
            self.prefix.fill_buf()
        } else {
            self.inner.fill_buf()
        }
    }

    fn consume(&mut self, amount: usize) {
        if self.has_prefix() {
            self.prefix.consume(amount);
            if !self.has_prefix() {
                self.prefix.get_mut().clear();
                self.prefix.set_position(0);
            }
        } else {
            self.inner.consume(amount);
        }
    }
}

pub(crate) fn take_pushback(ctx: &mut Ctx, id: i32) -> Vec<u8> {
    let table = ctx.ext::<ConnTable>();
    let cursor = if id == 0 {
        &mut table.stdin
    } else if let Some(connection) = table.entries.get_mut(&id) {
        &mut connection.pushback
    } else {
        return Vec::new();
    };
    let pos = (cursor.position() as usize).min(cursor.get_ref().len());
    let rest = cursor.get_ref()[pos..].to_vec();
    cursor.get_mut().clear();
    cursor.set_position(0);
    rest
}

pub(crate) fn set_pushback(ctx: &mut Ctx, id: i32, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    let limit = ctx.host().limits().max_vector_bytes;
    let table = ctx.ext::<ConnTable>();
    let cursor = if id == 0 {
        &mut table.stdin
    } else if let Some(connection) = table.entries.get_mut(&id) {
        &mut connection.pushback
    } else {
        return;
    };
    let pos = (cursor.position() as usize).min(cursor.get_ref().len());
    let existing = &cursor.get_ref()[pos..];
    if data.len().saturating_add(existing.len()) > limit {
        return;
    }
    let mut combined = Vec::with_capacity(data.len() + existing.len());
    combined.extend_from_slice(data);
    combined.extend_from_slice(existing);
    *cursor = Cursor::new(combined);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Lines,
    Binary,
    Characters,
    Serialized(Serialization),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Serialization {
    Stream,
    Rds,
    Workspace,
}

impl Access {
    fn binary(self) -> bool {
        self != Self::Lines
    }

    fn text_error(self, read: bool) -> Option<&'static str> {
        match (self, read) {
            (Self::Binary, true) => Some("can only read from a binary connection"),
            (Self::Binary, false) => Some("can only write to a binary connection"),
            (Self::Serialized(Serialization::Stream | Serialization::Rds), true) => {
                Some("input format does not match specified format")
            }
            (Self::Serialized(Serialization::Rds), false) => {
                Some("binary-mode connection required for ascii=false")
            }
            (Self::Serialized(Serialization::Workspace), false) => {
                Some("can only save to a binary connection")
            }
            _ => None,
        }
    }
}

pub(crate) fn read<T>(
    ctx: &mut Ctx,
    call: &Call,
    source: SexpId,
    access: Access,
    read: impl FnOnce(&mut dyn BufRead, usize) -> io::Result<T>,
) -> EvalResult<T> {
    let binary = access.binary();
    let limit = ctx.host().limits().max_vector_bytes;
    match ctx.session().type_of(source)? {
        SexpType::Raw if binary => {
            check_limit(ctx, call, ctx.session().xlength(source)?)?;
            let data: Vec<_> = ctx.session().raw(source)?.iter().collect();
            io_result(ctx, call, read(&mut Cursor::new(data), limit))
        }
        SexpType::Str => {
            let source = super::text(ctx, call, source, "con")?;
            let path = FilePath {
                name: super::filesystem::expand(ctx, &source),
                format: None,
                level: 0,
                detect: !binary,
                temporary: false,
            };
            let mut stream = path.open(ctx, call, Mode::read(binary))?;
            let result = read_from(&mut stream, |input| read(input, limit));
            io_result(ctx, call, result)
        }
        _ => {
            let id = id(ctx, call, source)?;
            if id == 0 && !binary {
                let pending = std::mem::take(&mut ctx.ext::<ConnTable>().stdin);
                let mut console = Console { ctx, pending };
                let result = read(&mut console, limit);
                console.ctx.ext::<ConnTable>().stdin = console.pending;
                return io_result(ctx, call, result);
            }
            if (0..=2).contains(&id) {
                return Err(ctx.stop(
                    call.err_call(),
                    if binary {
                        "can only read from a binary connection"
                    } else {
                        "cannot read from this connection"
                    },
                ));
            }
            operate(
                ctx,
                call,
                id,
                Mode::read(binary),
                access,
                |_, stream, pushback| {
                    read_from(stream, |input| {
                        if !binary && pushback.position() < pushback.get_ref().len() as u64 {
                            let mut prefixed = PrefixedReader {
                                prefix: pushback,
                                inner: input,
                            };
                            read(&mut prefixed, limit)
                        } else {
                            read(input, limit)
                        }
                    })
                },
            )
        }
    }
}

fn read_from<T>(
    stream: &mut Stream,
    read: impl FnOnce(&mut dyn BufRead) -> io::Result<T>,
) -> io::Result<T> {
    match stream {
        Stream::File(file, _) => read(file),
        Stream::CompressedRead(file) => read(file),
        Stream::Raw(buffer) | Stream::ReadMemory(buffer) => read(buffer),
        _ => Err(io::Error::other("cannot read from this connection")),
    }
}

pub(crate) fn read_serialized<T>(
    ctx: &mut Ctx,
    call: &Call,
    source: SexpId,
    serialization: Serialization,
    read: impl FnOnce(&mut RSession, &mut dyn BufRead) -> io::Result<T>,
) -> EvalResult<T> {
    let id = id(ctx, call, source)?;
    let caller_opened = ctx
        .ext::<ConnTable>()
        .entries
        .get(&id)
        .is_some_and(|connection| connection.stream.is_some());
    operate(
        ctx,
        call,
        id,
        Mode::read(true),
        Access::Serialized(serialization),
        |ctx, stream, _| {
            if serialization == Serialization::Workspace {
                prepare_workspace(
                    stream,
                    caller_opened,
                    ctx.host().limits().max_decompress_bytes,
                )?;
            }
            read_from(stream, |input| read(ctx.session_mut(), input))
        },
    )
}

fn prepare_workspace(stream: &mut Stream, open: bool, limit: usize) -> io::Result<()> {
    let Stream::File(file, _) = stream else {
        return Ok(());
    };
    let Some(format) = Compression::detect(file.fill_buf()?) else {
        return Ok(());
    };
    if !open {
        return Err(io::Error::other(
            "can only use read- or write- binary connections",
        ));
    }
    if !matches!(format, Compression::Gzip) {
        return Err(io::Error::other(
            "the input does not start with a magic number compatible with loading from a connection",
        ));
    }
    let original = std::mem::replace(stream, Stream::ReadMemory(Cursor::default()));
    if let Stream::File(file, _) = original {
        *stream = Stream::CompressedRead(codec::gzip_reader(file, limit));
    }
    Ok(())
}

pub(crate) fn write_serialized(
    ctx: &mut Ctx,
    call: &Call,
    dest: SexpId,
    serialization: Serialization,
    data: &[u8],
) -> EvalResult<()> {
    let id = id(ctx, call, dest)?;
    operate(
        ctx,
        call,
        id,
        Mode::write(true),
        Access::Serialized(serialization),
        |ctx, stream, _| write_to(ctx, stream, data),
    )
}

pub(super) fn write(
    ctx: &mut Ctx,
    call: &Call,
    dest: SexpId,
    access: Access,
    data: &[u8],
) -> EvalResult<()> {
    let binary = access.binary();
    if ctx.session().type_of(dest)? == SexpType::Str {
        let dest = super::text(ctx, call, dest, "con")?;
        let path = FilePath {
            name: super::filesystem::expand(ctx, &dest),
            format: None,
            level: 0,
            detect: false,
            temporary: false,
        };
        let mut stream = path.open(ctx, call, Mode::write(binary))?;
        let result = write_to(ctx, &mut stream, data).and_then(|()| finish(ctx, stream));
        return io_result(ctx, call, result);
    }
    let id = id(ctx, call, dest)?;
    if (0..=2).contains(&id) {
        let result = match id {
            1 if !binary => ctx.host().write_out(data),
            2 if !binary => ctx.host().write_err(data),
            _ => return Err(ctx.stop(call.err_call(), "cannot write to this connection")),
        };
        return result.map_err(|e| ctx.stop(call.err_call(), e.to_string()));
    }
    operate(
        ctx,
        call,
        id,
        Mode::write(binary),
        access,
        |ctx, stream, _| write_to(ctx, stream, data),
    )
}

fn check_buffer(ctx: &mut Ctx, size: usize) -> io::Result<()> {
    if size > ctx.host().limits().max_vector_bytes {
        return Err(io::Error::other("I/O result exceeds the vector size limit"));
    }
    Ok(())
}

fn write_to(ctx: &mut Ctx, stream: &mut Stream, data: &[u8]) -> io::Result<()> {
    match stream {
        Stream::File(file, write_position) => {
            if let Some(write_position) = write_position {
                let read_position = file.stream_position()?;
                file.seek(io::SeekFrom::Start(*write_position))?;
                let result = file.get_mut().write_all(data);
                let position = file.get_mut().stream_position();
                let restored = file.seek(io::SeekFrom::Start(read_position));
                *write_position = position?;
                restored?;
                result
            } else {
                file.get_mut().write_all(data)
            }
        }
        Stream::CompressedWrite(writer) => writer.write_all(data),
        Stream::Raw(buffer) => {
            check_buffer(ctx, (buffer.position() as usize).saturating_add(data.len()))?;
            buffer.write_all(data)
        }
        Stream::TextWrite(out) => out.write(ctx, data),
        Stream::CompressedRead(_) | Stream::ReadMemory(_) => {
            Err(io::Error::other("cannot write to this connection"))
        }
    }
}

fn operate<T>(
    ctx: &mut Ctx,
    call: &Call,
    id: i32,
    mode: Mode,
    access: Access,
    operation: impl FnOnce(&mut Ctx, &mut Stream, &mut Cursor<Vec<u8>>) -> io::Result<T>,
) -> EvalResult<T> {
    let table = ctx.ext::<ConnTable>();
    let Some(connection) = table.entries.get(&id) else {
        return Err(ctx.stop(call.err_call(), "invalid connection"));
    };
    if connection.stream.is_none() {
        if access == Access::Serialized(Serialization::Stream) {
            return Err(ctx.stop(call.err_call(), "connection is not open"));
        }
        let Some(path) = connection.path.clone() else {
            return Err(ctx.stop(call.err_call(), "invalid connection"));
        };
        let mut stream = path.open(ctx, call, mode)?;
        let mut empty = Cursor::default();
        let result = operation(ctx, &mut stream, &mut empty);
        let closed = finish(ctx, stream);
        return io_result(ctx, call, result.and_then(|value| closed.map(|()| value)));
    }

    if !connection.mode.binary
        && let Some(message) = access.text_error(mode.flags.read)
    {
        return Err(ctx.stop(call.err_call(), message));
    }
    if mode.flags.read && !connection.mode.flags.read {
        return Err(ctx.stop(call.err_call(), "cannot read from this connection"));
    }
    if mode.flags.write && !connection.mode.flags.write {
        return Err(ctx.stop(call.err_call(), "cannot write to this connection"));
    }
    if access == Access::Characters && !connection.mode.binary {
        let name = if mode.flags.read {
            "readChar"
        } else {
            "writeChar"
        };
        ctx.warn(
            call.err_call(),
            format!("text connection used with {name}(), results may be incorrect"),
        );
    }
    // Stream operations return I/O errors. Restore the entry before an R calling handler can run.
    let mut connection = take(ctx, call, id)?;
    let Connection {
        stream, pushback, ..
    } = &mut connection;
    let result = stream.as_mut().map_or_else(
        || Err(io::Error::other("invalid connection")),
        |stream| operation(ctx, stream, pushback),
    );
    ctx.ext::<ConnTable>().entries.insert(id, connection);
    io_result(ctx, call, result)
}

pub(super) fn label(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<String> {
    if ctx.session().type_of(value)? == SexpType::Str {
        return super::text(ctx, call, value, "con");
    }
    let id = id(ctx, call, value)?;
    Ok(ctx
        .ext::<ConnTable>()
        .entries
        .get(&id)
        .and_then(|connection| connection.path.as_ref())
        .map_or_else(
            || String::from("connection"),
            |path| path.name.to_string_lossy().into_owned(),
        ))
}

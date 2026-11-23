use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Cursor, Read, Seek, Write};
use std::path::PathBuf;

use roxide_eval::{Args, Call, Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_host::{HostFile, OpenMode};
use roxide_value::{Encoding, SEXP_NIL, SexpType, Sym, ValueApi};

use super::{
    Compression, attr, bytes, check_limit, compress, decompress, flag, integer, io_result, strings,
};

#[derive(Default)]
struct ConnTable {
    entries: BTreeMap<i32, Connection>,
    stdin: Cursor<Vec<u8>>,
    last_id: i32,
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
    path: Option<FilePath>,
    mode: Mode,
    stream: Option<Stream>,
}

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
    CompressedWrite {
        file: Box<dyn HostFile>,
        format: Compression,
        level: u32,
        buffer: Vec<u8>,
    },
    TextWrite(TextOutput),
}

struct TextOutput {
    env: SexpId,
    symbol: Sym,
    lines: Vec<Option<Vec<u8>>>,
    partial: Vec<u8>,
    size: usize,
}

impl TextOutput {
    fn value(&self, ctx: &mut Ctx) -> EvalResult {
        strings(
            ctx,
            self.lines.iter().map(Option::as_deref),
            Encoding::Native,
        )
    }

    fn update(&self, ctx: &mut Ctx) -> EvalResult<()> {
        let value = self.value(ctx)?;
        ctx.assign(self.symbol, value, self.env)
    }

    fn write(&mut self, ctx: &mut Ctx, call: &Call, data: &[u8]) -> EvalResult<()> {
        check_limit(ctx, call, self.size.saturating_add(data.len()))?;
        self.size += data.len();
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
            return Ok(Stream::CompressedWrite {
                file,
                format,
                level: self.level,
                buffer: Vec::new(),
            });
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
                return Ok(Stream::ReadMemory(Cursor::new(io_result(
                    ctx,
                    call,
                    decompress(format, file, limit),
                )?)));
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

fn register(ctx: &mut Ctx, call: &Call, kind: &str, connection: Connection) -> EvalResult {
    let next = ctx.ext::<ConnTable>().last_id.max(2).saturating_add(1);
    if next == i32::MAX {
        return Err(ctx.stop(call.err_call(), "all connections are in use"));
    }
    let value = handle(ctx, kind, next)?;
    let table = ctx.ext::<ConnTable>();
    table.last_id = next;
    table.entries.insert(next, connection);
    Ok(value)
}

fn id(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<i32> {
    let key = ctx.session_mut().intern("class")?;
    let class = ctx.session().attr(value, key);
    let is_connection = ctx.session().str_slice(class).is_ok_and(|ids| {
        ids.iter().any(|&id| {
            ctx.session()
                .char_bytes(id)
                .is_ok_and(|b| b == b"connection")
        })
    });
    if !is_connection {
        return Err(ctx.stop(call.err_call(), "'con' is not a connection"));
    }
    Ok(ctx.session().int(value)?.iter().next().unwrap_or(-1))
}

fn take(ctx: &mut Ctx, call: &Call, id: i32) -> EvalResult<Connection> {
    ctx.ext::<ConnTable>()
        .entries
        .remove(&id)
        .ok_or_else(|| ctx.stop(call.err_call(), "invalid connection"))
}

pub(super) fn file(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut name = super::text(ctx, call, args.get(0), "description")?;
    let open = super::text(ctx, call, args.get(1), "open")?;
    let kind = super::text(ctx, call, args.get(2), "kind")?;
    let level = integer(ctx, args.get(3))?;
    let raw = flag(ctx, call, args.get(4), "raw")?;
    let format = match kind.as_str() {
        "gzfile" => Some(Compression::Gzip),
        "bzfile" => Some(Compression::Bzip2),
        "xzfile" => Some(Compression::Xz),
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
            path: Some(path),
            mode,
            stream,
        },
    )
}

pub(super) fn text(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let object = args.get(0);
    let open = super::text(ctx, call, args.get(1), "open")?;
    let mode = Mode::parse(&open)
        .filter(|_| matches!(open.as_str(), "r" | "w" | "a"))
        .ok_or_else(|| ctx.stop(call.err_call(), "invalid 'open' argument"))?;
    let stream = if mode.flags.read {
        let mut input = Vec::new();
        for line in bytes(ctx, object)? {
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
        let name = super::text(ctx, call, object, "text")?;
        let symbol = ctx.session_mut().intern(&name)?;
        let env = args.get(2);
        let lines = if mode.flags.append
            && let Some(binding) = ctx.session().env_get(env, symbol)
        {
            let value = if ctx.session().type_of(binding.value)? == SexpType::Prom {
                ctx.force(binding.value)?
            } else {
                binding.value
            };
            bytes(ctx, value).map_err(|_| ctx.stop(call.err_call(), "invalid 'text' argument"))?
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
        };
        out.update(ctx)?;
        Stream::TextWrite(out)
    };
    register(
        ctx,
        call,
        "textConnection",
        Connection {
            path: None,
            mode,
            stream: Some(stream),
        },
    )
}

pub(super) fn text_value(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = id(ctx, call, args.get(0))?;
    let connection = take(ctx, call, id)?;
    let result = match &connection.stream {
        Some(Stream::TextWrite(out)) => out.value(ctx),
        _ => Err(ctx.stop(call.err_call(), "'con' is not an output textConnection")),
    };
    ctx.ext::<ConnTable>().entries.insert(id, connection);
    result
}

fn finish(ctx: &mut Ctx, call: &Call, stream: Stream) -> EvalResult<()> {
    match stream {
        Stream::CompressedWrite {
            mut file,
            format,
            level,
            buffer,
        } => {
            let compressed = io_result(ctx, call, compress(format, &buffer, level))?;
            io_result(
                ctx,
                call,
                file.write_all(&compressed).and_then(|()| file.flush()),
            )
        }
        Stream::File(mut file, _) => io_result(ctx, call, file.get_mut().flush()),
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
    let connection = take(ctx, call, id)?;
    let result = connection
        .stream
        .map_or(Ok(()), |stream| finish(ctx, call, stream));
    if let Some(path) = connection.path.filter(|path| path.temporary) {
        let _ = ctx.host().remove(&path.name, false);
    }
    result?;
    Ok(SEXP_NIL)
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Access {
    Lines,
    Binary,
    Characters,
}

impl Access {
    fn binary(self) -> bool {
        self != Self::Lines
    }
}

pub(super) fn read<T>(
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
            read_from(ctx, call, &mut stream, read, limit)
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
            operate(ctx, call, id, Mode::read(binary), access, |ctx, stream| {
                read_from(ctx, call, stream, read, limit)
            })
        }
    }
}

fn read_from<T>(
    ctx: &mut Ctx,
    call: &Call,
    stream: &mut Stream,
    read: impl FnOnce(&mut dyn BufRead, usize) -> io::Result<T>,
    limit: usize,
) -> EvalResult<T> {
    let result = match stream {
        Stream::File(file, _) => read(file, limit),
        Stream::ReadMemory(buffer) => read(buffer, limit),
        _ => return Err(ctx.stop(call.err_call(), "cannot read from this connection")),
    };
    io_result(ctx, call, result)
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
        write_to(ctx, call, &mut stream, data)?;
        return finish(ctx, call, stream);
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
    operate(ctx, call, id, Mode::write(binary), access, |ctx, stream| {
        write_to(ctx, call, stream, data)
    })
}

fn write_to(ctx: &mut Ctx, call: &Call, stream: &mut Stream, data: &[u8]) -> EvalResult<()> {
    match stream {
        Stream::File(file, write_position) => {
            if let Some(write_position) = write_position {
                let read_position = io_result(ctx, call, file.stream_position())?;
                io_result(
                    ctx,
                    call,
                    file.seek(std::io::SeekFrom::Start(*write_position)),
                )?;
                let result = file.get_mut().write_all(data);
                let position = file.get_mut().stream_position();
                let restored = file.seek(std::io::SeekFrom::Start(read_position));
                *write_position = io_result(ctx, call, position)?;
                io_result(ctx, call, restored)?;
                io_result(ctx, call, result)
            } else {
                io_result(ctx, call, file.get_mut().write_all(data))
            }
        }
        Stream::CompressedWrite { buffer, .. } => {
            check_limit(ctx, call, buffer.len().saturating_add(data.len()))?;
            buffer.extend_from_slice(data);
            Ok(())
        }
        Stream::TextWrite(out) => out.write(ctx, call, data),
        Stream::ReadMemory(_) => Err(ctx.stop(call.err_call(), "cannot write to this connection")),
    }
}

fn operate<T>(
    ctx: &mut Ctx,
    call: &Call,
    id: i32,
    mode: Mode,
    access: Access,
    operation: impl FnOnce(&mut Ctx, &mut Stream) -> EvalResult<T>,
) -> EvalResult<T> {
    let mut connection = take(ctx, call, id)?;
    let result = match &mut connection.stream {
        Some(_) if access == Access::Binary && !connection.mode.binary => Err(ctx.stop(
            call.err_call(),
            if mode.flags.read {
                "can only read from a binary connection"
            } else {
                "can only write to a binary connection"
            },
        )),
        Some(_) if mode.flags.read && !connection.mode.flags.read => {
            Err(ctx.stop(call.err_call(), "cannot read from this connection"))
        }
        Some(_) if mode.flags.write && !connection.mode.flags.write => {
            Err(ctx.stop(call.err_call(), "cannot write to this connection"))
        }
        Some(stream) => {
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
            operation(ctx, stream)
        }
        None => (|| {
            let path = connection
                .path
                .as_ref()
                .ok_or_else(|| ctx.stop(call.err_call(), "invalid connection"))?;
            let mut stream = path.open(ctx, call, mode)?;
            let result = operation(ctx, &mut stream);
            let closed = finish(ctx, call, stream);
            result.and_then(|value| closed.map(|()| value))
        })(),
    };
    ctx.ext::<ConnTable>().entries.insert(id, connection);
    result
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

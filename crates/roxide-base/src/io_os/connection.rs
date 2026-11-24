mod input;
mod sink;

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Cursor, Seek, Write};
use std::path::PathBuf;

use roxide_eval::{Args, Call, Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_host::{HostFile, OpenMode};
use roxide_value::{CharId, Encoding, RSession, SEXP_NIL, SexpType, Sym, ValueApi};

use super::codec::{self, Compression};
use super::{
    ByteEncoding, attr, bytes, check_limit, flag, integer, io_result, strings, translated_bytes,
};

pub(crate) use input::{read, set_pushback, take_pushback};
pub(crate) use sink::{sink, sink_number};

use input::read_from;
use sink::Sinks;

#[derive(Default)]
struct ConnTable {
    entries: BTreeMap<i32, Connection>,
    stdin: Cursor<Vec<u8>>,
    last_id: i32,
    sinks: Sinks,
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
    kind: String,
    open_mode: String,
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

    fn closed_file() -> Self {
        let mut mode = Self::read(false);
        mode.flags.write = true;
        mode
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

fn register(ctx: &mut Ctx, call: &Call, connection: Connection) -> EvalResult {
    check_slot(ctx, call)?;
    let next = ctx.ext::<ConnTable>().last_id.max(2) + 1;
    let value = handle(ctx, &connection.kind, next)?;
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

fn take(ctx: &mut Ctx, error_call: SexpId, id: i32) -> EvalResult<Connection> {
    let connection = ctx
        .ext::<ConnTable>()
        .entries
        .remove(&id)
        .ok_or_else(|| ctx.stop(error_call, "invalid connection"))?;
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
        Mode::closed_file()
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
        Connection {
            description: name,
            open_mode: if open.is_empty() {
                if format.is_some() { "rb" } else { "r" }.to_owned()
            } else {
                open.to_owned()
            },
            kind,
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
        Connection {
            description,
            kind: "rawConnection".into(),
            open_mode: open,
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
        Connection {
            description,
            kind: "textConnection".into(),
            open_mode: open,
            path: None,
            mode,
            stream: Some(stream),
            pushback: Cursor::default(),
        },
    )
}

pub(super) fn text_value(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = id(ctx, call, args.get(0))?;
    let connection = take(ctx, call.err_call(), id)?;
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
    let connection = take(ctx, call.err_call(), id)?;
    let result = connection
        .stream
        .map_or(Ok(()), |stream| finish(ctx, stream));
    if let Some(path) = connection.path.filter(|path| path.temporary) {
        let _ = ctx.host().remove(&path.name, false);
    }
    io_result(ctx, call, result)
}

pub(super) fn is_open(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = id(ctx, call, args.get(0))?;
    let s = ctx.session();
    let Some(&rw) = s.str_slice(args.get(1))?.first() else {
        return Err(ctx.stop(call.err_call(), "unknown 'rw' value"));
    };
    let rw = s.char_bytes(rw)?;
    let read_mode = !rw.is_empty() && b"read".starts_with(rw);
    let write_mode = !rw.is_empty() && b"write".starts_with(rw);
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
    let result = open && (!read_mode || read) && (!write_mode || write);
    Ok(ctx
        .session_mut()
        .alloc_lgl_from(&[roxide_value::RLogical::from_option(Some(result))])?)
}

pub(super) fn summary(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = id(ctx, call, args.get(0))?;
    let (description, kind, mode, binary, open, read, write) = if let Some(name) = standard_name(id)
    {
        let input = id == 0;
        (
            name.into(),
            "terminal".into(),
            if input { "r" } else { "w" }.into(),
            false,
            true,
            input,
            !input,
        )
    } else {
        let Some(connection) = ctx.ext::<ConnTable>().entries.get(&id) else {
            return Err(ctx.stop(call.err_call(), "invalid connection"));
        };
        let open = connection.stream.is_some();
        (
            connection.description.clone(),
            connection.kind.clone(),
            connection.open_mode.clone(),
            connection.mode.binary,
            open,
            connection.mode.flags.read,
            connection.mode.flags.write,
        )
    };
    let yes_no = |flag| if flag { "yes" } else { "no" };
    let s = ctx.session_mut();
    let values = [
        description.as_str(),
        &kind,
        &mode,
        if binary { "binary" } else { "text" },
        if open { "opened" } else { "closed" },
        yes_no(read),
        yes_no(write),
    ]
    .map(|text| s.scalar_str(text))
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    let result = crate::ops::common::list(s, &values)?;
    let names = [
        "description",
        "class",
        "mode",
        "text",
        "opened",
        "can read",
        "can write",
    ]
    .map(|name| s.mk_char(name.as_bytes(), Encoding::Native))
    .into_iter()
    .collect::<Result<Vec<CharId>, _>>()?;
    crate::ops::common::set_names(s, result, &names)?;
    Ok(result)
}

fn standard_name(id: i32) -> Option<&'static str> {
    match id {
        0 => Some("stdin"),
        1 => Some("stdout"),
        2 => Some("stderr"),
        _ => None,
    }
}

pub(super) fn description(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = id(ctx, call, args.get(0))?;
    let description = match standard_name(id) {
        Some(name) => name.to_owned(),
        None => {
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
        if let Some(connection) = ctx.ext::<ConnTable>().entries.get_mut(&id) {
            connection.mode = mode;
        }
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
    with_open_stream(ctx, call.err_call(), id, operation)
}

fn with_open_stream<T>(
    ctx: &mut Ctx,
    error_call: SexpId,
    id: i32,
    operation: impl FnOnce(&mut Ctx, &mut Stream, &mut Cursor<Vec<u8>>) -> io::Result<T>,
) -> EvalResult<T> {
    let mut connection = take(ctx, error_call, id)?;
    let Connection {
        stream, pushback, ..
    } = &mut connection;
    let result = stream.as_mut().map_or_else(
        || Err(io::Error::other("invalid connection")),
        |stream| operation(ctx, stream, pushback),
    );
    // An R calling handler can inspect the connection as soon as ctx.stop runs.
    ctx.ext::<ConnTable>().entries.insert(id, connection);
    result.map_err(|err| ctx.stop(error_call, err.to_string()))
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

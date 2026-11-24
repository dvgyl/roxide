use std::io::{self, BufRead, Cursor, Read};

use roxide_eval::{Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SexpType, ValueApi};

use super::super::{check_limit, filesystem, io_result, text};
use super::{Access, ConnTable, FilePath, Mode, Stream, id, operate};

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
            let source = text(ctx, call, source, "con")?;
            let path = FilePath {
                name: filesystem::expand(ctx, &source),
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

pub(super) fn read_from<T>(
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

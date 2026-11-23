use std::io::{self, BufReader, Read, Write};

use roxide_host::HostFile;

use bzip2::{read::MultiBzDecoder, write::BzEncoder};
use flate2::{read::MultiGzDecoder, write::GzEncoder};
use lzma_rust2::{XzOptions, XzReader, XzWriter};

/// Compression formats shared by connections and serialized objects.
#[derive(Clone, Copy)]
pub(super) enum Compression {
    /// Gzip stream.
    Gzip,
    /// Bzip2 stream.
    Bzip2,
    /// XZ stream.
    Xz,
    /// Zstandard stream.
    Zstd,
}

impl Compression {
    /// Identify a compression format from the first bytes of a stream.
    pub(super) fn detect(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"\x1f\x8b") {
            Some(Self::Gzip)
        } else if bytes.starts_with(b"BZh") {
            Some(Self::Bzip2)
        } else if bytes.starts_with(b"\xfd7zXZ\0") {
            Some(Self::Xz)
        } else if bytes.starts_with(b"\x28\xb5\x2f\xfd") {
            Some(Self::Zstd)
        } else {
            None
        }
    }
}

pub(super) enum Writer {
    Gzip(GzEncoder<Box<dyn HostFile>>),
    Bzip2(BzEncoder<Box<dyn HostFile>>),
    Xz(Box<XzWriter<Box<dyn HostFile>>>),
    Zstd(zstd::stream::write::Encoder<'static, Box<dyn HostFile>>),
}

impl Writer {
    pub(super) fn new(
        format: Compression,
        file: Box<dyn HostFile>,
        level: u32,
    ) -> io::Result<Self> {
        Ok(match format {
            Compression::Gzip => {
                Self::Gzip(GzEncoder::new(file, flate2::Compression::new(level.min(9))))
            }
            Compression::Bzip2 => Self::Bzip2(BzEncoder::new(
                file,
                bzip2::Compression::new(level.clamp(1, 9)),
            )),
            Compression::Xz => Self::Xz(Box::new(XzWriter::new(
                file,
                XzOptions::with_preset(level.min(9)),
            )?)),
            Compression::Zstd => Self::Zstd(zstd::stream::write::Encoder::new(
                file,
                level.min(22) as i32,
            )?),
        })
    }

    pub(super) fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Self::Gzip(writer) => writer.write_all(bytes),
            Self::Bzip2(writer) => writer.write_all(bytes),
            Self::Xz(writer) => writer.write_all(bytes),
            Self::Zstd(writer) => writer.write_all(bytes),
        }
    }

    pub(super) fn finish(self) -> io::Result<()> {
        let mut file = match self {
            Self::Gzip(writer) => writer.finish()?,
            Self::Bzip2(writer) => writer.finish()?,
            Self::Xz(writer) => writer.finish()?,
            Self::Zstd(writer) => writer.finish()?,
        };
        file.flush()
    }
}

pub(super) fn reader(
    format: Compression,
    file: impl Read + 'static,
    limit: usize,
) -> io::Result<BufReader<LimitedReader>> {
    let input: Box<dyn Read> = match format {
        Compression::Gzip => Box::new(MultiGzDecoder::new(file)),
        Compression::Bzip2 => Box::new(MultiBzDecoder::new(file)),
        Compression::Xz => Box::new(XzReader::new_mem_limit(file, true, 128 * 1024)),
        Compression::Zstd => {
            let mut reader = zstd::stream::read::Decoder::new(file)?;
            reader.window_log_max(27)?;
            Box::new(reader)
        }
    };
    Ok(limited(input, limit))
}

pub(super) fn gzip_reader(file: impl Read + 'static, limit: usize) -> BufReader<LimitedReader> {
    limited(Box::new(MultiGzDecoder::new(file)), limit)
}

fn limited(input: Box<dyn Read>, limit: usize) -> BufReader<LimitedReader> {
    BufReader::new(LimitedReader {
        input,
        remaining: limit,
        exceeded: false,
    })
}

pub(super) struct LimitedReader {
    input: Box<dyn Read>,
    remaining: usize,
    exceeded: bool,
}

impl Read for LimitedReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            if self.exceeded || self.input.read(&mut [0])? != 0 {
                self.exceeded = true;
                return Err(io::Error::other("input exceeds the byte limit"));
            }
            return Ok(0);
        }
        let width = self.remaining.min(buffer.len());
        let count = self.input.read(&mut buffer[..width])?;
        self.remaining -= count;
        Ok(count)
    }
}

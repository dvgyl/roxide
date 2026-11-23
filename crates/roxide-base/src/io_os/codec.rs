use std::io::{self, Read, Write};

use bzip2::{read::MultiBzDecoder, write::BzEncoder};
use flate2::{read::MultiGzDecoder, write::GzEncoder};
use lzma_rust2::{XzOptions, XzReader, XzWriter};

/// Compression formats shared by connections and serialized objects.
#[derive(Clone, Copy)]
pub(crate) enum Compression {
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
    pub(crate) fn detect(bytes: &[u8]) -> Option<Self> {
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

/// Encode bytes as a complete compressed stream.
pub(crate) fn compress(format: Compression, bytes: &[u8], level: u32) -> io::Result<Vec<u8>> {
    match format {
        Compression::Gzip => {
            let mut writer = GzEncoder::new(Vec::new(), flate2::Compression::new(level.min(9)));
            writer.write_all(bytes)?;
            writer.finish()
        }
        Compression::Bzip2 => {
            let mut writer = BzEncoder::new(Vec::new(), bzip2::Compression::new(level.clamp(1, 9)));
            writer.write_all(bytes)?;
            writer.finish()
        }
        Compression::Xz => {
            let mut writer = XzWriter::new(Vec::new(), XzOptions::with_preset(level.min(9)))?;
            writer.write_all(bytes)?;
            writer.finish()
        }
        Compression::Zstd => zstd::stream::encode_all(bytes, level.min(22) as i32),
    }
}

/// Decode a compressed stream up to the specified output limit in bytes.
pub(crate) fn decompress(
    format: Compression,
    reader: impl Read,
    limit: usize,
) -> io::Result<Vec<u8>> {
    match format {
        Compression::Gzip => read_limited(MultiGzDecoder::new(reader), limit),
        Compression::Bzip2 => read_limited(MultiBzDecoder::new(reader), limit),
        Compression::Xz => read_limited(XzReader::new_mem_limit(reader, true, 128 * 1024), limit),
        Compression::Zstd => {
            let mut reader = zstd::stream::read::Decoder::new(reader)?;
            reader.window_log_max(27)?;
            read_limited(reader, limit)
        }
    }
}

pub(super) fn read_limited(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::other("input exceeds the byte limit"));
    }
    Ok(bytes)
}

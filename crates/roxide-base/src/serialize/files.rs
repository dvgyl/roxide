use std::io::{BufReader, Cursor, Read, Write};
use std::path::Path;

use bzip2::read::MultiBzDecoder;
use bzip2::write::BzEncoder;
use flate2::read::MultiGzDecoder;
use flate2::write::GzEncoder;
use lzma_rust2::{XzOptions, XzReader, XzWriter};
use roxide_host::{Host, OpenMode};

use super::{WireResult, invalid};

pub(super) enum Compression {
    None,
    Gzip,
    Bzip2,
    Xz,
    Zstd,
}

impl Compression {
    pub(super) fn from_name(name: &[u8]) -> WireResult<Self> {
        match name {
            b"gzip" => Ok(Self::Gzip),
            b"bzip2" => Ok(Self::Bzip2),
            b"xz" => Ok(Self::Xz),
            b"zstd" => Ok(Self::Zstd),
            _ => Err(invalid("invalid 'compress' argument")),
        }
    }
}

pub(super) fn read(host: &mut dyn Host, path: &Path) -> WireResult<Vec<u8>> {
    let limit = host.limits().max_decompress_bytes;
    let file = host.open(path, OpenMode::read_only()).map_err(invalid)?;
    let mut file = BufReader::new(file);
    let mut prefix = Vec::with_capacity(6);
    file.by_ref().take(6).read_to_end(&mut prefix)?;
    let file = Cursor::new(prefix.clone()).chain(file);
    let reader: Box<dyn Read> = if prefix.starts_with(b"\x1f\x8b") {
        Box::new(MultiGzDecoder::new(file))
    } else if prefix.starts_with(b"BZh") {
        Box::new(MultiBzDecoder::new(file))
    } else if prefix.starts_with(b"\xfd7zXZ\0") {
        Box::new(XzReader::new_mem_limit(file, true, 128 * 1024))
    } else if prefix.starts_with(b"\x28\xb5\x2f\xfd") {
        let mut decoder = zstd::stream::read::Decoder::new(file)?;
        decoder.window_log_max(27)?;
        Box::new(decoder)
    } else {
        Box::new(file)
    };
    let mut reader = reader.take((limit as u64).saturating_add(1));
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid("serialized input exceeds the decompression limit"));
    }
    Ok(bytes)
}

pub(super) fn write(
    host: &mut dyn Host,
    path: &Path,
    bytes: &[u8],
    compression: Compression,
    level: Option<u32>,
) -> WireResult<()> {
    let mut file = host
        .open(path, OpenMode::write_truncate())
        .map_err(invalid)?;
    match compression {
        Compression::None => file.write_all(bytes)?,
        Compression::Gzip => {
            let mut encoder =
                GzEncoder::new(&mut file, flate2::Compression::new(level.unwrap_or(6)));
            encoder.write_all(bytes)?;
            encoder.finish()?;
        }
        Compression::Bzip2 => {
            let mut encoder = BzEncoder::new(
                &mut file,
                bzip2::Compression::new(level.unwrap_or(9).max(1)),
            );
            encoder.write_all(bytes)?;
            encoder.finish()?;
        }
        Compression::Xz => {
            let mut encoder = XzWriter::new(&mut file, XzOptions::with_preset(level.unwrap_or(6)))?;
            encoder.write_all(bytes)?;
            encoder.finish()?;
        }
        Compression::Zstd => {
            let mut encoder =
                zstd::stream::write::Encoder::new(&mut file, level.unwrap_or(3) as i32)?;
            encoder.write_all(bytes)?;
            encoder.finish()?;
        }
    }
    file.flush()?;
    Ok(())
}

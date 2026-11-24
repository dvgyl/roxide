//! Bounded tar archive extraction through the session host.

use std::io::{self, BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};

use bzip2::read::MultiBzDecoder;
use flate2::read::MultiGzDecoder;
use lzma_rust2::XzReader;
use roxide_host::{Host, HostError, HostErrorKind, HostResult, OpenMode};

const BLOCK: usize = 512;

fn invalid(message: &str) -> HostError {
    HostError::new(HostErrorKind::InvalidInput, message)
}

/// Extract regular files and directories from a tar archive into an existing directory.
/// Return the common top-level path. Reject links, special files, and paths outside the root.
pub fn extract(host: &mut dyn Host, archive: &Path, root: &Path) -> HostResult<PathBuf> {
    let mut input = BufReader::new(host.open(archive, OpenMode::read_only())?);
    let magic = input.fill_buf().map_err(HostError::from_io)?;
    let limit = host.limits().max_decompress_bytes;
    let entry_limit = (host.limits().max_vector_bytes / BLOCK).min(100_000);
    let decoder: Box<dyn Read> = if magic.starts_with(&[0x1f, 0x8b]) {
        Box::new(MultiGzDecoder::new(input))
    } else if magic.starts_with(b"BZh") {
        Box::new(MultiBzDecoder::new(input))
    } else if magic.starts_with(b"\xfd7zXZ\0") {
        Box::new(XzReader::new_mem_limit(
            input,
            true,
            (limit / 1024).min(128 * 1024) as u32,
        ))
    } else {
        Box::new(input)
    };
    let mut input = decoder.take((limit as u64).saturating_add(1));
    let root_meta = host.metadata(root, false)?;
    if !root_meta.is_dir || root_meta.is_symlink {
        return Err(invalid("invalid tar extraction directory"));
    }

    let mut top = None;
    let mut global = Pax::default();
    let mut local = Pax::default();
    let mut long_path = None;
    let mut header = [0; BLOCK];
    for _ in 0..=entry_limit {
        input.read_exact(&mut header).map_err(HostError::from_io)?;
        if header == [0; BLOCK] {
            let mut padding = [0; 8192];
            loop {
                let count = input.read(&mut padding).map_err(HostError::from_io)?;
                if input.limit() == 0 || padding[..count].iter().any(|&byte| byte != 0) {
                    return Err(invalid("invalid tar archive padding or size"));
                }
                if count == 0 {
                    break;
                }
            }
            return top.ok_or_else(|| invalid("empty tar archive"));
        }
        let checksum = header
            .iter()
            .enumerate()
            .map(|(i, &b)| usize::from(if (148..156).contains(&i) { b' ' } else { b }))
            .sum::<usize>();
        if number(&header[148..156])? != checksum {
            return Err(invalid("invalid tar header checksum"));
        }
        let kind = header[156];
        let mut size = number(&header[124..136])?;
        if !matches!(kind, b'g' | b'x' | b'L') {
            size = local.size.or(global.size).unwrap_or(size);
        }
        let padded = size
            .div_ceil(BLOCK)
            .checked_mul(BLOCK)
            .ok_or_else(|| invalid("invalid tar entry size"))?;
        if padded as u64 >= input.limit() {
            return Err(invalid("tar archive exceeds the decompression limit"));
        }
        match kind {
            b'x' | b'g' | b'L' => {
                if size > host.limits().max_vector_bytes.min(1024 * 1024) {
                    return Err(invalid("tar metadata exceeds the size limit"));
                }
                let mut data = vec![0; size];
                input.read_exact(&mut data).map_err(HostError::from_io)?;
                match kind {
                    b'L' => long_path = Some(field(&data)?.to_owned()),
                    b'x' => local.update(&data)?,
                    _ => global.update(&data)?,
                }
            }
            0 | b'0' | b'5' | b'7' => {
                let path = local
                    .path
                    .take()
                    .or_else(|| long_path.take())
                    .or_else(|| global.path.clone());
                local = Pax::default();
                long_path = None;
                let path = if let Some(path) = path {
                    path
                } else {
                    let name = field(&header[..100])?;
                    let prefix = if &header[257..263] == b"ustar\0" {
                        field(&header[345..500])?
                    } else {
                        ""
                    };
                    if prefix.is_empty() {
                        name.to_owned()
                    } else {
                        format!("{prefix}/{name}")
                    }
                };
                let components = safe_path(&path)?;
                if let Some(Component::Normal(first)) = components.components().next() {
                    let first = root.join(first);
                    if top.as_ref().is_some_and(|current| current != &first) {
                        return Err(invalid("tar archive has multiple top-level paths"));
                    }
                    top = Some(first);
                } else if kind != b'5' {
                    return Err(invalid("empty tar file path"));
                }
                let path = root.join(&components);
                if kind == b'5' {
                    if size != 0 {
                        return Err(invalid("invalid tar directory size"));
                    }
                    make_directories(host, root, &components)?;
                } else {
                    if let Some(parent) = components.parent() {
                        make_directories(host, root, parent)?;
                    }
                    match host.metadata(&path, false) {
                        Err(err) if err.kind == HostErrorKind::NotFound => {}
                        Err(err) => return Err(err),
                        Ok(_) => return Err(invalid("duplicate tar path")),
                    }
                    let executable_bits = (number(&header[100..108])? & 0o111) as u32;
                    let mut dest = host.open(&path, OpenMode::write_truncate())?;
                    let copied = io::copy(&mut (&mut input).take(size as u64), &mut dest)
                        .map_err(HostError::from_io)?;
                    if copied != size as u64 {
                        return Err(invalid("truncated tar file"));
                    }
                    if executable_bits != 0 {
                        dest.add_executable_bits(executable_bits)?;
                    }
                }
            }
            _ => return Err(invalid("tar links and special files are unsupported")),
        }
        input
            .read_exact(&mut header[..padded - size])
            .map_err(HostError::from_io)?;
    }
    Err(invalid("tar archive exceeds the entry limit"))
}

fn field(bytes: &[u8]) -> HostResult<&str> {
    std::str::from_utf8(bytes.split(|&byte| byte == 0).next().unwrap_or_default())
        .map_err(|_| invalid("invalid UTF-8 in tar header"))
}

fn number(bytes: &[u8]) -> HostResult<usize> {
    if let Some((&first, rest)) = bytes.split_first()
        && first & 0x80 != 0
    {
        return rest.iter().try_fold(usize::from(first & 0x7f), |n, &b| {
            n.checked_mul(256)
                .and_then(|n| n.checked_add(usize::from(b)))
                .ok_or_else(|| invalid("invalid tar number"))
        });
    }
    let value = field(bytes)?.trim();
    if value.is_empty() {
        Ok(0)
    } else {
        usize::from_str_radix(value, 8).map_err(|_| invalid("invalid tar number"))
    }
}

fn safe_path(path: &str) -> HostResult<PathBuf> {
    if path.contains(['\\', '\0', ':']) {
        return Err(invalid("unsafe tar path"));
    }
    let mut safe = PathBuf::new();
    for part in Path::new(path).components() {
        match part {
            Component::Normal(name) => safe.push(name),
            Component::CurDir => {}
            _ => return Err(invalid("unsafe tar path")),
        }
    }
    Ok(safe)
}

fn make_directories(host: &mut dyn Host, root: &Path, path: &Path) -> HostResult<()> {
    let mut current = root.to_path_buf();
    for component in path.components() {
        current.push(component);
        match host.metadata(&current, false) {
            Ok(info) if info.is_dir && !info.is_symlink => {}
            Ok(_) => return Err(invalid("unsafe tar directory")),
            Err(err) if err.kind == HostErrorKind::NotFound => {
                host.create_dir(&current, false, 0o755)?;
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

#[derive(Default)]
struct Pax {
    path: Option<String>,
    size: Option<usize>,
}

impl Pax {
    fn update(&mut self, mut data: &[u8]) -> HostResult<()> {
        while !data.is_empty() {
            let space = data
                .iter()
                .position(|&byte| byte == b' ')
                .ok_or_else(|| invalid("invalid PAX record"))?;
            let length = std::str::from_utf8(&data[..space])
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|length| *length > space + 1 && *length <= data.len())
                .ok_or_else(|| invalid("invalid PAX record size"))?;
            let record = std::str::from_utf8(&data[space + 1..length])
                .ok()
                .and_then(|value| value.strip_suffix('\n'))
                .and_then(|value| value.split_once('='))
                .ok_or_else(|| invalid("invalid PAX record field"))?;
            match record {
                ("path", value) => self.path = (!value.is_empty()).then(|| value.to_owned()),
                ("size", "") => self.size = None,
                ("size", value) => {
                    self.size = Some(value.parse().map_err(|_| invalid("invalid PAX size"))?)
                }
                (key, _) if key.starts_with("GNU.sparse.") => {
                    return Err(invalid("sparse tar files are unsupported"));
                }
                _ => {}
            }
            data = &data[length..];
        }
        Ok(())
    }
}

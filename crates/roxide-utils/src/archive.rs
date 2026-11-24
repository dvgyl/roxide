//! Read and write regular files and directories in POSIX ustar archives.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use roxide_eval::{Args, Call, Ctx, EvalResult, Module};
use roxide_gc::SexpId;
use roxide_host::{FileMeta, Host, HostErrorKind, OpenMode};
use roxide_value::{CharId, Encoding, ValueApi};

use crate::pkgutils::internal;

const BLOCK: usize = 512;

pub(crate) const MODULE: Module = Module {
    prims: &[
        internal("C_rox_tar_pack", 2, pack),
        internal("C_rox_tar_extract", 4, extract),
        internal("C_rox_tar_limit", 0, limit),
    ],
    prelude: include_str!("archive.R"),
};

fn archive_limit(host: &dyn Host) -> usize {
    let limits = host.limits();
    limits
        .max_decompress_bytes
        .min(limits.max_vector_bytes)
        .min(i32::MAX as usize)
}

fn limit(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let size = archive_limit(ctx.host()) as i32;
    Ok(ctx.session_mut().scalar_int(size)?)
}

fn strings(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<String>> {
    let session = ctx.session();
    let ids = session.str_slice(value)?.to_vec();
    ids.into_iter()
        .map(|id| {
            if id == CharId::NA {
                return Err(ctx.stop(roxide_value::SEXP_NIL, "Missing archive path"));
            }
            Ok(String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned())
        })
        .collect()
}

fn pack(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sources = strings(ctx, args.get(0))?;
    let paths = strings(ctx, args.get(1))?;
    let [output] = paths.as_slice() else {
        return Err(ctx.stop(call.err_call(), "Expected one archive path"));
    };
    let data = pack_files(ctx.host(), &sources, Path::new(output))
        .map_err(|message| ctx.stop(call.err_call(), message))?;
    Ok(ctx.session_mut().alloc_raw_from(&data)?)
}

fn pack_files(host: &mut dyn Host, sources: &[String], output: &Path) -> Result<Vec<u8>, String> {
    let limit = archive_limit(host)
        .checked_sub(2 * BLOCK)
        .ok_or("Archive size limit exceeded")?;
    let output = host.canonicalize(output).ok();
    let mut data = Vec::new();
    let mut pending: Vec<_> = sources
        .iter()
        .rev()
        .map(|source| {
            (
                PathBuf::from(source),
                source.trim_start_matches('/').to_owned(),
                true,
            )
        })
        .collect();
    while let Some((path, mut name, top_level)) = pending.pop() {
        if output
            .as_ref()
            .is_some_and(|to| host.canonicalize(&path).as_ref() == Ok(to))
        {
            continue;
        }
        let meta = host
            .metadata(&path, false)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if meta.is_symlink || !(meta.is_dir || meta.is_file) {
            return Err(format!("Unsupported archive input '{}'", path.display()));
        }
        if !safe_name(&name) {
            return Err(format!("Unsafe archive path '{name}'"));
        }
        if meta.is_dir {
            name.push('/');
            split_name(&name)?;
            let mut children = host.read_dir(&path).map_err(|error| error.to_string())?;
            children.sort_unstable_by(|a, b| a.name.cmp(&b.name));
            pending.extend(
                children
                    .into_iter()
                    .rev()
                    .map(|entry| (entry.path, format!("{name}{}", entry.name), false)),
            );
            if top_level {
                continue;
            }
        }
        let size = if meta.is_dir {
            0
        } else {
            usize::try_from(meta.len).map_err(|_| "Archive size limit exceeded")?
        };
        let end = data
            .len()
            .saturating_add(BLOCK)
            .saturating_add(padded(size));
        if end > limit {
            return Err("Archive size limit exceeded".into());
        }
        data.extend(header(&name, &meta, size)?);
        if !meta.is_dir {
            let mut file = host
                .open(&path, OpenMode::read_only())
                .map_err(|error| error.to_string())?;
            let start = data.len();
            data.resize(end, 0);
            file.read_exact(&mut data[start..start + size])
                .map_err(|error| error.to_string())?;
        }
    }
    data.resize(data.len() + 2 * BLOCK, 0);
    Ok(data)
}

fn padded(size: usize) -> usize {
    size.saturating_add(BLOCK - 1) / BLOCK * BLOCK
}

fn split_name(name: &str) -> Result<(&str, &str), String> {
    if name.len() <= 100 {
        return Ok(("", name));
    }
    name.match_indices('/')
        .rev()
        .filter(|&(index, _)| index <= 155 && index + 1 < name.len())
        .map(|(index, _)| (&name[..index], &name[index + 1..]))
        .find(|(_, tail)| tail.len() <= 100)
        .ok_or_else(|| format!("Archive path is too long: '{name}'"))
}

fn octal(field: &mut [u8], value: u64) -> Result<(), String> {
    let width = field.len() - 1;
    let digits = format!("{value:0width$o}");
    if digits.len() > width {
        return Err("Value does not fit in a ustar header".into());
    }
    field[..width].copy_from_slice(digits.as_bytes());
    Ok(())
}

fn header(name: &str, meta: &FileMeta, size: usize) -> Result<[u8; BLOCK], String> {
    let (prefix, tail) = split_name(name)?;
    let mut header = [0; BLOCK];
    header[..tail.len()].copy_from_slice(tail.as_bytes());
    header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    octal(
        &mut header[100..108],
        if meta.is_dir { 0o755 } else { 0o644 },
    )?;
    octal(&mut header[108..116], 0)?;
    octal(&mut header[116..124], 0)?;
    octal(&mut header[124..136], size as u64)?;
    let mtime = (meta.modified_us.unwrap_or(0) / 1_000_000).max(0);
    octal(&mut header[136..148], mtime as u64)?;
    header[148..156].fill(b' ');
    header[156] = if meta.is_dir { b'5' } else { b'0' };
    header[257..265].copy_from_slice(b"ustar\x0000");
    let sum = checksum(&header);
    octal(&mut header[148..155], sum)?;
    Ok(header)
}

fn checksum(header: &[u8]) -> u64 {
    header[..148]
        .iter()
        .chain(&header[156..])
        .map(|&byte| u64::from(byte))
        .sum::<u64>()
        + 8 * u64::from(b' ')
}

fn parse_octal(bytes: &[u8]) -> Result<u64, &'static str> {
    let digits = std::str::from_utf8(bytes).map_err(|_| "Invalid archive header")?;
    let digits = digits.trim_matches(['\0', ' ']);
    if digits.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(digits, 8).map_err(|_| "Invalid archive header")
}

fn path_field(field: &[u8]) -> Result<&str, &'static str> {
    std::str::from_utf8(field.split(|&byte| byte == 0).next().unwrap_or_default())
        .map_err(|_| "Invalid archive path encoding")
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains(['\\', '\0'])
        && !name
            .split('/')
            .any(|part| part == ".." || part.contains(':'))
}

struct Member<'a> {
    name: String,
    data: &'a [u8],
    directory: bool,
}

fn members(mut data: &[u8]) -> Result<Vec<Member<'_>>, String> {
    let mut members = Vec::new();
    while !data.is_empty() {
        let header = data.get(..BLOCK).ok_or("Incomplete archive header")?;
        if header.iter().all(|&byte| byte == 0) {
            if data.iter().all(|&byte| byte == 0) {
                return Ok(members);
            }
            return Err("Invalid archive trailer".into());
        }
        if parse_octal(&header[148..156])? != checksum(header) {
            return Err("Invalid archive header checksum".into());
        }
        let tail = path_field(&header[..100])?;
        let prefix = if &header[257..263] == b"ustar\x00" {
            path_field(&header[345..500])?
        } else {
            ""
        };
        let name = if prefix.is_empty() {
            tail.to_owned()
        } else {
            format!("{prefix}/{tail}")
        };
        if !safe_name(&name) {
            return Err(format!("Unsafe archive path '{name}'"));
        }
        let size = usize::try_from(parse_octal(&header[124..136])?)
            .map_err(|_| "Archive size limit exceeded")?;
        let directory = match header[156] {
            b'0' | 0 => name.ends_with('/'),
            b'5' => true,
            _ => return Err(format!("Unsupported archive entry '{name}'")),
        };
        data = &data[BLOCK..];
        if directory && size != 0 {
            return Err("Invalid archive directory size".into());
        }
        members.push(Member {
            name,
            data: data.get(..size).ok_or("Incomplete archive entry")?,
            directory,
        });
        data = data
            .get(padded(size)..)
            .ok_or("Incomplete archive padding")?;
    }
    Err("Missing archive trailer".into())
}

fn extract(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let files: HashSet<_> = strings(ctx, args.get(1))?
        .into_iter()
        .map(|name| name.trim_end_matches('/').to_owned())
        .collect();
    let paths = strings(ctx, args.get(3))?;
    let [path] = paths.as_slice() else {
        return Err(ctx.stop(call.err_call(), "Expected one extraction directory"));
    };
    let list = ctx
        .session()
        .lgl(args.get(2))?
        .iter()
        .next()
        .and_then(|v| v.get());
    let Some(list) = list else {
        return Err(ctx.stop(call.err_call(), "Invalid 'list' argument"));
    };
    let data = ctx.session().raw(args.get(0))?.iter().collect::<Vec<u8>>();
    let members = members(&data).map_err(|message| ctx.stop(call.err_call(), message))?;
    if list {
        let session = ctx.session_mut();
        let ids = members
            .iter()
            .map(|m| session.mk_char(m.name.as_bytes(), Encoding::Utf8))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(session.alloc_str_from(&ids)?);
    }
    let host = ctx.host();
    let path = Path::new(path);
    let result = (|| {
        if !host.metadata(path, true).is_ok_and(|meta| meta.is_dir) {
            host.create_dir(path, true, 0o755)
                .map_err(|e| e.to_string())?;
        }
        let path = host.canonicalize(path).map_err(|e| e.to_string())?;
        for member in members {
            if files.is_empty() || files.contains(member.name.trim_end_matches('/')) {
                extract_member(host, &path, member)?;
            }
        }
        Ok::<_, String>(())
    })();
    result.map_err(|message| ctx.stop(call.err_call(), message))?;
    Ok(ctx.session_mut().scalar_int(0)?)
}

fn extract_member(host: &mut dyn Host, root: &Path, member: Member<'_>) -> Result<(), String> {
    let mut target = root.to_path_buf();
    let mut components = Path::new(&member.name)
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .peekable();
    if components.peek().is_none() && !member.directory {
        return Err(format!("Invalid archive file '{}'", member.name));
    }
    while let Some(part) = components.next() {
        target.push(part);
        let directory = components.peek().is_some() || member.directory;
        let exists = match host.metadata(&target, false) {
            Ok(meta)
                if !meta.is_symlink && (if directory { meta.is_dir } else { meta.is_file }) =>
            {
                true
            }
            Err(error) if error.kind == HostErrorKind::NotFound => false,
            _ => return Err(format!("Unsafe extraction path '{}'", target.display())),
        };
        if directory {
            if !exists {
                host.create_dir(&target, false, 0o755)
                    .map_err(|error| error.to_string())?;
            }
        } else {
            let mut output = host
                .open(&target, OpenMode::write_truncate())
                .map_err(|error| error.to_string())?;
            output
                .write_all(member.data)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use roxide_host::PureHost;

    #[test]
    fn archive_has_ustar_prefix_padding_and_trailer() {
        let mut host = PureHost::new();
        let name = format!("{}/{}", "a".repeat(120), "b".repeat(95));
        host.insert_file(&name, b"abc");
        let bytes = pack_files(&mut host, std::slice::from_ref(&name), Path::new("out.tar"))
            .expect("Write archive");
        assert_eq!(&bytes[257..265], b"ustar\x0000");
        assert_eq!(bytes.len(), 4 * BLOCK);
        assert!(bytes[BLOCK + 3..].iter().all(|&byte| byte == 0));
        let entries = members(&bytes).expect("Read archive");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, name);
        assert_eq!(entries[0].data, b"abc");
    }

    #[test]
    fn reader_rejects_unsafe_paths_and_invalid_headers() {
        let meta = FileMeta {
            len: 0,
            is_dir: false,
            is_file: true,
            is_symlink: false,
            modified_us: None,
        };
        for name in [
            "../file",
            "dir/../file",
            "/abs/file",
            "C:/file",
            "dir\\file",
        ] {
            let mut bytes = header(name, &meta, 0).expect("Make a header").to_vec();
            bytes.resize(3 * BLOCK, 0);
            assert!(matches!(members(&bytes), Err(e) if e.contains("Unsafe archive path")));
        }
        let mut bytes = header("file", &meta, 0).expect("Make a header").to_vec();
        assert!(matches!(members(&bytes), Err(e) if e.contains("Missing archive trailer")));
        bytes[0] = b'X';
        assert!(matches!(members(&bytes), Err(e) if e.contains("checksum")));
    }
}

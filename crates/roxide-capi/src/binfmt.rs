#![forbid(unsafe_code)]

//! Check shared-library dependencies before loading a native image.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const CPU: u32 = if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    0x0100_0007
} else {
    0x0100_000c
};
const MAX_METADATA_SIZE: u64 = 16 * 1024 * 1024;
const MAX_IMAGE_READ: u64 = 32 * 1024 * 1024;
const MAX_CPU_SLICES: usize = 8;

#[derive(Debug)]
pub(crate) enum Error {
    Invalid,
    Foreign(Vec<u8>),
    Read(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => f.write_str("invalid or unsupported shared-library format"),
            Self::Foreign(name) => write!(
                f,
                "foreign R dependency '{}'; reinstall from source with Roxide",
                String::from_utf8_lossy(name)
            ),
            Self::Read(err) => err.fmt(f),
        }
    }
}

pub(crate) fn check(
    file: &mut (impl Read + Seek),
    len: u64,
    libr: Option<&Path>,
    canonicalize: impl Fn(&Path) -> Option<PathBuf>,
) -> Result<(), Error> {
    let image = Region { offset: 0, len };
    let mut reader = ImageReader {
        file,
        remaining: MAX_IMAGE_READ,
    };
    let magic = image.part(0, 4)?.read(&mut reader)?;
    let elf_image = magic == b"\x7fELF";
    let dependency = |name: &[u8]| {
        let base = name.rsplit(|b| *b == b'/').next().unwrap_or(name);
        let framework = name
            .split(|b| *b == b'/')
            .any(|part| part == b"R.framework");
        let r_library = (base.starts_with(b"libR.") && base.ends_with(b".dylib"))
            || base == b"libR.dylib"
            || base == b"libR.so"
            || base.starts_with(b"libR.so.");
        let path = Path::new(OsStr::from_bytes(name));
        let allowed_runtime = || {
            libr.is_some_and(|libr| {
                (elf_image && name == b"libR.so")
                    || (path.is_absolute() && canonicalize(path).as_deref() == Some(libr))
            })
        };
        if base.starts_with(b"libRblas")
            || base.starts_with(b"libRlapack")
            || ((framework || r_library) && !allowed_runtime())
        {
            return Err(Error::Foreign(name.to_vec()));
        }
        Ok(())
    };
    if elf_image {
        elf(&mut reader, image, dependency)
    } else {
        mach_image(&mut reader, image, &magic, dependency)
    }
}

struct ImageReader<R> {
    file: R,
    remaining: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Region {
    offset: u64,
    len: u64,
}

impl Region {
    fn part(self, offset: u64, len: u64) -> Result<Self, Error> {
        if offset.checked_add(len).is_none_or(|end| end > self.len) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            offset: self.offset + offset,
            len,
        })
    }

    fn read(self, reader: &mut ImageReader<impl Read + Seek>) -> Result<Vec<u8>, Error> {
        if self.len > MAX_METADATA_SIZE || self.len > reader.remaining {
            return Err(Error::Invalid);
        }
        reader.remaining -= self.len;
        let mut bytes = vec![0; self.len as usize];
        reader
            .file
            .seek(SeekFrom::Start(self.offset))
            .map_err(Error::Read)?;
        reader.file.read_exact(&mut bytes).map_err(Error::Read)?;
        Ok(bytes)
    }
}

#[derive(Clone, Copy)]
struct Words<'a> {
    bytes: &'a [u8],
    le: bool,
}

impl Words<'_> {
    fn read<const N: usize>(self, at: usize) -> Result<[u8; N], Error> {
        let mut bytes = self
            .bytes
            .get(at..at + N)
            .and_then(|bytes| <[u8; N]>::try_from(bytes).ok())
            .ok_or(Error::Invalid)?;
        if !self.le {
            bytes.reverse();
        }
        Ok(bytes)
    }

    fn u16(self, at: usize) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.read(at)?))
    }

    fn u32(self, at: usize) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.read(at)?))
    }

    fn u64(self, at: usize) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.read(at)?))
    }
}

fn c_string(bytes: &[u8], offset: usize) -> Result<&[u8], Error> {
    let tail = bytes.get(offset..).ok_or(Error::Invalid)?;
    let end = tail
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(Error::Invalid)?;
    if end == 0 {
        return Err(Error::Invalid);
    }
    Ok(&tail[..end])
}

fn mach_image(
    file: &mut ImageReader<impl Read + Seek>,
    image: Region,
    magic: &[u8],
    dependency: impl Fn(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let (le, wide) = match magic {
        [0xca, 0xfe, 0xba, 0xbe] => (false, false),
        [0xbe, 0xba, 0xfe, 0xca] => (true, false),
        [0xca, 0xfe, 0xba, 0xbf] => (false, true),
        [0xbf, 0xba, 0xfe, 0xca] => (true, true),
        _ => return mach(file, image, None, &dependency),
    };
    let count = image.part(4, 4)?.read(file)?;
    let count = u64::from(Words { bytes: &count, le }.u32(0)?);
    let stride = if wide { 32 } else { 20 };
    let table = image.part(8, count * stride)?.read(file)?;
    let mut regions = HashSet::new();
    let mut matching = Vec::new();
    for entry in table.chunks_exact(stride as usize) {
        let entry = Words { bytes: entry, le };
        let (start, size) = if wide {
            (entry.u64(8)?, entry.u64(16)?)
        } else {
            (u64::from(entry.u32(8)?), u64::from(entry.u32(12)?))
        };
        let slice = image.part(start, size)?;
        if !regions.insert(slice) {
            return Err(Error::Invalid);
        }
        if entry.u32(0)? == CPU {
            if matching.len() == MAX_CPU_SLICES {
                return Err(Error::Invalid);
            }
            matching.push(slice);
        }
    }
    if matching.is_empty() {
        return Err(Error::Invalid);
    }
    for slice in matching {
        mach(file, slice, Some(CPU), &dependency)?;
    }
    Ok(())
}

fn mach(
    file: &mut ImageReader<impl Read + Seek>,
    image: Region,
    cpu: Option<u32>,
    dependency: &impl Fn(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let header = image.part(0, 32)?.read(file)?;
    let le = match header[..4] {
        [0xcf, 0xfa, 0xed, 0xfe] => true,
        [0xfe, 0xed, 0xfa, 0xcf] => false,
        _ => return Err(Error::Invalid),
    };
    let header = Words { bytes: &header, le };
    if !matches!(header.u32(12)?, 6 | 8) || cpu.is_some_and(|cpu| header.u32(4).ok() != Some(cpu)) {
        return Err(Error::Invalid);
    }
    let commands = image.part(32, u64::from(header.u32(20)?))?.read(file)?;
    let mut commands = commands.as_slice();
    for _ in 0..header.u32(16)? {
        let command = Words {
            bytes: commands,
            le,
        };
        let size = command.u32(4)? as usize;
        if size < 8 || !size.is_multiple_of(8) {
            return Err(Error::Invalid);
        }
        let (bytes, rest) = commands.split_at_checked(size).ok_or(Error::Invalid)?;
        let command = Words { bytes, le };
        if matches!(
            command.u32(0)?,
            0xc | 0x8000_0018 | 0x8000_001f | 0x20 | 0x8000_0023
        ) {
            let offset = command.u32(8)? as usize;
            if offset < 24 {
                return Err(Error::Invalid);
            }
            dependency(c_string(bytes, offset)?)?;
        }
        commands = rest;
    }
    if !commands.is_empty() {
        return Err(Error::Invalid);
    }
    Ok(())
}

fn elf(
    file: &mut ImageReader<impl Read + Seek>,
    image: Region,
    dependency: impl Fn(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let header = image.part(0, 64)?.read(file)?;
    let le = match header[5] {
        1 => true,
        2 => false,
        _ => return Err(Error::Invalid),
    };
    let header = Words { bytes: &header, le };
    if header.bytes[4] != 2 || header.bytes[6] != 1 || header.u16(16)? != 3 || header.u32(20)? != 1
    {
        return Err(Error::Invalid);
    }
    let stride = u64::from(header.u16(54)?);
    let count = u64::from(header.u16(56)?);
    if stride != 56 || count == 0xffff {
        return Err(Error::Invalid);
    }
    let headers = image.part(header.u64(32)?, stride * count)?.read(file)?;
    let mut loads = Vec::new();
    let mut dynamic = None;
    for bytes in headers.chunks_exact(stride as usize) {
        let ph = Words { bytes, le };
        let segment_type = ph.u32(0)?;
        if matches!(segment_type, 1 | 2) {
            let segment = image.part(ph.u64(8)?, ph.u64(32)?)?;
            let address = ph.u64(16)?;
            let memory_size = ph.u64(40)?;
            if segment.len > memory_size || address.checked_add(memory_size).is_none() {
                return Err(Error::Invalid);
            }
            if segment_type == 1 {
                loads.push((address, segment));
            } else if dynamic.replace((address, segment)).is_some() {
                return Err(Error::Invalid);
            }
        }
    }
    let mapped = |address: u64, size: u64| {
        let mut matches = loads
            .iter()
            .filter_map(|&(base, segment)| segment.part(address.checked_sub(base)?, size).ok());
        let found = matches.next().ok_or(Error::Invalid)?;
        if matches.next().is_some() {
            return Err(Error::Invalid);
        }
        Ok(found)
    };
    let (address, entries) = dynamic.ok_or(Error::Invalid)?;
    if mapped(address, entries.len)? != entries {
        return Err(Error::Invalid);
    }
    let entries = entries.read(file)?;
    let mut needed = Vec::new();
    let mut address = None;
    let mut size = None;
    for bytes in entries.as_chunks::<16>().0 {
        let entry = Words { bytes, le };
        let value = entry.u64(8)?;
        match entry.u64(0)? {
            0 if needed.is_empty() => return Ok(()),
            0 => {
                let strings = mapped(address.ok_or(Error::Invalid)?, size.ok_or(Error::Invalid)?)?
                    .read(file)?;
                for offset in needed {
                    dependency(c_string(&strings, offset)?)?;
                }
                return Ok(());
            }
            1 => needed.push(usize::try_from(value).map_err(|_| Error::Invalid)?),
            5 if address.replace(value).is_some() => return Err(Error::Invalid),
            10 if size.replace(value).is_some() => return Err(Error::Invalid),
            _ => {}
        }
    }
    Err(Error::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(bytes: &[u8], libr: Option<&[u8]>) -> Result<(), Error> {
        let libr = libr.map(|name| Path::new(OsStr::from_bytes(name)));
        super::check(
            &mut io::Cursor::new(bytes),
            bytes.len() as u64,
            libr,
            |path| Some(path.to_path_buf()),
        )
    }

    fn thin(name: &[u8], command: u32) -> Vec<u8> {
        let size = (24 + name.len() + 1).next_multiple_of(8);
        let mut data = vec![0; 32 + size];
        data[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        for (at, word) in [
            (4, CPU),
            (12, 6),
            (16, 1),
            (20, size as u32),
            (32, command),
            (36, size as u32),
            (40, 24),
        ] {
            data[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        data[56..56 + name.len()].copy_from_slice(name);
        data
    }

    fn fat(slices: &[&[u8]]) -> Vec<u8> {
        let mut data = vec![0; 8 + 20 * slices.len()];
        data[..4].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe]);
        data[4..8].copy_from_slice(&(slices.len() as u32).to_be_bytes());
        for (index, thin) in slices.iter().enumerate() {
            let cpu = u32::from_le_bytes(thin[4..8].try_into().unwrap());
            let offset = data.len() as u32;
            let at = 8 + index * 20;
            for (at, word) in [
                (at, cpu),
                (at + 4, index as u32),
                (at + 8, offset),
                (at + 12, thin.len() as u32),
            ] {
                data[at..at + 4].copy_from_slice(&word.to_be_bytes());
            }
            data.extend_from_slice(thin);
        }
        data
    }

    fn elf_fixture(name: &[u8]) -> Vec<u8> {
        let mut data = vec![0; 241 + name.len()];
        data[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        for (at, word) in [(16, 3u16), (54, 56), (56, 2)] {
            data[at..at + 2].copy_from_slice(&word.to_le_bytes());
        }
        data[20] = 1;
        data[32] = 64;
        data[64] = 1;
        data[120] = 2;
        let len = data.len() as u64;
        for (at, word) in [
            (96, len),
            (104, len),
            (128, 176),
            (136, 176),
            (152, 64),
            (160, 64),
            (176, 1),
            (192, 5),
            (200, 240),
            (208, 10),
            (216, name.len() as u64 + 1),
        ] {
            data[at..at + 8].copy_from_slice(&word.to_le_bytes());
        }
        data[240..240 + name.len()].copy_from_slice(name);
        data
    }

    #[test]
    fn macho_checks_all_dependency_commands_and_universal_slice() {
        let foreign = b"/Library/Frameworks/R.framework/Versions/4.6/Resources/lib/libR.dylib";
        for command in [0xc, 0x8000_0018, 0x8000_001f, 0x20, 0x8000_0023] {
            let bytes = thin(foreign, command);
            assert!(matches!(check(&bytes, None), Err(Error::Foreign(name)) if name == foreign));
            assert!(
                matches!(check(&fat(&[&bytes]), None), Err(Error::Foreign(name)) if name == foreign)
            );
        }
        let canonical = b"/roxide/lib/libR.dylib";
        assert!(check(&thin(canonical, 0xc), Some(canonical)).is_ok());
        assert!(matches!(
            check(&thin(b"@rpath/libR.dylib", 0xc), Some(canonical)),
            Err(Error::Foreign(name)) if name == b"@rpath/libR.dylib"
        ));
        let ordinary = thin(b"/usr/lib/OtherR.framework/libSystem.B.dylib", 0xc);
        assert!(check(&fat(&[&ordinary, &ordinary]), None).is_ok());
        let other = thin(foreign, 0xc);
        assert!(
            matches!(check(&fat(&[&ordinary, &other]), None), Err(Error::Foreign(name)) if name == foreign)
        );
    }

    #[test]
    fn fat_rejects_duplicate_and_excess_matching_slices_before_reading_bodies() {
        let thin = thin(b"/usr/lib/libSystem.B.dylib", 0xc);
        let mut duplicate = fat(&[&thin, &thin]);
        duplicate.copy_within(16..24, 36);
        let mut mixed_cpu_duplicate = duplicate.clone();
        mixed_cpu_duplicate[28..32].copy_from_slice(&(CPU ^ 1).to_be_bytes());
        let excessive = fat(&[thin.as_slice(); MAX_CPU_SLICES + 1]);

        for (bytes, table_end) in [
            (&duplicate, 8 + 2 * 20),
            (&mixed_cpu_duplicate, 8 + 2 * 20),
            (&excessive, 8 + (MAX_CPU_SLICES + 1) * 20),
        ] {
            let mut reader = io::Cursor::new(bytes);
            assert!(matches!(
                super::check(&mut reader, bytes.len() as u64, None, |_| None),
                Err(Error::Invalid)
            ));
            assert_eq!(reader.position(), table_end as u64);
        }
        assert!(check(&fat(&[thin.as_slice(); MAX_CPU_SLICES]), None).is_ok());
    }

    #[test]
    fn metadata_reads_share_one_budget() {
        let mut reader = ImageReader {
            file: io::Cursor::new([0; 12]),
            remaining: 8,
        };
        let first = Region { offset: 0, len: 6 };
        let second = Region { offset: 6, len: 6 };
        assert_eq!(first.read(&mut reader).unwrap().len(), 6);
        assert!(matches!(second.read(&mut reader), Err(Error::Invalid)));
        assert_eq!(reader.file.position(), 6);
    }

    #[test]
    fn elf_checks_dependencies_without_section_headers() {
        assert!(check(&elf_fixture(b"libc.so.6"), None).is_ok());
        for name in [
            b"libR.so.4".as_slice(),
            b"libRblas.so",
            b"libRlapack.so",
            b"/gnu/libR.so",
        ] {
            assert!(
                matches!(check(&elf_fixture(name), None), Err(Error::Foreign(dep)) if dep == name)
            );
        }
        let canonical = b"/roxide/lib/libR.so";
        assert!(check(&elf_fixture(canonical), Some(canonical)).is_ok());
        assert!(check(&elf_fixture(b"libR.so"), Some(canonical)).is_ok());
        assert!(matches!(
            check(&elf_fixture(b"libR.so"), None),
            Err(Error::Foreign(_))
        ));
        let linked = elf_fixture(b"/alias/libR.so");
        assert!(
            super::check(
                &mut io::Cursor::new(&linked),
                linked.len() as u64,
                Some(Path::new("/roxide/lib/libR.so")),
                |path| (path == Path::new("/alias/libR.so")).then(|| "/roxide/lib/libR.so".into())
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_truncated_images_and_handles_arbitrary_mutations() {
        let mach = thin(b"libSystem.dylib", 0xc);
        for bytes in [&mach, &fat(&[&mach]), &elf_fixture(b"libc.so.6")] {
            for end in 0..bytes.len() {
                assert!(
                    matches!(check(&bytes[..end], None), Err(Error::Invalid)),
                    "prefix {end}"
                );
            }
            let mut input = bytes.clone();
            for index in 0..bytes.len() {
                for byte in [0, 0x7f, 0xff] {
                    input[index] = byte;
                    let _ = check(&input, None);
                }
                input[index] = bytes[index];
            }
        }
    }
}

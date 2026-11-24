#![forbid(unsafe_code)]

//! Check shared-library dependencies before loading a native image.

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error<'a> {
    Invalid,
    Foreign(&'a [u8]),
}

impl fmt::Display for Error<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => f.write_str("invalid or unsupported shared-library format"),
            Self::Foreign(name) => write!(
                f,
                "foreign R dependency '{}'; reinstall from source with Roxide",
                String::from_utf8_lossy(name)
            ),
        }
    }
}

pub(crate) fn check<'a>(data: &'a [u8], libr: Option<&[u8]>) -> Result<(), Error<'a>> {
    let dependencies = if data.starts_with(b"\x7fELF") {
        elf(data)
    } else {
        mach(universal_slice(data).ok_or(Error::Invalid)?)
    }
    .ok_or(Error::Invalid)?;
    for name in dependencies {
        let base = name.rsplit(|b| *b == b'/').next().unwrap_or(name);
        let r_library = (base.starts_with(b"libR.") && base.ends_with(b".dylib"))
            || base == b"libR.dylib"
            || base == b"libR.so"
            || base.starts_with(b"libR.so.");
        if name
            .windows(b"R.framework".len())
            .any(|w| w == b"R.framework")
            || base.starts_with(b"libRblas")
            || base.starts_with(b"libRlapack")
            || (r_library && Some(name) != libr)
        {
            return Err(Error::Foreign(name));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Image<'a> {
    bytes: &'a [u8],
    le: bool,
}

impl Image<'_> {
    fn u16(self, at: usize) -> Option<u16> {
        let bytes = range(self.bytes, at, 2)?.try_into().ok()?;
        Some(if self.le {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    }

    fn u32(self, at: usize) -> Option<u32> {
        let bytes = range(self.bytes, at, 4)?.try_into().ok()?;
        Some(if self.le {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }

    fn u64(self, at: usize) -> Option<u64> {
        let bytes = range(self.bytes, at, 8)?.try_into().ok()?;
        Some(if self.le {
            u64::from_le_bytes(bytes)
        } else {
            u64::from_be_bytes(bytes)
        })
    }
}

fn range(bytes: &[u8], start: usize, len: usize) -> Option<&[u8]> {
    bytes.get(start..start.checked_add(len)?)
}

fn c_string(bytes: &[u8], offset: usize) -> Option<&[u8]> {
    let tail = bytes.get(offset..)?;
    let end = tail.iter().position(|&byte| byte == 0)?;
    (end > 0).then(|| &tail[..end])
}

fn universal_slice(bytes: &[u8]) -> Option<&[u8]> {
    let (le, wide) = match bytes.get(..4)? {
        [0xca, 0xfe, 0xba, 0xbe] => (false, false),
        [0xbe, 0xba, 0xfe, 0xca] => (true, false),
        [0xca, 0xfe, 0xba, 0xbf] => (false, true),
        [0xbf, 0xba, 0xfe, 0xca] => (true, true),
        _ => return Some(bytes),
    };
    let image = Image { bytes, le };
    let stride = if wide { 32 } else { 20 };
    let len = (image.u32(4)? as usize).checked_mul(stride)?;
    let mut selected = None;
    let cpu = if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        0x0100_0007
    } else {
        0x0100_000c
    };
    for entry in range(bytes, 8, len)?.chunks_exact(stride) {
        let entry = Image { bytes: entry, le };
        let (start, size) = if wide {
            (
                usize::try_from(entry.u64(8)?).ok()?,
                usize::try_from(entry.u64(16)?).ok()?,
            )
        } else {
            (entry.u32(8)? as usize, entry.u32(12)? as usize)
        };
        let slice = range(bytes, start, size)?;
        if entry.u32(0)? == cpu {
            // Reject images with ambiguous CPU subtypes. The loader could select either.
            if selected.replace(slice).is_some() {
                return None;
            }
            let le = match slice.get(..4)? {
                [0xcf, 0xfa, 0xed, 0xfe] => true,
                [0xfe, 0xed, 0xfa, 0xcf] => false,
                _ => return None,
            };
            if (Image { bytes: slice, le }).u32(4)? != cpu {
                return None;
            }
        }
    }
    selected
}

fn mach(bytes: &[u8]) -> Option<Vec<&[u8]>> {
    let le = match bytes.get(..4)? {
        [0xcf, 0xfa, 0xed, 0xfe] => true,
        [0xfe, 0xed, 0xfa, 0xcf] => false,
        _ => return None,
    };
    let header = Image { bytes, le };
    if !matches!(header.u32(12)?, 6 | 8) {
        return None;
    }
    let mut commands = range(bytes, 32, header.u32(20)? as usize)?;
    let mut needed = Vec::new();
    for _ in 0..header.u32(16)? {
        let command = Image {
            bytes: commands,
            le,
        };
        let size = command.u32(4)? as usize;
        if size < 8 || !size.is_multiple_of(8) {
            return None;
        }
        let (bytes, rest) = commands.split_at_checked(size)?;
        if matches!(
            command.u32(0)?,
            0xc | 0x8000_0018 | 0x8000_001f | 0x20 | 0x8000_0023
        ) {
            let offset = command.u32(8)? as usize;
            if offset < 24 {
                return None;
            }
            needed.push(c_string(bytes, offset)?);
        }
        commands = rest;
    }
    commands.is_empty().then_some(needed)
}

fn elf(bytes: &[u8]) -> Option<Vec<&[u8]>> {
    let le = match *bytes.get(5)? {
        1 => true,
        2 => false,
        _ => return None,
    };
    let image = Image { bytes, le };
    if bytes.get(4..7)? != [2, bytes[5], 1] || image.u16(16)? != 3 || image.u32(20)? != 1 {
        return None;
    }
    let stride = usize::from(image.u16(54)?);
    let count = usize::from(image.u16(56)?);
    if stride != 56 || count == 0xffff {
        return None;
    }
    let headers = range(
        bytes,
        usize::try_from(image.u64(32)?).ok()?,
        stride.checked_mul(count)?,
    )?;
    let mut loads = Vec::new();
    let mut dynamic = None;
    for bytes_of_header in headers.chunks_exact(stride) {
        let ph = Image {
            bytes: bytes_of_header,
            le,
        };
        let segment_type = ph.u32(0)?;
        if matches!(segment_type, 1 | 2) {
            let segment = range(
                bytes,
                usize::try_from(ph.u64(8)?).ok()?,
                usize::try_from(ph.u64(32)?).ok()?,
            )?;
            let address = ph.u64(16)?;
            if ph.u64(32)? > ph.u64(40)? || address.checked_add(ph.u64(40)?).is_none() {
                return None;
            }
            if segment_type == 1 {
                loads.push((address, segment));
            } else if dynamic.replace((address, segment)).is_some() {
                return None;
            }
        }
    }
    let mapped = |address: u64, size: usize| {
        let mut matches = loads.iter().filter_map(|&(base, segment)| {
            range(
                segment,
                usize::try_from(address.checked_sub(base)?).ok()?,
                size,
            )
        });
        let found = matches.next()?;
        matches.next().is_none().then_some(found)
    };
    let (dynamic_address, entries) = dynamic?;
    if !std::ptr::eq(mapped(dynamic_address, entries.len())?, entries) {
        return None;
    }
    let mut needed = Vec::new();
    let mut address: Option<u64> = None;
    let mut size = None;
    for bytes in entries.as_chunks::<16>().0 {
        let entry = Image { bytes, le };
        let value = entry.u64(8)?;
        match entry.u64(0)? {
            0 if needed.is_empty() => return Some(Vec::new()),
            0 => {
                let address = address?;
                let size = size?;
                let strings_table = mapped(address, size)?;
                return needed
                    .into_iter()
                    .map(|offset| c_string(strings_table, offset))
                    .collect();
            }
            1 => needed.push(usize::try_from(value).ok()?),
            5 if address.replace(value).is_some() => return None,
            10 if size.replace(usize::try_from(value).ok()?).is_some() => return None,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thin(name: &[u8], command: u32) -> Vec<u8> {
        let size = (24 + name.len() + 1).next_multiple_of(8);
        let mut data = vec![0; 32 + size];
        data[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        let cpu: u32 = if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            0x0100_0007
        } else {
            0x0100_000c
        };
        for (at, word) in [
            (4, cpu),
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

    fn fat(thin: &[u8]) -> Vec<u8> {
        let mut data = vec![0; 28];
        data[..4].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe]);
        let cpu = u32::from_le_bytes(thin[4..8].try_into().unwrap());
        for (at, word) in [(4, 1), (8, cpu), (16, 28), (20, thin.len() as u32)] {
            data[at..at + 4].copy_from_slice(&word.to_be_bytes());
        }
        data.extend_from_slice(thin);
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
            assert_eq!(check(&bytes, None), Err(Error::Foreign(foreign)));
            assert_eq!(check(&fat(&bytes), None), Err(Error::Foreign(foreign)));
        }
        let canonical = b"/roxide/lib/libR.dylib";
        assert_eq!(check(&thin(canonical, 0xc), Some(canonical)), Ok(()));
        assert_eq!(
            check(&thin(b"@rpath/libR.dylib", 0xc), Some(canonical)),
            Err(Error::Foreign(b"@rpath/libR.dylib"))
        );
        assert_eq!(
            check(&fat(&thin(b"/usr/lib/libSystem.B.dylib", 0xc)), None),
            Ok(())
        );
    }

    #[test]
    fn elf_checks_dependencies_without_section_headers() {
        assert_eq!(check(&elf_fixture(b"libc.so.6"), None), Ok(()));
        for name in [
            b"libR.so.4".as_slice(),
            b"libRblas.so",
            b"libRlapack.so",
            b"/gnu/libR.so",
        ] {
            assert_eq!(check(&elf_fixture(name), None), Err(Error::Foreign(name)));
        }
        let canonical = b"/roxide/lib/libR.so";
        assert_eq!(check(&elf_fixture(canonical), Some(canonical)), Ok(()));
    }

    #[test]
    fn rejects_truncated_images_and_handles_arbitrary_mutations() {
        let mach = thin(b"libSystem.dylib", 0xc);
        for bytes in [&mach, &fat(&mach), &elf_fixture(b"libc.so.6")] {
            for end in 0..bytes.len() {
                assert_eq!(
                    check(&bytes[..end], None),
                    Err(Error::Invalid),
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

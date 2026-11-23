//! Real OS-backed [`Host`] implementation for CLI execution.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::{
    CommandSpec, CpuTimes, DirEntry, FileMeta, Host, HostError, HostErrorKind, HostFile,
    HostResult, Limits, LocaleInfo, OpenMode, ProcessOutput, StdStream, SysInfo, WallTime,
};

/// Native OS implementation of [`Host`].
pub struct NativeHost {
    start: Instant,
    temp_dir: PathBuf,
    limits: Limits,
    /// Overlay environment map so `Sys.setenv` avoids `unsafe` `std::env::set_var` in Rust 2024.
    env_overlay: BTreeMap<String, Option<Vec<u8>>>,
}

impl NativeHost {
    /// Construct a new [`NativeHost`] with default limits and a session temp directory.
    #[must_use]
    pub fn new() -> Self {
        let temp_dir = std::env::temp_dir().join(format!("Rtmp-roxide-{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        Self {
            start: Instant::now(),
            temp_dir,
            limits: Limits::default(),
            env_overlay: BTreeMap::new(),
        }
    }

    /// Construct a [`NativeHost`] with explicit [`Limits`].
    #[must_use]
    pub fn with_limits(limits: Limits) -> Self {
        let mut host = Self::new();
        host.limits = limits;
        host
    }
}

impl Default for NativeHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for NativeHost {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

impl Host for NativeHost {
    fn write_out(&mut self, bytes: &[u8]) -> HostResult<()> {
        io::stdout().write_all(bytes).map_err(HostError::from_io)
    }

    fn write_err(&mut self, bytes: &[u8]) -> HostResult<()> {
        io::stderr().write_all(bytes).map_err(HostError::from_io)
    }

    fn flush(&mut self, stream: StdStream) -> HostResult<()> {
        match stream {
            StdStream::Stdout => io::stdout().flush().map_err(HostError::from_io),
            StdStream::Stderr => io::stderr().flush().map_err(HostError::from_io),
            StdStream::Stdin => Ok(()),
        }
    }

    fn read_line(&mut self, buf: &mut Vec<u8>) -> HostResult<usize> {
        read_line_bounded(io::stdin().lock(), buf, self.limits.max_vector_bytes)
    }

    fn is_tty(&self, stream: StdStream) -> bool {
        match stream {
            StdStream::Stdin => io::stdin().is_terminal(),
            StdStream::Stdout => io::stdout().is_terminal(),
            StdStream::Stderr => io::stderr().is_terminal(),
        }
    }

    fn open(&mut self, path: &Path, mode: OpenMode) -> HostResult<Box<dyn HostFile>> {
        let file = OpenOptions::new()
            .read(mode.read)
            .write(mode.write)
            .create(mode.create)
            .truncate(mode.truncate)
            .append(mode.append)
            .open(path)
            .map_err(HostError::from_io)?;
        Ok(Box::new(file))
    }

    fn metadata(&self, path: &Path, follow: bool) -> HostResult<FileMeta> {
        let meta = if follow {
            fs::metadata(path)
        } else {
            fs::symlink_metadata(path)
        }
        .map_err(HostError::from_io)?;
        let modified_us = meta.modified().ok().and_then(|t| {
            t.duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|d| i64::try_from(d.as_micros()).ok())
        });
        Ok(FileMeta {
            len: meta.len(),
            is_dir: meta.is_dir(),
            is_file: meta.is_file(),
            is_symlink: meta.file_type().is_symlink(),
            modified_us,
        })
    }

    fn read_dir(&self, path: &Path) -> HostResult<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path).map_err(HostError::from_io)? {
            let entry = entry.map_err(HostError::from_io)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let entry_path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            entries.push(DirEntry {
                name,
                path: entry_path,
                is_dir,
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn create_dir(&mut self, path: &Path, recursive: bool, _mode: u32) -> HostResult<()> {
        if recursive {
            fs::create_dir_all(path).map_err(HostError::from_io)
        } else {
            fs::create_dir(path).map_err(HostError::from_io)
        }
    }

    fn remove(&mut self, path: &Path, recursive: bool) -> HostResult<()> {
        let meta = fs::symlink_metadata(path).map_err(HostError::from_io)?;
        if meta.is_dir() {
            if recursive {
                fs::remove_dir_all(path).map_err(HostError::from_io)
            } else {
                fs::remove_dir(path).map_err(HostError::from_io)
            }
        } else {
            fs::remove_file(path).map_err(HostError::from_io)
        }
    }

    fn rename(&mut self, from: &Path, to: &Path) -> HostResult<()> {
        fs::rename(from, to).map_err(HostError::from_io)
    }

    fn canonicalize(&self, path: &Path) -> HostResult<PathBuf> {
        fs::canonicalize(path).map_err(HostError::from_io)
    }

    fn cwd(&self) -> HostResult<PathBuf> {
        std::env::current_dir().map_err(HostError::from_io)
    }

    fn set_cwd(&mut self, path: &Path) -> HostResult<()> {
        std::env::set_current_dir(path).map_err(HostError::from_io)
    }

    fn session_temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    fn home_dir(&self) -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    fn env_var(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(overlay) = self.env_overlay.get(key) {
            return overlay.clone();
        }
        std::env::var_os(key).map(|v| v.as_encoded_bytes().to_vec())
    }

    fn set_env_var(&mut self, key: &str, value: Option<&[u8]>) -> HostResult<()> {
        self.env_overlay
            .insert(key.to_owned(), value.map(<[u8]>::to_vec));
        Ok(())
    }

    fn env_vars(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut merged: BTreeMap<Vec<u8>, Vec<u8>> = std::env::vars_os()
            .map(|(k, v)| (k.as_encoded_bytes().to_vec(), v.as_encoded_bytes().to_vec()))
            .collect();
        for (k, v) in &self.env_overlay {
            let kb = k.as_bytes().to_vec();
            match v {
                Some(val) => {
                    merged.insert(kb, val.clone());
                }
                None => {
                    merged.remove(&kb);
                }
            }
        }
        merged.into_iter().collect()
    }

    fn locale(&self) -> LocaleInfo {
        let ctype = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_CTYPE"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_else(|_| String::from("en_US.UTF-8"));
        let utf8 = ctype.to_ascii_uppercase().contains("UTF-8")
            || ctype.to_ascii_uppercase().contains("UTF8");
        LocaleInfo { ctype, utf8 }
    }

    fn tzif(&self, zone: &str) -> HostResult<Vec<u8>> {
        if zone.contains("..") || zone.starts_with('/') {
            return Err(HostError::new(
                HostErrorKind::InvalidInput,
                format!("invalid time zone '{zone}'"),
            ));
        }
        let path = Path::new("/usr/share/zoneinfo").join(zone);
        fs::read(path).map_err(HostError::from_io)
    }

    fn now(&self) -> WallTime {
        let unix_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_micros()).ok())
            .unwrap_or(0);
        WallTime { unix_us }
    }

    fn monotonic(&self) -> Duration {
        self.start.elapsed()
    }

    fn cpu_times(&self) -> CpuTimes {
        let elapsed = self.start.elapsed().as_secs_f64();
        CpuTimes {
            user_secs: elapsed,
            system_secs: 0.0,
            elapsed_secs: elapsed,
        }
    }

    fn sleep(&mut self, duration: Duration) -> HostResult<()> {
        std::thread::sleep(duration);
        Ok(())
    }

    fn pid(&self) -> u32 {
        std::process::id()
    }

    fn fill_entropy(&mut self, buf: &mut [u8]) {
        // Seed from OS time + pid + monotonic nanos via splitmix64 (zero unsafe).
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |d| d.as_nanos());
        let mut state = (now_ns as u64)
            ^ ((now_ns >> 64) as u64)
            ^ u64::from(std::process::id()).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (self.start.elapsed().as_nanos() as u64);
        for chunk in buf.chunks_mut(8) {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let bytes = z.to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }

    fn sys_info(&self) -> SysInfo {
        SysInfo::default()
    }

    fn spawn(&mut self, spec: &CommandSpec) -> HostResult<ProcessOutput> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &self.env_overlay {
            match v {
                Some(bytes) => {
                    if let Ok(s) = std::str::from_utf8(bytes) {
                        cmd.env(k, s);
                    }
                }
                None => {
                    cmd.env_remove(k);
                }
            }
        }
        if spec.stdin.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        let mut child = cmd.spawn().map_err(HostError::from_io)?;
        if let Some(input) = &spec.stdin
            && let Some(mut stdin) = child.stdin.take()
        {
            stdin.write_all(input).map_err(HostError::from_io)?;
        }
        let output = child.wait_with_output().map_err(HostError::from_io)?;
        Ok(ProcessOutput {
            status: output.status.code().unwrap_or(1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn poll_interrupt(&mut self) -> bool {
        false
    }

    fn limits(&self) -> &Limits {
        &self.limits
    }
}

fn read_line_bounded(input: impl BufRead, buf: &mut Vec<u8>, limit: usize) -> HostResult<usize> {
    let start = buf.len();
    let count = input
        .take((limit as u64).saturating_add(1))
        .read_until(b'\n', buf)
        .map_err(HostError::from_io)?;
    if count > limit {
        buf.truncate(start);
        return Err(HostError::new(
            HostErrorKind::InvalidInput,
            "I/O result exceeds the vector size limit",
        ));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_line_rejects_input_after_one_byte_past_the_limit() {
        let mut input = &b"abcdefgh\n"[..];
        let mut buf = Vec::new();
        let error = read_line_bounded(&mut input, &mut buf, 4).unwrap_err();
        assert_eq!(error.kind, HostErrorKind::InvalidInput);
        assert!(buf.is_empty());
        assert_eq!(input, b"fgh\n");
        assert_eq!(read_line_bounded(input, &mut buf, 4).unwrap(), 4);
        assert_eq!(buf, b"fgh\n");
    }
}

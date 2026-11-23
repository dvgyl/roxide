//! Deterministic in-memory [`Host`] implementation for unit tests and sandboxing.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{
    CommandSpec, CpuTimes, DirEntry, FileMeta, Host, HostError, HostErrorKind, HostFile,
    HostResult, Limits, LocaleInfo, OpenMode, ProcessOutput, StdStream, SysInfo, WallTime,
};

struct MemFile {
    storage: Arc<Mutex<Vec<u8>>>,
    cursor: u64,
    readable: bool,
    writable: bool,
    append: bool,
}

impl Read for MemFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.readable {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file not opened for reading",
            ));
        }
        let data = self
            .storage
            .lock()
            .map_err(|_| io::Error::other("memory file lock poisoned"))?;
        let mut cur = Cursor::new(data.as_slice());
        cur.set_position(self.cursor);
        let n = cur.read(buf)?;
        self.cursor = cur.position();
        Ok(n)
    }
}

impl Write for MemFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.writable {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file not opened for writing",
            ));
        }
        let mut data = self
            .storage
            .lock()
            .map_err(|_| io::Error::other("memory file lock poisoned"))?;
        if self.append {
            self.cursor = data.len() as u64;
        }
        let pos = self.cursor as usize;
        if pos > data.len() {
            data.resize(pos, 0);
        }
        let end = pos + buf.len();
        if end > data.len() {
            data.resize(end, 0);
        }
        data[pos..end].copy_from_slice(buf);
        self.cursor = end as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for MemFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let len = self
            .storage
            .lock()
            .map_err(|_| io::Error::other("memory file lock poisoned"))?
            .len() as i64;
        let next = match pos {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::End(d) => len.saturating_add(d),
            SeekFrom::Current(d) => (self.cursor as i64).saturating_add(d),
        };
        if next < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid seek before start of file",
            ));
        }
        self.cursor = next as u64;
        Ok(self.cursor)
    }
}

/// Deterministic in-memory [`Host`] with captured stdout/stderr and virtual filesystem.
pub struct PureHost {
    /// Captured standard output bytes.
    pub stdout: Vec<u8>,
    /// Captured standard error bytes.
    pub stderr: Vec<u8>,
    stdin: Cursor<Vec<u8>>,
    files: BTreeMap<PathBuf, Arc<Mutex<Vec<u8>>>>,
    dirs: BTreeSet<PathBuf>,
    cwd: PathBuf,
    temp_dir: PathBuf,
    env: BTreeMap<Vec<u8>, Vec<u8>>,
    now: WallTime,
    monotonic: Duration,
    entropy_state: u64,
    limits: Limits,
    interrupted: bool,
}

impl PureHost {
    /// Create a fresh deterministic [`PureHost`].
    #[must_use]
    pub fn new() -> Self {
        let mut dirs = BTreeSet::new();
        dirs.insert(PathBuf::from("/"));
        dirs.insert(PathBuf::from("/work"));
        dirs.insert(PathBuf::from("/tmp"));
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdin: Cursor::new(Vec::new()),
            files: BTreeMap::new(),
            dirs,
            cwd: PathBuf::from("/work"),
            temp_dir: PathBuf::from("/tmp"),
            env: BTreeMap::new(),
            now: WallTime {
                unix_us: 1_700_000_000_000_000,
            },
            monotonic: Duration::ZERO,
            entropy_state: 0x517C_C1B7_2722_0A95,
            limits: Limits::default(),
            interrupted: false,
        }
    }

    /// Populate scripted standard input bytes.
    pub fn set_stdin(&mut self, data: impl Into<Vec<u8>>) {
        self.stdin = Cursor::new(data.into());
    }

    /// Seed an in-memory file at `path`.
    pub fn insert_file(&mut self, path: impl AsRef<Path>, data: impl Into<Vec<u8>>) {
        let resolved = self.resolve(path.as_ref());
        if let Some(parent) = resolved.parent() {
            self.dirs.insert(parent.to_path_buf());
        }
        self.files
            .insert(resolved, Arc::new(Mutex::new(data.into())));
    }

    /// Read captured stdout as UTF-8 (lossy).
    #[must_use]
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Read captured stderr as UTF-8 (lossy).
    #[must_use]
    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }

    /// Flag an interrupt for the next [`Host::poll_interrupt`] call.
    pub fn trigger_interrupt(&mut self) {
        self.interrupted = true;
    }

    fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        }
    }
}

impl Default for PureHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for PureHost {
    fn write_out(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.stdout.extend_from_slice(bytes);
        Ok(())
    }

    fn write_err(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.stderr.extend_from_slice(bytes);
        Ok(())
    }

    fn flush(&mut self, _stream: StdStream) -> HostResult<()> {
        Ok(())
    }

    fn read_line(&mut self, buf: &mut Vec<u8>) -> HostResult<usize> {
        use std::io::BufRead;
        self.stdin
            .read_until(b'\n', buf)
            .map_err(HostError::from_io)
    }

    fn is_tty(&self, _stream: StdStream) -> bool {
        false
    }

    fn open(&mut self, path: &Path, mode: OpenMode) -> HostResult<Box<dyn HostFile>> {
        let resolved = self.resolve(path);
        let storage = if let Some(existing) = self.files.get(&resolved) {
            if mode.truncate
                && let Ok(mut guard) = existing.lock()
            {
                guard.clear();
            }
            Arc::clone(existing)
        } else if mode.create {
            let s = Arc::new(Mutex::new(Vec::new()));
            self.files.insert(resolved, Arc::clone(&s));
            s
        } else {
            return Err(HostError::new(
                HostErrorKind::NotFound,
                "No such file or directory",
            ));
        };
        Ok(Box::new(MemFile {
            storage,
            cursor: 0,
            readable: mode.read,
            writable: mode.write,
            append: mode.append,
        }))
    }

    fn metadata(&self, path: &Path, _follow: bool) -> HostResult<FileMeta> {
        let resolved = self.resolve(path);
        if self.dirs.contains(&resolved) {
            return Ok(FileMeta {
                len: 0,
                is_dir: true,
                is_file: false,
                is_symlink: false,
                modified_us: Some(self.now.unix_us),
            });
        }
        if let Some(file) = self.files.get(&resolved) {
            let len = file.lock().map_or(0, |g| g.len() as u64);
            return Ok(FileMeta {
                len,
                is_dir: false,
                is_file: true,
                is_symlink: false,
                modified_us: Some(self.now.unix_us),
            });
        }
        Err(HostError::new(
            HostErrorKind::NotFound,
            "No such file or directory",
        ))
    }

    fn read_dir(&self, path: &Path) -> HostResult<Vec<DirEntry>> {
        let resolved = self.resolve(path);
        if !self.dirs.contains(&resolved) {
            return Err(HostError::new(
                HostErrorKind::NotFound,
                "No such file or directory",
            ));
        }
        let mut entries = Vec::new();
        for dir in &self.dirs {
            if dir != &resolved
                && dir.parent() == Some(resolved.as_path())
                && let Some(name) = dir.file_name()
            {
                entries.push(DirEntry {
                    name: name.to_string_lossy().into_owned(),
                    path: dir.clone(),
                    is_dir: true,
                });
            }
        }
        for file in self.files.keys() {
            if file.parent() == Some(resolved.as_path())
                && let Some(name) = file.file_name()
            {
                entries.push(DirEntry {
                    name: name.to_string_lossy().into_owned(),
                    path: file.clone(),
                    is_dir: false,
                });
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn create_dir(&mut self, path: &Path, _recursive: bool, _mode: u32) -> HostResult<()> {
        let resolved = self.resolve(path);
        self.dirs.insert(resolved);
        Ok(())
    }

    fn remove(&mut self, path: &Path, recursive: bool) -> HostResult<()> {
        let resolved = self.resolve(path);
        if self.files.remove(&resolved).is_some() {
            return Ok(());
        }
        if self.dirs.contains(&resolved) {
            if recursive {
                self.files.retain(|k, _| !k.starts_with(&resolved));
                self.dirs.retain(|k| !k.starts_with(&resolved));
            } else {
                self.dirs.remove(&resolved);
            }
            return Ok(());
        }
        Err(HostError::new(
            HostErrorKind::NotFound,
            "No such file or directory",
        ))
    }

    fn rename(&mut self, from: &Path, to: &Path) -> HostResult<()> {
        let src = self.resolve(from);
        let dst = self.resolve(to);
        if let Some(f) = self.files.remove(&src) {
            self.files.insert(dst, f);
            Ok(())
        } else {
            Err(HostError::new(
                HostErrorKind::NotFound,
                "No such file or directory",
            ))
        }
    }

    fn canonicalize(&self, path: &Path) -> HostResult<PathBuf> {
        let resolved = self.resolve(path);
        if self.files.contains_key(&resolved) || self.dirs.contains(&resolved) {
            Ok(resolved)
        } else {
            Err(HostError::new(
                HostErrorKind::NotFound,
                "No such file or directory",
            ))
        }
    }

    fn cwd(&self) -> HostResult<PathBuf> {
        Ok(self.cwd.clone())
    }

    fn set_cwd(&mut self, path: &Path) -> HostResult<()> {
        let resolved = self.resolve(path);
        if self.dirs.contains(&resolved) {
            self.cwd = resolved;
            Ok(())
        } else {
            Err(HostError::new(
                HostErrorKind::NotFound,
                "No such file or directory",
            ))
        }
    }

    fn session_temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    fn home_dir(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/work"))
    }

    fn env_var(&self, key: &str) -> Option<Vec<u8>> {
        self.env.get(key.as_bytes()).cloned()
    }

    fn set_env_var(&mut self, key: &str, value: Option<&[u8]>) -> HostResult<()> {
        match value {
            Some(v) => {
                self.env.insert(key.as_bytes().to_vec(), v.to_vec());
            }
            None => {
                self.env.remove(key.as_bytes());
            }
        }
        Ok(())
    }

    fn env_vars(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn locale(&self) -> LocaleInfo {
        LocaleInfo::default()
    }

    fn tzif(&self, _zone: &str) -> HostResult<Vec<u8>> {
        Err(HostError::new(
            HostErrorKind::NotFound,
            "time zone database unavailable in PureHost",
        ))
    }

    fn now(&self) -> WallTime {
        self.now
    }

    fn monotonic(&self) -> Duration {
        self.monotonic
    }

    fn cpu_times(&self) -> CpuTimes {
        CpuTimes::default()
    }

    fn sleep(&mut self, duration: Duration) -> HostResult<()> {
        self.monotonic += duration;
        Ok(())
    }

    fn pid(&self) -> u32 {
        1
    }

    fn fill_entropy(&mut self, buf: &mut [u8]) {
        for b in buf {
            self.entropy_state = self
                .entropy_state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            *b = (self.entropy_state >> 56) as u8;
        }
    }

    fn sys_info(&self) -> SysInfo {
        SysInfo {
            sysname: String::from("PureHost"),
            release: String::from("0.1"),
            machine: String::from("deterministic"),
            platform: String::from("pure-host"),
        }
    }

    fn spawn(&mut self, _spec: &CommandSpec) -> HostResult<ProcessOutput> {
        Err(HostError::new(
            HostErrorKind::InvalidInput,
            "subprocess execution is disabled in PureHost",
        ))
    }

    fn poll_interrupt(&mut self) -> bool {
        let was = self.interrupted;
        self.interrupted = false;
        was
    }

    fn limits(&self) -> &Limits {
        &self.limits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_host_in_memory_file_roundtrip() {
        let mut host = PureHost::new();
        {
            let mut f = host
                .open(Path::new("hello.txt"), OpenMode::write_truncate())
                .unwrap();
            f.write_all(b"roxide").unwrap();
        }
        let mut out = String::new();
        {
            let mut f = host
                .open(Path::new("hello.txt"), OpenMode::read_only())
                .unwrap();
            f.read_to_string(&mut out).unwrap();
        }
        assert_eq!(out, "roxide");
        assert!(host.metadata(Path::new("hello.txt"), true).unwrap().is_file);
    }
}

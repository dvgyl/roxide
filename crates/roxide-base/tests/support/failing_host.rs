//! In-memory host with a file that fails on each write.

use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use roxide_host::{
    CommandSpec, CpuTimes, DirEntry, FileMeta, Host, HostFile, HostResult, Limits, LocaleInfo,
    OpenMode, ProcessOutput, PureHost, StdStream, SysInfo, WallTime,
};

struct FailingFile(Cursor<Vec<u8>>);

impl HostFile for FailingFile {}

impl Read for FailingFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for FailingFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}

impl Write for FailingFile {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected file write failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct FailingHost {
    host: PureHost,
    limits: Limits,
    locale: LocaleInfo,
}

impl FailingHost {
    pub(super) fn with_limit(max_vector_bytes: usize, utf8: bool) -> Self {
        Self {
            limits: Limits {
                max_vector_bytes,
                ..Limits::default()
            },
            locale: if utf8 {
                LocaleInfo::default()
            } else {
                LocaleInfo {
                    ctype: "C".to_owned(),
                    utf8,
                }
            },
            ..Self::default()
        }
    }
}

impl Host for FailingHost {
    fn write_out(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.host.write_out(bytes)
    }
    fn write_err(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.host.write_err(bytes)
    }
    fn flush(&mut self, stream: StdStream) -> HostResult<()> {
        self.host.flush(stream)
    }
    fn read_line(&mut self, buf: &mut Vec<u8>) -> HostResult<usize> {
        self.host.read_line(buf)
    }
    fn is_tty(&self, stream: StdStream) -> bool {
        self.host.is_tty(stream)
    }
    fn open(&mut self, path: &Path, mode: OpenMode) -> HostResult<Box<dyn HostFile>> {
        if path.ends_with("failing.txt") && mode.write {
            return Ok(Box::new(FailingFile(Cursor::default())));
        }
        self.host.open(path, mode)
    }
    fn metadata(&self, path: &Path, follow: bool) -> HostResult<FileMeta> {
        self.host.metadata(path, follow)
    }
    fn read_dir(&self, path: &Path) -> HostResult<Vec<DirEntry>> {
        self.host.read_dir(path)
    }
    fn create_dir(&mut self, path: &Path, recursive: bool, mode: u32) -> HostResult<()> {
        self.host.create_dir(path, recursive, mode)
    }
    fn remove(&mut self, path: &Path, recursive: bool) -> HostResult<()> {
        self.host.remove(path, recursive)
    }
    fn rename(&mut self, from: &Path, to: &Path) -> HostResult<()> {
        self.host.rename(from, to)
    }
    fn canonicalize(&self, path: &Path) -> HostResult<PathBuf> {
        self.host.canonicalize(path)
    }
    fn cwd(&self) -> HostResult<PathBuf> {
        self.host.cwd()
    }
    fn set_cwd(&mut self, path: &Path) -> HostResult<()> {
        self.host.set_cwd(path)
    }
    fn session_temp_dir(&self) -> &Path {
        self.host.session_temp_dir()
    }
    fn home_dir(&self) -> Option<PathBuf> {
        self.host.home_dir()
    }
    fn env_var(&self, key: &str) -> Option<Vec<u8>> {
        self.host.env_var(key)
    }
    fn set_env_var(&mut self, key: &str, value: Option<&[u8]>) -> HostResult<()> {
        self.host.set_env_var(key, value)
    }
    fn env_vars(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.host.env_vars()
    }
    fn locale(&self) -> LocaleInfo {
        self.locale.clone()
    }
    fn tzif(&self, zone: &str) -> HostResult<Vec<u8>> {
        self.host.tzif(zone)
    }
    fn now(&self) -> WallTime {
        self.host.now()
    }
    fn monotonic(&self) -> Duration {
        self.host.monotonic()
    }
    fn cpu_times(&self) -> CpuTimes {
        self.host.cpu_times()
    }
    fn sleep(&mut self, duration: Duration) -> HostResult<()> {
        self.host.sleep(duration)
    }
    fn pid(&self) -> u32 {
        self.host.pid()
    }
    fn fill_entropy(&mut self, buf: &mut [u8]) {
        self.host.fill_entropy(buf)
    }
    fn sys_info(&self) -> SysInfo {
        self.host.sys_info()
    }
    fn spawn(&mut self, spec: &CommandSpec) -> HostResult<ProcessOutput> {
        self.host.spawn(spec)
    }
    fn poll_interrupt(&mut self) -> bool {
        self.host.poll_interrupt()
    }
    fn limits(&self) -> &Limits {
        &self.limits
    }
}

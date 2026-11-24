#![forbid(unsafe_code)]

//! OS abstraction layer for Roxide (`Host`, `NativeHost`, and deterministic `PureHost`).
//!
//! All console I/O, filesystem access, environment variables, clocks, entropy,
//! and subprocess execution route through [`Host`] so tests and embeddings run
//! deterministically without touching the host OS.

mod forward;
pub mod native;
pub mod pure;

use std::fmt;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use native::NativeHost;
pub use pure::PureHost;

/// Standard console stream selector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StdStream {
    /// Standard input (`stdin`).
    Stdin,
    /// Standard output (`stdout`).
    Stdout,
    /// Standard error (`stderr`).
    Stderr,
}

/// File open mode flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpenMode {
    /// Open for reading.
    pub read: bool,
    /// Open for writing.
    pub write: bool,
    /// Create the file if it does not exist.
    pub create: bool,
    /// Truncate existing contents on open.
    pub truncate: bool,
    /// Append writes to end of file.
    pub append: bool,
}

impl OpenMode {
    /// Read-only open mode.
    #[must_use]
    pub const fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            create: false,
            truncate: false,
            append: false,
        }
    }

    /// Create/truncate write-only open mode.
    #[must_use]
    pub const fn write_truncate() -> Self {
        Self {
            read: false,
            write: true,
            create: true,
            truncate: true,
            append: false,
        }
    }
}

/// Abstract open file handle supporting read, write, and seek.
pub trait HostFile: Read + Write + Seek {}
impl<T: Read + Write + Seek> HostFile for T {}

/// File metadata snapshot.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileMeta {
    /// Byte length of the file.
    pub len: u64,
    /// True if this entry is a directory.
    pub is_dir: bool,
    /// True if this entry is a regular file.
    pub is_file: bool,
    /// True if this entry is a symbolic link.
    pub is_symlink: bool,
    /// Last modification timestamp in microseconds since Unix epoch, if available.
    pub modified_us: Option<i64>,
}

/// Single directory listing entry.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DirEntry {
    /// Entry filename (not full path).
    pub name: String,
    /// Full path of the entry.
    pub path: PathBuf,
    /// True if entry is a directory.
    pub is_dir: bool,
}

/// Locale summary exposed to R's `Sys.getlocale()` / `l10n_info()`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LocaleInfo {
    /// Active LC_CTYPE string (e.g. `"en_US.UTF-8"` or `"C"`).
    pub ctype: String,
    /// Whether the active locale uses UTF-8.
    pub utf8: bool,
}

impl Default for LocaleInfo {
    fn default() -> Self {
        Self {
            ctype: String::from("en_US.UTF-8"),
            utf8: true,
        }
    }
}

/// Wall-clock time in microseconds since the Unix epoch.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct WallTime {
    /// Signed microseconds relative to `1970-01-01T00:00:00Z`.
    pub unix_us: i64,
}

/// Process CPU times for `proc.time()`.
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub struct CpuTimes {
    /// User CPU seconds.
    pub user_secs: f64,
    /// System CPU seconds.
    pub system_secs: f64,
    /// Elapsed wall seconds since session start.
    pub elapsed_secs: f64,
}

/// Platform summary for `Sys.info()` and `R.version`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SysInfo {
    /// OS name (e.g. `"Darwin"`).
    pub sysname: String,
    /// OS release string.
    pub release: String,
    /// CPU architecture (e.g. `"aarch64"`).
    pub machine: String,
    /// Platform triple (e.g. `"aarch64-apple-darwin"`).
    pub platform: String,
}

impl Default for SysInfo {
    fn default() -> Self {
        Self {
            sysname: String::from(std::env::consts::OS),
            release: String::new(),
            machine: String::from(std::env::consts::ARCH),
            platform: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        }
    }
}

/// Subprocess launch specification for `system()` / `system2()`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CommandSpec {
    /// Executable program name or path.
    pub program: String,
    /// Argument vector.
    pub args: Vec<String>,
    /// Optional stdin bytes.
    pub stdin: Option<Vec<u8>>,
    /// Optional timeout.
    pub timeout: Option<Duration>,
}

/// Result of running a subprocess.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProcessOutput {
    /// Process exit status code.
    pub status: i32,
    /// Captured standard output bytes.
    pub stdout: Vec<u8>,
    /// Captured standard error bytes.
    pub stderr: Vec<u8>,
}

/// Safety and resource limits enforced across untrusted inputs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Limits {
    /// Maximum single vector allocation in bytes (default: 512 MiB on 16 GB workstation).
    pub max_vector_bytes: usize,
    /// Maximum expression evaluation nesting depth (matches R `options(expressions = 5000)`).
    pub max_expressions: usize,
    /// Maximum decompressed bytes for archive/RDS streams.
    pub max_decompress_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_vector_bytes: 512 * 1024 * 1024,
            max_expressions: 5000,
            max_decompress_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Broad error classification for [`HostError`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HostErrorKind {
    /// File or resource not found.
    NotFound,
    /// Permission denied.
    PermissionDenied,
    /// Resource already exists.
    AlreadyExists,
    /// Invalid input or unsupported operation.
    InvalidInput,
    /// Operation interrupted.
    Interrupted,
    /// Operation timed out.
    TimedOut,
    /// Any other OS/host error.
    Other,
}

/// OS/host error carrying bare `strerror`-style text without Rust's `(os error N)` suffix.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HostError {
    /// Categorized error kind.
    pub kind: HostErrorKind,
    /// Clean human-readable message.
    pub message: String,
    /// Raw OS errno code when available.
    pub os_code: Option<i32>,
}

impl HostError {
    /// Create a [`HostError`] with a kind and clean message.
    #[must_use]
    pub fn new(kind: HostErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            os_code: None,
        }
    }

    /// Convert a [`std::io::Error`], stripping Rust's trailing `" (os error N)"` suffix.
    #[must_use]
    pub fn from_io(err: std::io::Error) -> Self {
        let os_code = err.raw_os_error();
        let kind = match err.kind() {
            std::io::ErrorKind::NotFound => HostErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied => HostErrorKind::PermissionDenied,
            std::io::ErrorKind::AlreadyExists => HostErrorKind::AlreadyExists,
            std::io::ErrorKind::InvalidInput => HostErrorKind::InvalidInput,
            std::io::ErrorKind::Interrupted => HostErrorKind::Interrupted,
            std::io::ErrorKind::TimedOut => HostErrorKind::TimedOut,
            _ => HostErrorKind::Other,
        };
        let raw = err.to_string();
        let clean = match raw.rfind(" (os error ") {
            Some(pos) if raw.ends_with(')') => raw[..pos].to_owned(),
            _ => raw,
        };
        Self {
            kind,
            message: clean,
            os_code,
        }
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HostError {}

/// Convenience alias for [`Host`] fallible results.
pub type HostResult<T> = Result<T, HostError>;

/// Platform abstraction boundary used by `roxide-eval` and all standard packages.
pub trait Host {
    /// Write bytes to standard output.
    fn write_out(&mut self, bytes: &[u8]) -> HostResult<()>;
    /// Write bytes to standard error.
    fn write_err(&mut self, bytes: &[u8]) -> HostResult<()>;
    /// Flush a standard output stream.
    fn flush(&mut self, stream: StdStream) -> HostResult<()>;
    /// Read one line from standard input into `buf`, returning bytes read (`0` on EOF).
    fn read_line(&mut self, buf: &mut Vec<u8>) -> HostResult<usize>;
    /// Return whether a standard stream is attached to a TTY.
    fn is_tty(&self, stream: StdStream) -> bool;

    /// Open a file with the given mode.
    fn open(&mut self, path: &Path, mode: OpenMode) -> HostResult<Box<dyn HostFile>>;
    /// Read file metadata (`follow = true` follows symlinks).
    fn metadata(&self, path: &Path, follow: bool) -> HostResult<FileMeta>;
    /// List entries of a directory.
    fn read_dir(&self, path: &Path) -> HostResult<Vec<DirEntry>>;
    /// Create a directory.
    fn create_dir(&mut self, path: &Path, recursive: bool, mode: u32) -> HostResult<()>;
    /// Remove a file or directory.
    fn remove(&mut self, path: &Path, recursive: bool) -> HostResult<()>;
    /// Rename a file or directory.
    fn rename(&mut self, from: &Path, to: &Path) -> HostResult<()>;
    /// Resolve canonical absolute path.
    fn canonicalize(&self, path: &Path) -> HostResult<PathBuf>;
    /// Current working directory.
    fn cwd(&self) -> HostResult<PathBuf>;
    /// Change current working directory.
    fn set_cwd(&mut self, path: &Path) -> HostResult<()>;
    /// Per-session temporary directory path.
    fn session_temp_dir(&self) -> &Path;
    /// User home directory, if known.
    fn home_dir(&self) -> Option<PathBuf>;

    /// Look up an environment variable.
    fn env_var(&self, key: &str) -> Option<Vec<u8>>;
    /// Set or unset (`None`) an environment variable.
    fn set_env_var(&mut self, key: &str, value: Option<&[u8]>) -> HostResult<()>;
    /// Enumerate all environment variables.
    fn env_vars(&self) -> Vec<(Vec<u8>, Vec<u8>)>;
    /// Active locale summary.
    fn locale(&self) -> LocaleInfo;
    /// Load TZif bytes for an IANA time zone name.
    fn tzif(&self, zone: &str) -> HostResult<Vec<u8>>;

    /// Current wall clock time.
    fn now(&self) -> WallTime;
    /// Monotonic duration since session start.
    fn monotonic(&self) -> Duration;
    /// CPU and elapsed time summary.
    fn cpu_times(&self) -> CpuTimes;
    /// Sleep for `duration` (interruptible).
    fn sleep(&mut self, duration: Duration) -> HostResult<()>;

    /// Process identifier.
    fn pid(&self) -> u32;
    /// Fill `buf` with entropy bytes (for RNG seeding).
    fn fill_entropy(&mut self, buf: &mut [u8]);
    /// Platform information.
    fn sys_info(&self) -> SysInfo;
    /// Run a child process.
    fn spawn(&mut self, spec: &CommandSpec) -> HostResult<ProcessOutput>;
    /// Return true if a user interrupt has been signaled.
    fn poll_interrupt(&mut self) -> bool;
    /// Resource and safety limits.
    fn limits(&self) -> &Limits;
}

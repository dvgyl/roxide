//! Deterministic [`roxide_host::PureHost`] test runner for fast in-process unit tests.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use roxide_gc::GcPolicy;
use roxide_host::{
    CommandSpec, CpuTimes, DirEntry, FileMeta, Host, HostFile, HostResult, Limits, LocaleInfo,
    OpenMode, ProcessOutput, PureHost, StdStream, SysInfo, WallTime,
};

use crate::api::Unwind;
use crate::ctx::Ctx;

/// Captured result of running an R snippet in-process on [`PureHost`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
    /// Process-equivalent exit status (`0` on success, nonzero on error).
    pub status: i32,
}

struct SharedPureHost {
    inner: Rc<RefCell<PureHost>>,
    temp_dir: PathBuf,
    limits: Limits,
}

impl Host for SharedPureHost {
    fn write_out(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.inner.borrow_mut().write_out(bytes)
    }
    fn write_err(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.inner.borrow_mut().write_err(bytes)
    }
    fn flush(&mut self, stream: StdStream) -> HostResult<()> {
        self.inner.borrow_mut().flush(stream)
    }
    fn read_line(&mut self, buf: &mut Vec<u8>) -> HostResult<usize> {
        self.inner.borrow_mut().read_line(buf)
    }
    fn is_tty(&self, stream: StdStream) -> bool {
        self.inner.borrow().is_tty(stream)
    }
    fn open(&mut self, path: &Path, mode: OpenMode) -> HostResult<Box<dyn HostFile>> {
        self.inner.borrow_mut().open(path, mode)
    }
    fn metadata(&self, path: &Path, follow: bool) -> HostResult<FileMeta> {
        self.inner.borrow().metadata(path, follow)
    }
    fn read_dir(&self, path: &Path) -> HostResult<Vec<DirEntry>> {
        self.inner.borrow().read_dir(path)
    }
    fn create_dir(&mut self, path: &Path, recursive: bool, mode: u32) -> HostResult<()> {
        self.inner.borrow_mut().create_dir(path, recursive, mode)
    }
    fn remove(&mut self, path: &Path, recursive: bool) -> HostResult<()> {
        self.inner.borrow_mut().remove(path, recursive)
    }
    fn rename(&mut self, from: &Path, to: &Path) -> HostResult<()> {
        self.inner.borrow_mut().rename(from, to)
    }
    fn canonicalize(&self, path: &Path) -> HostResult<PathBuf> {
        self.inner.borrow().canonicalize(path)
    }
    fn cwd(&self) -> HostResult<PathBuf> {
        self.inner.borrow().cwd()
    }
    fn set_cwd(&mut self, path: &Path) -> HostResult<()> {
        self.inner.borrow_mut().set_cwd(path)
    }
    fn session_temp_dir(&self) -> &Path {
        &self.temp_dir
    }
    fn home_dir(&self) -> Option<PathBuf> {
        self.inner.borrow().home_dir()
    }
    fn env_var(&self, key: &str) -> Option<Vec<u8>> {
        self.inner.borrow().env_var(key)
    }
    fn set_env_var(&mut self, key: &str, value: Option<&[u8]>) -> HostResult<()> {
        self.inner.borrow_mut().set_env_var(key, value)
    }
    fn env_vars(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.inner.borrow().env_vars()
    }
    fn locale(&self) -> LocaleInfo {
        self.inner.borrow().locale()
    }
    fn tzif(&self, zone: &str) -> HostResult<Vec<u8>> {
        self.inner.borrow().tzif(zone)
    }
    fn now(&self) -> WallTime {
        self.inner.borrow().now()
    }
    fn monotonic(&self) -> Duration {
        self.inner.borrow().monotonic()
    }
    fn cpu_times(&self) -> CpuTimes {
        self.inner.borrow().cpu_times()
    }
    fn sleep(&mut self, duration: Duration) -> HostResult<()> {
        self.inner.borrow_mut().sleep(duration)
    }
    fn pid(&self) -> u32 {
        self.inner.borrow().pid()
    }
    fn fill_entropy(&mut self, buf: &mut [u8]) {
        self.inner.borrow_mut().fill_entropy(buf);
    }
    fn sys_info(&self) -> SysInfo {
        self.inner.borrow().sys_info()
    }
    fn spawn(&mut self, spec: &CommandSpec) -> HostResult<ProcessOutput> {
        self.inner.borrow_mut().spawn(spec)
    }
    fn poll_interrupt(&mut self) -> bool {
        self.inner.borrow_mut().poll_interrupt()
    }
    fn limits(&self) -> &Limits {
        &self.limits
    }
}

/// Evaluate `src` deterministically on [`PureHost`] with adaptive or torture GC.
#[must_use]
pub fn run(src: &str) -> Outcome {
    let policy = if std::env::var_os("ROXIDE_GC_TORTURE").is_some() {
        GcPolicy::Torture
    } else {
        GcPolicy::default()
    };
    run_with_policy(src, policy)
}

/// Evaluate `src` deterministically on [`PureHost`] with an explicit [`GcPolicy`].
#[must_use]
pub fn run_with_policy(src: &str, policy: GcPolicy) -> Outcome {
    let pure = PureHost::new();
    let temp_dir = pure.session_temp_dir().to_path_buf();
    let limits = pure.limits().clone();
    let shared = Rc::new(RefCell::new(pure));
    let host = Box::new(SharedPureHost {
        inner: Rc::clone(&shared),
        temp_dir,
        limits,
    });
    let Ok(mut ctx) = Ctx::new(host, policy) else {
        return Outcome {
            stdout: String::new(),
            stderr: String::from("Error: failed to initialize Roxide context\n"),
            status: 1,
        };
    };
    let status = match ctx.run_global("<test>", src) {
        Ok(()) => 0,
        Err(Unwind::Quit { status, .. }) => status,
        Err(Unwind::TopLevel) => 1,
        Err(Unwind::Internal(err)) => {
            let _ = ctx
                .host
                .write_err(format!("Error: {}\n", err.message).as_bytes());
            1
        }
        Err(Unwind::Transfer { .. }) => 1,
    };
    drop(ctx);
    let borrowed = shared.borrow();
    Outcome {
        stdout: borrowed.stdout_str(),
        stderr: borrowed.stderr_str(),
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torture_gc_executes_smoke_script_with_exact_output() {
        let out = run_with_policy("x <- 40\nx + 2\nprint(x)\n", GcPolicy::Torture);
        assert_eq!(out.status, 0);
        assert_eq!(out.stdout, "[1] 42\n[1] 40\n");
        assert_eq!(out.stderr, "");
    }
}

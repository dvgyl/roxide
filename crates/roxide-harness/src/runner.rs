//! Run the oracle and Roxide and compare their results.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The process output for one case.
pub struct CaseOutput {
    /// The output from GNU R.
    pub oracle: Output,
    /// The output from Roxide, if the harness ran it.
    pub roxide: Option<Output>,
}

/// Run both executables with the same temporary script and a timeout for each.
///
/// Pass `None` for Roxide to run only the oracle.
pub fn run_case(
    script: &str,
    oracle: &Path,
    roxide: Option<&Path>,
    timeout: Duration,
) -> Result<CaseOutput, String> {
    let script = TempScript::new(script)
        .map_err(|error| format!("cannot write temporary script: {error}"))?;
    let oracle_output = run(oracle, &script.0, timeout)
        .map_err(|error| format!("oracle ({}): {error}", oracle.display()))?;
    let roxide_output = roxide
        .map(|roxide| {
            run(roxide, &script.0, timeout)
                .map_err(|error| format!("Roxide ({}): {error}", roxide.display()))
        })
        .transpose()?;
    Ok(CaseOutput {
        oracle: oracle_output,
        roxide: roxide_output,
    })
}

/// Return one diagnostic for each exit status or output stream that differs.
///
/// The comparison uses the original bytes. Set `ignore_stderr_text` to skip
/// the comparison of standard error.
pub fn compare(oracle: &Output, roxide: &Output, ignore_stderr_text: bool) -> Vec<String> {
    let mut differences = Vec::new();
    if oracle.status != roxide.status {
        differences.push(format!(
            "exit status differs\n  Oracle: {}\n  Roxide: {}",
            oracle.status, roxide.status
        ));
    }
    if oracle.stdout != roxide.stdout {
        differences.push(byte_difference("stdout", &oracle.stdout, &roxide.stdout));
    }
    if !ignore_stderr_text && oracle.stderr != roxide.stderr {
        differences.push(byte_difference("stderr", &oracle.stderr, &roxide.stderr));
    }
    differences
}

fn byte_difference(stream: &str, oracle: &[u8], roxide: &[u8]) -> String {
    let offset = oracle
        .iter()
        .zip(roxide)
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| oracle.len().min(roxide.len()));
    let start = offset.saturating_sub(16);
    let excerpt = |bytes: &[u8]| {
        format!(
            "\"{}\" ({} bytes total)",
            bytes[start..bytes.len().min(offset.saturating_add(48))].escape_ascii(),
            bytes.len()
        )
    };
    format!(
        "{stream} differs at byte offset {offset}\n  Oracle (from {start}): {}\n  Roxide (from {start}): {}",
        excerpt(oracle),
        excerpt(roxide)
    )
}

struct TempScript(PathBuf);

impl TempScript {
    fn new(script: &str) -> io::Result<Self> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir();
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        for _ in 0..128 {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!("roxide-case-{}-{id}.R", std::process::id()));
            match options.open(&path) {
                Ok(mut file) => {
                    let script_file = Self(path);
                    let result = file.write_all(script.as_bytes());
                    drop(file);
                    result?;
                    return Ok(script_file);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot find a unique temporary script path",
        ))
    }
}

impl Drop for TempScript {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn run(program: &Path, script: &Path, timeout: Duration) -> io::Result<Output> {
    let mut child = Command::new(program)
        .arg("--vanilla")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = capture_output(&mut child, timeout);
    if result.is_err() && child.kill().is_ok() {
        let _ = child.wait();
    }
    result
}

fn capture_output(child: &mut Child, timeout: Duration) -> io::Result<Output> {
    let start = Instant::now();
    let stdout = read_pipe(child.stdout.take())?;
    let stderr = read_pipe(child.stderr.take())?;
    loop {
        if let Some(status) = child.try_wait()?
            && stdout.is_finished()
            && stderr.is_finished()
        {
            let join = |reader: JoinHandle<io::Result<Vec<u8>>>| {
                reader
                    .join()
                    .map_err(|_| io::Error::other("output reader stopped unexpectedly"))?
            };
            return Ok(Output {
                status,
                stdout: join(stdout)?,
                stderr: join(stderr)?,
            });
        }
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            // Another process can inherit a pipe. Joining its reader can block.
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("timed out after {} ms", timeout.as_millis()),
            ));
        }
        thread::sleep(remaining.min(Duration::from_millis(5)));
    }
}

fn read_pipe(
    pipe: Option<impl Read + Send + 'static>,
) -> io::Result<JoinHandle<io::Result<Vec<u8>>>> {
    let mut pipe = pipe.ok_or_else(|| io::Error::other("cannot open process output pipe"))?;
    thread::Builder::new().spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_differences_show_non_utf8_bytes_and_newlines() {
        let diagnostic = byte_difference("stdout", b"a\xff\n", b"a\xfe");
        assert!(diagnostic.contains("byte offset 1"));
        assert!(diagnostic.contains(r#""a\xff\n" (3 bytes total)"#));
        assert!(diagnostic.contains(r#""a\xfe" (2 bytes total)"#));
    }
}

//! Run the oracle and Roxide and compare their results.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::parser::CompareMode;

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
    compare_with_mode(oracle, roxide, CompareMode::Exact, ignore_stderr_text)
}

/// Compare oracle and Roxide outputs using `mode` for stdout and exact comparison for stderr.
pub fn compare_with_mode(
    oracle: &Output,
    roxide: &Output,
    mode: CompareMode,
    ignore_stderr_text: bool,
) -> Vec<String> {
    let mut differences = Vec::new();
    if oracle.status != roxide.status {
        differences.push(format!(
            "exit status differs\n  Oracle: {}\n  Roxide: {}",
            oracle.status, roxide.status
        ));
    }
    match mode {
        CompareMode::Exact => {
            if oracle.stdout != roxide.stdout {
                differences.push(byte_difference("stdout", &oracle.stdout, &roxide.stdout));
            }
        }
        CompareMode::Ulp(_) | CompareMode::Rel { .. } => {
            if oracle.stdout != roxide.stdout
                && let Some(diff) = tolerant_stdout_difference(&oracle.stdout, &roxide.stdout, mode)
            {
                differences.push(diff);
            }
        }
    }
    if !ignore_stderr_text && oracle.stderr != roxide.stderr {
        differences.push(byte_difference("stderr", &oracle.stderr, &roxide.stderr));
    }
    differences
}

fn tolerant_stdout_difference(oracle: &[u8], roxide: &[u8], mode: CompareMode) -> Option<String> {
    let (Ok(oracle_str), Ok(roxide_str)) =
        (std::str::from_utf8(oracle), std::str::from_utf8(roxide))
    else {
        return Some(byte_difference("stdout", oracle, roxide));
    };
    let o_lines: Vec<&str> = oracle_str.lines().collect();
    let r_lines: Vec<&str> = roxide_str.lines().collect();
    if o_lines.len() != r_lines.len() {
        return Some(format!(
            "stdout line count differs ({} vs {})\n{}",
            o_lines.len(),
            r_lines.len(),
            byte_difference("stdout", oracle, roxide)
        ));
    }
    for (line_idx, (&ol, &rl)) in o_lines.iter().zip(r_lines.iter()).enumerate() {
        if ol == rl {
            continue;
        }
        let o_toks: Vec<&str> = ol.split_whitespace().collect();
        let r_toks: Vec<&str> = rl.split_whitespace().collect();
        if o_toks.len() != r_toks.len() {
            return Some(format!(
                "stdout line {} token count differs ({} vs {})\n  Oracle: {ol}\n  Roxide: {rl}",
                line_idx + 1,
                o_toks.len(),
                r_toks.len()
            ));
        }
        for (tok_idx, (&ot, &rt)) in o_toks.iter().zip(r_toks.iter()).enumerate() {
            if ot == rt {
                continue;
            }
            if !tokens_match_tolerant(ot, rt, mode) {
                return Some(format!(
                    "stdout differs at line {} token {} ({mode:?})\n  Oracle token: {ot}\n  Roxide token: {rt}\n  Oracle line: {ol}\n  Roxide line: {rl}",
                    line_idx + 1,
                    tok_idx + 1
                ));
            }
        }
    }
    None
}

fn tokens_match_tolerant(ot: &str, rt: &str, mode: CompareMode) -> bool {
    let (o_inner, r_inner) = if ot.starts_with('"')
        && ot.ends_with('"')
        && ot.len() >= 2
        && rt.starts_with('"')
        && rt.ends_with('"')
        && rt.len() >= 2
    {
        (&ot[1..ot.len() - 1], &rt[1..rt.len() - 1])
    } else {
        (ot, rt)
    };
    if is_special_r_token(o_inner) || is_special_r_token(r_inner) {
        return ot == rt;
    }
    let (Some(a), Some(b)) = (parse_numeric_token(o_inner), parse_numeric_token(r_inner)) else {
        return false;
    };
    if !a.is_finite() || !b.is_finite() {
        return false;
    }
    match mode {
        CompareMode::Exact => a.to_bits() == b.to_bits(),
        CompareMode::Ulp(max_ulp) => ulp_distance(a, b) <= max_ulp,
        CompareMode::Rel { rel, abs } => {
            let diff = (a - b).abs();
            let scale = a.abs().max(b.abs());
            diff <= abs.max(rel * scale)
        }
    }
}

fn is_special_r_token(tok: &str) -> bool {
    matches!(
        tok,
        "NA" | "NaN"
            | "Inf"
            | "-Inf"
            | "+Inf"
            | "NA_real_"
            | "NA_integer_"
            | "NA_character_"
            | "NA_complex_"
    )
}

fn parse_numeric_token(tok: &str) -> Option<f64> {
    if tok.starts_with("0x")
        || tok.starts_with("0X")
        || tok.starts_with("-0x")
        || tok.starts_with("-0X")
        || tok.starts_with("+0x")
        || tok.starts_with("+0X")
    {
        return parse_hex_f64(tok);
    }
    tok.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn parse_hex_f64(s: &str) -> Option<f64> {
    let (neg, rest) = if let Some(r) = s.strip_prefix('-') {
        (true, r)
    } else if let Some(r) = s.strip_prefix('+') {
        (false, r)
    } else {
        (false, s)
    };
    let rest = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))?;
    let p_pos = rest.find(['p', 'P'])?;
    let mant_str = &rest[..p_pos];
    let exp_str = &rest[p_pos + 1..];
    let exp_base: i32 = exp_str.parse().ok()?;
    let (int_part, frac_part) = match mant_str.split_once('.') {
        Some((i, f)) => (i, f),
        None => (mant_str, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let mut digits: Vec<u8> = Vec::with_capacity(int_part.len() + frac_part.len());
    for b in int_part.bytes().chain(frac_part.bytes()) {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        digits.push(d);
    }
    if digits.iter().all(|&d| d == 0) {
        return Some(if neg { -0.0 } else { 0.0 });
    }
    // Build exact IEEE-754 value from hex nibbles using bit accumulator + rounding.
    let mut sig: u128 = 0;
    let mut sig_bits: i32 = 0;
    let mut sticky = false;
    let mut leading_bits_before_dot: i32 = (int_part.len() as i32) * 4;
    let mut started = false;
    for d in digits {
        for shift in (0..4).rev() {
            let bit = (d >> shift) & 1;
            if !started {
                leading_bits_before_dot -= 1;
                if bit == 0 {
                    continue;
                }
                started = true;
            }
            if sig_bits < 60 {
                sig = (sig << 1) | u128::from(bit);
                sig_bits += 1;
            } else if bit != 0 {
                sticky = true;
            }
        }
    }
    // Value is (sig + sticky_eps) * 2^(exp_base + leading_bits_before_dot - (sig_bits - 1))
    let unbiased_exp = exp_base + leading_bits_before_dot;
    let value = if unbiased_exp > 1023 {
        f64::INFINITY
    } else if unbiased_exp >= -1022 {
        // Normal f64: need 53 bits of significand (1 implicit + 52 fraction).
        let (mant53, carry) = round_to_bits(sig, sig_bits, 53, sticky);
        let final_exp = unbiased_exp + i32::from(carry);
        if final_exp > 1023 {
            f64::INFINITY
        } else {
            let exp_bits = ((final_exp + 1023) as u64) << 52;
            let frac_bits = mant53 & ((1u64 << 52) - 1);
            f64::from_bits(exp_bits | frac_bits)
        }
    } else {
        // Subnormal f64: exponent is -1022, precision = 53 - (-1022 - unbiased_exp).
        let shift_down = -1022 - unbiased_exp;
        if shift_down > 53 {
            0.0
        } else {
            let keep = (53 - shift_down) as usize;
            let (mant, _) = round_to_bits(sig, sig_bits, keep, sticky);
            f64::from_bits(mant)
        }
    };
    Some(if neg { -value } else { value })
}

fn round_to_bits(sig: u128, sig_bits: i32, keep: usize, sticky_in: bool) -> (u64, bool) {
    let keep_i32 = keep as i32;
    if keep == 0 {
        // Strictly rounding half-to-even at bit 0 (value is < 2^-1074).
        let half = sig_bits == 1 && !sticky_in;
        return (u64::from(!half && sig != 0), false);
    }
    if sig_bits <= keep_i32 {
        return ((sig << (keep_i32 - sig_bits)) as u64, false);
    }
    let drop = (sig_bits - keep_i32) as u32;
    let main = (sig >> drop) as u64;
    let guard = ((sig >> (drop - 1)) & 1) != 0;
    let low_mask = (1u128 << (drop - 1)) - 1;
    let sticky = sticky_in || ((sig & low_mask) != 0);
    let round_up = guard && (sticky || (main & 1) != 0);
    let rounded = main + u64::from(round_up);
    if rounded == (1u64 << keep) {
        (rounded >> 1, true)
    } else {
        (rounded, false)
    }
}

fn ulp_distance(a: f64, b: f64) -> u64 {
    if a == 0.0 && b == 0.0 {
        return 0;
    }
    let oa = ordered_f64_bits(a);
    let ob = ordered_f64_bits(b);
    oa.abs_diff(ob)
}

fn ordered_f64_bits(x: f64) -> u64 {
    let bits = x.to_bits();
    if (bits & (1u64 << 63)) != 0 {
        !bits
    } else {
        bits | (1u64 << 63)
    }
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

    #[test]
    fn tolerant_compare_handles_hex_floats_ulps_and_special_tokens() {
        let x = parse_hex_f64("0x1.5ef721feab7dap+0").unwrap();
        let y = parse_hex_f64("0x1.5ef721feab7dcp+0").unwrap();
        assert_eq!(ulp_distance(x, y), 2);
        assert!(
            tolerant_stdout_difference(
                b"[1] \"0x1.5ef721feab7dap+0\" NA Inf\n",
                b"[1] \"0x1.5ef721feab7dcp+0\" NA Inf\n",
                CompareMode::Ulp(2),
            )
            .is_none()
        );
        assert!(
            tolerant_stdout_difference(
                b"[1] \"0x1.5ef721feab7dap+0\"\n",
                b"[1] \"0x1.5ef721feab7dcp+0\"\n",
                CompareMode::Ulp(1),
            )
            .is_some()
        );
        assert!(
            tolerant_stdout_difference(b"[1] NA\n", b"[1] NaN\n", CompareMode::Ulp(10)).is_some()
        );
        assert!(
            tolerant_stdout_difference(
                b"[1] 1.00000000000001\n",
                b"[1] 1.00000000000000\n",
                CompareMode::Rel {
                    rel: 1e-13,
                    abs: 0.0
                },
            )
            .is_none()
        );
    }
}

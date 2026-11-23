//! Pure GNU R 4.6.2 text formatters for errors, warnings, and call trailers.

use std::fmt::Write as _;

/// Format an R error line (`Error: msg` or `Error in <call> : msg`).
#[must_use]
pub fn error(call: Option<&[String]>, msg: &str) -> String {
    match call {
        None | Some([]) => format!("Error: {msg}\n"),
        Some(lines) => {
            let head = &lines[0];
            let sep = if lines.len() > 1 || head.len() + msg.len() > 75 {
                " : \n  "
            } else {
                " : "
            };
            if lines.len() == 1 {
                format!("Error in {head}{sep}{msg}\n")
            } else {
                let mut out = String::from("Error in ");
                for (i, line) in lines.iter().enumerate() {
                    if i > 0 {
                        out.push('\n');
                    }
                    out.push_str(line);
                }
                let _ = writeln!(out, "{sep}{msg}");
                out
            }
        }
    }
}

/// Format R's deferred warning summary (`Warning message:` / `Warning messages:`).
#[must_use]
pub fn warnings(list: &[(Option<Vec<String>>, String)], total: usize) -> String {
    if list.is_empty() {
        return String::new();
    }
    if total == 1 && list.len() == 1 {
        let (call, msg) = &list[0];
        return match call {
            None => format!("Warning message:\n{msg}\n"),
            Some(lines) if lines.is_empty() => format!("Warning message:\n{msg}\n"),
            Some(lines) => format!("Warning message:\nIn {} : {msg}\n", lines.join("\n")),
        };
    }
    if total > 50 {
        return String::from(
            "There were 50 or more warnings (use warnings() to see the first 50)\n",
        );
    }
    if total > 10 {
        return format!("There were {total} warnings (use warnings() to see them)\n");
    }
    let mut out = String::from("Warning messages:\n");
    for (idx, (call, msg)) in list.iter().enumerate() {
        let n = idx + 1;
        match call {
            None => {
                let _ = writeln!(out, "{n}: {msg}");
            }
            Some(lines) if lines.is_empty() => {
                let _ = writeln!(out, "{n}: {msg}");
            }
            Some(lines) => {
                let _ = writeln!(out, "{n}: In {} : {msg}", lines.join("\n"));
            }
        }
    }
    out
}

/// Format the non-interactive `Calls: a -> b -> c` trailer line.
#[must_use]
pub fn calls_trailer(calls: &[String]) -> String {
    if calls.is_empty() {
        return String::new();
    }
    format!("Calls: {}\n", calls.join(" -> "))
}

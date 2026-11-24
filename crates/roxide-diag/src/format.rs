//! Format GNU R 4.6.2 errors and warnings.

use std::fmt::{self, Write as _};

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
    match list {
        [] => return String::new(),
        [entry] if total == 1 => {
            return format!("Warning message:\n{}\n", PendingWarning::from(entry));
        }
        _ => {}
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
    for (idx, entry) in list.iter().enumerate() {
        let n = idx + 1;
        let warning = PendingWarning::from(entry);
        let _ = writeln!(out, "{n}: {warning}");
    }
    out
}

struct PendingWarning<'a> {
    call_lines: &'a [String],
    message: &'a str,
}

impl<'a> From<&'a (Option<Vec<String>>, String)> for PendingWarning<'a> {
    fn from((call, message): &'a (Option<Vec<String>>, String)) -> Self {
        Self {
            call_lines: call.as_deref().unwrap_or_default(),
            message,
        }
    }
}

impl fmt::Display for PendingWarning<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.call_lines {
            [] => f.write_str(self.message),
            lines => write!(f, "In {} : {}", lines.join("\n"), self.message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::warnings;

    #[test]
    fn renders_deferred_warnings_with_and_without_calls() {
        let list = [
            (None, "first".into()),
            (Some(Vec::new()), "second".into()),
            (Some(vec!["f(".into(), "    x)".into()]), "third".into()),
        ];

        assert_eq!(warnings(&list[..1], 1), "Warning message:\nfirst\n");
        assert_eq!(warnings(&list[1..2], 1), "Warning message:\nsecond\n");
        assert_eq!(
            warnings(&list[2..], 1),
            "Warning message:\nIn f(\n    x) : third\n"
        );
        assert_eq!(
            warnings(&list, 3),
            "Warning messages:\n1: first\n2: second\n3: In f(\n    x) : third\n"
        );
    }

    #[test]
    fn uses_the_total_to_select_the_warning_summary() {
        let list = [(None, "warning".into())];

        assert_eq!(warnings(&[], 51), "");
        assert_eq!(warnings(&list, 10), "Warning messages:\n1: warning\n");
        assert_eq!(
            warnings(&list, 11),
            "There were 11 warnings (use warnings() to see them)\n"
        );
        assert_eq!(
            warnings(&list, 50),
            "There were 50 warnings (use warnings() to see them)\n"
        );
        assert_eq!(
            warnings(&list, 51),
            "There were 50 or more warnings (use warnings() to see the first 50)\n"
        );
    }
}

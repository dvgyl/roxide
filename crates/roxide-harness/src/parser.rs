//! Read conformance cases from a file or a directory.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Comparison mode for stdout matching against GNU R 4.6.2.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum CompareMode {
    /// Exact byte-for-byte stdout comparison (default, used for Layer 1 & Tier T0).
    #[default]
    Exact,
    /// Token-level numeric comparison allowing up to `max_ulp` ULPs on finite floats.
    Ulp(u64),
    /// Token-level numeric comparison allowing relative tolerance `rel` and optional absolute floor `abs`.
    Rel {
        /// Maximum relative error `|a - b| / max(|a|, |b|)`.
        rel: f64,
        /// Maximum absolute error `|a - b|` (0.0 when omitted).
        abs: f64,
    },
}

/// An R script and its conformance metadata.
#[derive(Debug)]
pub struct Case {
    /// The case name.
    pub name: String,
    /// The tags that identify the case.
    pub tags: Vec<String>,
    /// Whether the oracle must return a nonzero exit code in oracle-only mode.
    pub expect_error: bool,
    /// Stdout comparison mode (`exact`, `ulp=N`, or `rel=X[,abs=Y]`).
    pub compare: CompareMode,
    /// The script text with its original line endings.
    pub script: String,
    /// The path to the case file.
    pub path: PathBuf,
    /// The one-based line number of the case header.
    pub line: usize,
}

/// Load a `.rcase` file or all visible `.rcase` files below a directory.
///
/// Files are processed in path order, and cases are processed in file order.
/// Directory discovery skips hidden entries and symbolic links.
pub fn load_cases(path: &Path) -> Result<Vec<Case>, String> {
    let metadata = fs::metadata(path).map_err(|error| path_error(path, error))?;
    let files = if metadata.is_dir() {
        discover_files(path)?
    } else if metadata.is_file() && is_case_file(path) {
        vec![path.to_owned()]
    } else {
        return Err(format!(
            "{}: expected a .rcase file or directory",
            path.display()
        ));
    };

    let mut cases = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).map_err(|error| path_error(&file, error))?;
        cases.extend(parse_cases(&file, &text)?);
    }
    Ok(cases)
}

fn path_error(path: &Path, error: std::io::Error) -> String {
    format!("{}: {error}", path.display())
}

fn is_case_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "rcase")
}

fn discover_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut directories = vec![root.to_owned()];
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| path_error(&directory, error))? {
            let entry = entry.map_err(|error| path_error(&directory, error))?;
            if entry.file_name().as_encoded_bytes().starts_with(b".") {
                continue;
            }
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|error| path_error(&path, error))?;
            if kind.is_dir() {
                directories.push(path);
            } else if kind.is_file() && is_case_file(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn parse_cases(path: &Path, text: &str) -> Result<Vec<Case>, String> {
    let mut cases: Vec<Case> = Vec::new();
    let mut names = HashSet::new();
    let mut in_metadata = false;
    let mut has_expect_error = false;
    let mut has_compare = false;

    for (index, line) in text.split_inclusive('\n').enumerate() {
        let number = index + 1;
        let fail = |message: &str| format!("{}:{number}: {message}", path.display());
        let trimmed = line.trim();
        if trimmed.starts_with("===") {
            let name = trimmed
                .strip_prefix("=== case: ")
                .and_then(|header| header.strip_suffix(" ==="))
                .map(str::trim)
                .filter(|name| !name.is_empty() && !name.contains("==="))
                .ok_or_else(|| fail("malformed case header. Use '=== case: <name> ==='."))?;
            if !names.insert(name) {
                return Err(fail(&format!("duplicate case name '{name}'")));
            }
            cases.push(Case {
                name: name.to_owned(),
                tags: Vec::new(),
                expect_error: false,
                compare: CompareMode::Exact,
                script: String::new(),
                path: path.to_owned(),
                line: number,
            });
            in_metadata = true;
            has_expect_error = false;
            has_compare = false;
            continue;
        }

        let Some(case) = cases.last_mut() else {
            if trimmed.is_empty() {
                continue;
            }
            return Err(fail("expected a case header"));
        };
        if let Some(metadata) = trimmed.strip_prefix("#@").filter(|_| in_metadata) {
            let (key, value) = metadata
                .split_once(':')
                .ok_or_else(|| fail("malformed case metadata. Use '#@ <key>: <value>'."))?;
            match key.trim() {
                "tags" if case.tags.is_empty() => {
                    for tag in value.split(',').map(str::trim) {
                        if tag.is_empty() {
                            return Err(fail("tags must contain a name for each tag"));
                        }
                        case.tags.push(tag.to_owned());
                    }
                }
                "expect_error" if !has_expect_error => {
                    case.expect_error = match value.trim() {
                        "true" => true,
                        "false" => false,
                        _ => return Err(fail("expect_error must be 'true' or 'false'")),
                    };
                    has_expect_error = true;
                }
                "compare" if !has_compare => {
                    case.compare = parse_compare_mode(value.trim()).map_err(fail)?;
                    has_compare = true;
                }
                "tags" | "expect_error" | "compare" => return Err(fail("duplicate case metadata")),
                key => return Err(fail(&format!("unknown case metadata '{key}'"))),
            }
        } else {
            in_metadata = false;
            case.script.push_str(line);
        }
    }
    Ok(cases)
}

fn parse_compare_mode(spec: &str) -> Result<CompareMode, &'static str> {
    if spec == "exact" {
        return Ok(CompareMode::Exact);
    }
    if let Some(rest) = spec.strip_prefix("ulp=") {
        let ulp = rest
            .parse::<u64>()
            .map_err(|_| "compare ulp=<N> requires a nonnegative integer")?;
        return Ok(CompareMode::Ulp(ulp));
    }
    if spec.starts_with("rel=") {
        let mut rel: Option<f64> = None;
        let mut abs: f64 = 0.0;
        let mut seen_abs = false;
        for part in spec.split(',').map(str::trim) {
            if let Some(val) = part.strip_prefix("rel=") {
                if rel.is_some() {
                    return Err("duplicate rel= parameter in compare metadata");
                }
                let parsed = val
                    .parse::<f64>()
                    .map_err(|_| "compare rel=<X> requires a finite nonnegative float")?;
                if !parsed.is_finite() || parsed < 0.0 {
                    return Err("compare rel=<X> requires a finite nonnegative float");
                }
                rel = Some(parsed);
            } else if let Some(val) = part.strip_prefix("abs=") {
                if seen_abs {
                    return Err("duplicate abs= parameter in compare metadata");
                }
                let parsed = val
                    .parse::<f64>()
                    .map_err(|_| "compare abs=<Y> requires a finite nonnegative float")?;
                if !parsed.is_finite() || parsed < 0.0 {
                    return Err("compare abs=<Y> requires a finite nonnegative float");
                }
                abs = parsed;
                seen_abs = true;
            } else {
                return Err("compare must be 'exact', 'ulp=<N>', or 'rel=<X>[,abs=<Y>]'");
            }
        }
        return Ok(CompareMode::Rel {
            rel: rel.ok_or("compare rel=<X> requires a rel parameter")?,
            abs,
        });
    }
    Err("compare must be 'exact', 'ulp=<N>', or 'rel=<X>[,abs=<Y>]'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_script_line_endings_and_metadata_comments_in_the_body() {
        let text = concat!(
            "=== case: first ===\r\n",
            "#@ tags: smoke, syntax\r\n",
            "#@ expect_error: true\r\n",
            "stop('example')\r\n",
            "#@ keep this comment\r\n",
            "\r\n",
            "=== case: second ===\n",
            "print(1)"
        );
        let cases = parse_cases(Path::new("test.rcase"), text).unwrap();
        assert_eq!(cases[0].tags, ["smoke", "syntax"]);
        assert!(cases[0].expect_error);
        assert_eq!(
            cases[0].script,
            "stop('example')\r\n#@ keep this comment\r\n\r\n"
        );
        assert_eq!(cases[1].line, 7);
        assert_eq!(cases[1].script, "print(1)");
        assert!(!cases[1].expect_error);
    }

    #[test]
    fn reports_invalid_headers_and_duplicate_names_with_line_numbers() {
        let path = Path::new("test.rcase");
        for (text, message) in [
            ("=== case: first", "test.rcase:1: malformed case header"),
            ("=== case:  ===", "test.rcase:1: malformed case header"),
            (
                "=== case: first ===\n\n=== case: first ===",
                "test.rcase:3: duplicate case name 'first'",
            ),
        ] {
            assert!(parse_cases(path, text).unwrap_err().starts_with(message));
        }
    }
}

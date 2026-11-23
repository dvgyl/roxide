//! Read conformance cases from a file or a directory.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// An R script and its conformance metadata.
#[derive(Debug)]
pub struct Case {
    /// The case name.
    pub name: String,
    /// The tags that identify the case.
    pub tags: Vec<String>,
    /// Whether the oracle must return a nonzero exit code in oracle-only mode.
    pub expect_error: bool,
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
                script: String::new(),
                path: path.to_owned(),
                line: number,
            });
            in_metadata = true;
            has_expect_error = false;
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
                "tags" | "expect_error" => return Err(fail("duplicate case metadata")),
                key => return Err(fail(&format!("unknown case metadata '{key}'"))),
            }
        } else {
            in_metadata = false;
            case.script.push_str(line);
        }
    }
    Ok(cases)
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

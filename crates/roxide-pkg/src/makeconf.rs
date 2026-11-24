//! Read build settings from a staged R_HOME.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use roxide_host::{Host, OpenMode};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_DEPTH: usize = 64;

/// Assignments and variable expansions from an Roxide Makeconf file.
#[derive(Debug)]
pub struct Makeconf(BTreeMap<String, String>);

impl Makeconf {
    /// Read `etc/Makeconf` below `home`.
    pub fn read(host: &mut dyn Host, home: &Path) -> Result<Self, String> {
        let path = home.join("etc/Makeconf");
        let result = (|| {
            let mut text = String::new();
            host.open(&path, OpenMode::read_only())
                .map_err(|error| error.to_string())?
                .take((MAX_BYTES + 1) as u64)
                .read_to_string(&mut text)
                .map_err(|error| error.to_string())?;
            Self::parse(&text)
        })();
        result.map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Parse literal and recursive make assignments.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_BYTES || text.contains('\0') {
            return Err("invalid Makeconf size or content".into());
        }
        let mut conf = Self(BTreeMap::new());
        let mut logical = String::new();
        let mut budget = MAX_BYTES;
        for line in text.lines() {
            let line = line.trim_start();
            if let Some(part) = line.strip_suffix('\\') {
                logical.push_str(part.trim_end());
                logical.push(' ');
                continue;
            }
            logical.push_str(line);
            let line = strip_comment(&logical);
            if let Some((key, value)) = line.split_once('=') {
                let immediate = key.ends_with(':');
                let key = key.strip_suffix(':').unwrap_or(key).trim();
                if valid_key(key) {
                    let value = value.trim();
                    let value = if immediate {
                        conf.expand(value, &mut Vec::new(), &mut budget)
                            .ok_or_else(|| format!("invalid expansion for '{key}'"))?
                            .replace('$', "$$")
                    } else {
                        value.to_owned()
                    };
                    conf.0.insert(key.to_owned(), value);
                }
            }
            logical.clear();
        }
        if !logical.is_empty() {
            return Err("unfinished Makeconf continuation".into());
        }
        Ok(conf)
    }

    /// Return the expanded value, or `None` for an unknown or cyclic variable.
    pub fn get(&self, key: &str) -> Option<String> {
        let (key, value) = self.0.get_key_value(key)?;
        let mut budget = MAX_BYTES;
        self.expand(value, &mut vec![key], &mut budget)
    }

    /// Show assignments in key order. Show the original value if expansion fails.
    pub fn dump(&self) -> String {
        let mut output = String::new();
        let mut budget = MAX_BYTES;
        for (key, raw) in &self.0 {
            use std::fmt::Write;
            let value = self.expand(raw, &mut vec![key], &mut budget);
            let _ = writeln!(output, "{key} = {}", value.as_deref().unwrap_or(raw));
        }
        output
    }

    fn expand<'a>(
        &'a self,
        value: &str,
        stack: &mut Vec<&'a str>,
        budget: &mut usize,
    ) -> Option<String> {
        *budget = budget.checked_sub(value.len())?;
        let mut output = String::new();
        let mut rest = value;
        while let Some((prefix, token)) = rest.split_once('$') {
            output.push_str(prefix);
            let Some(open) = token.chars().next() else {
                output.push('$');
                rest = token;
                continue;
            };
            let tail = &token[open.len_utf8()..];
            let close = match open {
                '(' => ')',
                '{' => '}',
                '$' => {
                    output.push('$');
                    rest = tail;
                    continue;
                }
                _ => {
                    output.push('$');
                    rest = token;
                    continue;
                }
            };
            let (key, tail) = tail.split_once(close)?;
            if let Some((key, value)) = self.0.get_key_value(key) {
                if stack.len() == MAX_DEPTH || stack.contains(&key.as_str()) {
                    return None;
                }
                stack.push(key);
                output.push_str(&self.expand(value, stack, budget)?);
                stack.pop();
            } else if !valid_key(key) {
                return None;
            }
            rest = tail;
        }
        output.push_str(rest);
        Some(output)
    }
}

fn strip_comment(line: &str) -> String {
    let mut output = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '#' => break,
            '\\' if chars.peek() == Some(&'#') => {
                chars.next();
                output.push('#');
            }
            _ => output.push(ch),
        }
    }
    output
}

pub(crate) fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub(crate) fn r_home(host: &dyn Host) -> Result<PathBuf, String> {
    if let Some(path) = env_text(host, "R_HOME")?.filter(|path| !path.is_empty()) {
        return absolute(host, Path::new(&path));
    }
    let cwd = host.cwd().map_err(|error| error.to_string())?;
    let exe = std::env::current_exe().ok();
    let candidates = exe
        .iter()
        .flat_map(|path| path.ancestors().skip(2).take(2))
        .flat_map(|dir| [dir.to_path_buf(), dir.join("rhome")])
        .chain([cwd.join("target/rhome")]);
    candidates
        .into_iter()
        .find(|dir| is_file(host, &dir.join("etc/Makeconf")))
        .ok_or_else(|| "cannot find R_HOME with etc/Makeconf".into())
}

pub(crate) fn env_text(host: &dyn Host, key: &str) -> Result<Option<String>, String> {
    host.env_var(key)
        .map(|value| String::from_utf8(value).map_err(|_| format!("invalid UTF-8 in {key}")))
        .transpose()
}

pub(crate) fn absolute(host: &dyn Host, path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        host.cwd()
            .map(|cwd| cwd.join(path))
            .map_err(|error| error.to_string())
    }
}

pub(crate) fn is_file(host: &dyn Host, path: &Path) -> bool {
    host.metadata(path, true).is_ok_and(|meta| meta.is_file)
}

/// Run `R CMD config` and return its exit status.
pub fn run_config_cli(args: &[String], host: &mut dyn Host) -> i32 {
    let result = match args {
        [arg] if matches!(arg.as_str(), "-h" | "--help") => {
            Ok("Usage: R CMD config [--all | --cppflags | --ldflags | VARIABLE]\n".into())
        }
        [arg] => config(arg, host),
        _ => Err("Usage: R CMD config [--all | --cppflags | --ldflags | VARIABLE]".into()),
    };
    match result {
        Ok(output) => i32::from(host.write_out(output.as_bytes()).is_err()),
        Err(error) => {
            let _ = host.write_err(format!("{error}\n").as_bytes());
            1
        }
    }
}

fn config(arg: &str, host: &mut dyn Host) -> Result<String, String> {
    let home = r_home(host).map_err(|error| format!("ERROR: {error}"))?;
    let conf = Makeconf::read(host, &home).map_err(|error| format!("ERROR: {error}"))?;
    let value = match arg {
        "--all" => return Ok(conf.dump()),
        "--cppflags" => conf.get("R_XTRA_CPPFLAGS").or_else(|| {
            let include = conf
                .get("R_INCLUDE_DIR")
                .unwrap_or_else(|| home.join("include").to_string_lossy().into_owned());
            Some(format!("-I{}", crate::shlib::shell_word(&include)))
        }),
        "--ldflags" => conf.get("LIBR"),
        key => conf.get(key),
    };
    value
        .map(|value| format!("{value}\n"))
        .ok_or_else(|| format!("ERROR: no information for variable '{arg}'"))
}

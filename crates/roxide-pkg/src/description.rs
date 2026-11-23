//! Read and validate package DESCRIPTION metadata.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use roxide_eval::{Ctx, EvalResult};
use roxide_host::OpenMode;
use roxide_value::SEXP_NIL;

/// A dependency from DESCRIPTION.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    /// Dependency name.
    pub package: String,
    /// Optional version relation and version string.
    pub version: Option<(String, String)>,
}

/// Parsed package DESCRIPTION metadata.
#[derive(Clone, Debug, Default)]
pub struct Description {
    /// Package name.
    pub package: String,
    /// Package version.
    pub version: String,
    /// Source file order for the current platform.
    pub collate: Vec<String>,
    /// Packages required on the search path.
    pub depends: Vec<Dependency>,
    /// Packages required by the namespace.
    pub imports: Vec<Dependency>,
    /// Optional package dependencies.
    pub suggests: Vec<Dependency>,
    /// Whether the package requires compilation.
    pub needs_compilation: bool,
    /// Whether the package requests lazy loading.
    pub lazy_load: Option<bool>,
    /// Whether the package requests lazy data loading.
    pub lazy_data: Option<bool>,
    /// Build metadata from an installed package.
    pub built: Option<String>,
}

impl Description {
    /// Read DESCRIPTION from a package directory through the session host.
    pub fn read(ctx: &mut Ctx, package_dir: &Path) -> EvalResult<Self> {
        let path = package_dir.join("DESCRIPTION");
        let source = read_text(ctx, &path)?;
        Self::parse(&source, is_windows(ctx))
            .map_err(|error| ctx.stop(SEXP_NIL, format!("{}: {error}", path.display())))
    }

    /// Parse one UTF-8 DESCRIPTION record and select its platform file order.
    pub fn parse(source: &str, windows: bool) -> Result<Self, String> {
        let mut fields = BTreeMap::<&str, String>::new();
        let mut field = None;
        for line in source.trim_start_matches('\u{feff}').lines() {
            if line.starts_with('#') {
                continue;
            }
            let content = line.trim();
            if content.is_empty() {
                if !fields.is_empty() {
                    break;
                }
                continue;
            }
            if line.starts_with([' ', '\t']) {
                let value = field
                    .and_then(|key| fields.get_mut(key))
                    .ok_or_else(|| format!("invalid DESCRIPTION continuation: {content}"))?;
                value.push('\n');
                if content != "." {
                    value.push_str(content);
                }
            } else {
                let (key, value) = content
                    .split_once(':')
                    .filter(|(key, _)| !key.is_empty() && !key.contains(char::is_whitespace))
                    .ok_or_else(|| format!("invalid DESCRIPTION field: {content}"))?;
                fields.insert(key, value.trim().to_owned());
                field = Some(key);
            }
        }
        let get = |key| fields.get(key).map(String::as_str);
        let package = get("Package")
            .filter(|name| valid_package(name))
            .ok_or("invalid or missing 'Package' field")?;
        let version = get("Version")
            .filter(|version| version_parts(version).is_some())
            .ok_or("invalid or missing 'Version' field")?;
        let flag = |key| {
            get(key)
                .map(str::to_ascii_lowercase)
                .map(|v| matches!(v.as_str(), "yes" | "true"))
        };
        let platform_collate = if windows {
            "Collate.windows"
        } else {
            "Collate.unix"
        };
        Ok(Self {
            package: package.to_owned(),
            version: version.to_owned(),
            collate: collate(
                get(platform_collate)
                    .or_else(|| get("Collate"))
                    .unwrap_or(""),
            )?,
            depends: dependencies(get("Depends"))?,
            imports: dependencies(get("Imports"))?,
            suggests: dependencies(get("Suggests"))?,
            needs_compilation: flag("NeedsCompilation").unwrap_or(false),
            lazy_load: flag("LazyLoad"),
            lazy_data: flag("LazyData"),
            built: get("Built").map(str::to_owned),
        })
    }
}

pub(crate) fn valid_package(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
        && !name.ends_with('.')
}

pub(crate) fn is_windows(ctx: &mut Ctx) -> bool {
    ctx.host()
        .sys_info()
        .sysname
        .eq_ignore_ascii_case("windows")
}

pub(crate) fn version_parts(input: &str) -> Option<Vec<i32>> {
    input
        .split(['.', '-'])
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        })
        .collect()
}

fn dependencies(field: Option<&str>) -> Result<Vec<Dependency>, String> {
    field
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|entry| {
            let invalid = || format!("invalid package dependency: {entry}");
            let (package, version) = if let Some((name, requirement)) = entry.split_once('(') {
                let requirement = requirement.strip_suffix(')').ok_or_else(invalid)?.trim();
                let split = requirement
                    .find(|c: char| c.is_ascii_digit())
                    .ok_or_else(invalid)?;
                let (op, version) = requirement.split_at(split);
                let op = op.trim();
                if !matches!(op, ">=" | ">" | "<=" | "<" | "==" | "!=")
                    || version_parts(version).is_none()
                {
                    return Err(invalid());
                }
                (name.trim(), Some((op.to_owned(), version.to_owned())))
            } else {
                (entry, None)
            };
            if !valid_package(package) {
                return Err(invalid());
            }
            Ok(Dependency {
                package: package.to_owned(),
                version,
            })
        })
        .collect()
}

fn collate(source: &str) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(first) = chars.next() {
        if first.is_whitespace() {
            continue;
        }
        let mut file = String::new();
        if matches!(first, '\'' | '"') {
            loop {
                match chars.next() {
                    Some(c) if c == first => break,
                    Some('\\') => file.push(chars.next().ok_or("invalid 'Collate' escape")?),
                    Some(c) => file.push(c),
                    None => return Err("unterminated quote in 'Collate' field".to_owned()),
                }
            }
        } else {
            file.push(first);
            while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                file.push(c);
            }
        }
        if file.is_empty()
            || !Path::new(&file)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!("invalid 'Collate' path: {file}"));
        }
        files.push(file);
    }
    Ok(files)
}

pub(crate) fn read_text(ctx: &mut Ctx, path: &Path) -> EvalResult<String> {
    let limit = ctx.host().limits().max_vector_bytes;
    let file = ctx
        .host()
        .open(path, OpenMode::read_only())
        .map_err(|error| ctx.stop(SEXP_NIL, format!("{}: {error}", path.display())))?;
    let mut bytes = Vec::new();
    file.take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| ctx.stop(SEXP_NIL, format!("{}: {error}", path.display())))?;
    if bytes.len() > limit {
        return Err(ctx.stop(
            SEXP_NIL,
            format!("{}: file size limit exceeded", path.display()),
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| ctx.stop(SEXP_NIL, format!("{}: invalid UTF-8", path.display())))
}

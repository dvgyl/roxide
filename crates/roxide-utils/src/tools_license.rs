//! Classify CRAN licence expressions and map known licences to SPDX identifiers.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::cmp::Ordering;

use roxide_eval::{Args, Call, Ctx, EvalResult, Module};
use roxide_value::{CharId, NA_INTEGER, RLogical, SEXP_NIL, ValueApi};

use super::tools_rd::{chars, internal, set_attr};

/// Register the `tools::analyze_license` function.
pub const MODULE: Module = Module {
    prims: &[internal("rox_tools_license", 1, analyze)],
    prelude: r#"
analyze_license <- function(x) .Internal(rox_tools_license(as.character(x)))
"#,
};

const FIELDS: [&str; 13] = [
    "is_empty",
    "is_canonical",
    "bad_components",
    "is_standardizable",
    "is_verified",
    "standardization",
    "components",
    "expansions",
    "extensions",
    "pointers",
    "is_FOSS",
    "restricts_use",
    "spdx",
];

#[derive(Clone, Copy)]
struct License {
    expanded: &'static str,
    spdx: &'static str,
    extensible: bool,
    foss: bool,
}

const fn foss(expanded: &'static str, spdx: &'static str, extensible: bool) -> License {
    License {
        expanded,
        spdx,
        extensible,
        foss: true,
    }
}

const GPL: &[(&str, License)] = &[
    ("2", foss("GPL-2", "GPL-2.0-only", false)),
    ("3", foss("GPL-3", "GPL-3.0-only", true)),
];
const LGPL: &[(&str, License)] = &[
    ("2", foss("LGPL-2", "LGPL-2.0-only", false)),
    ("2.1", foss("LGPL-2.1", "LGPL-2.1-only", false)),
    ("3", foss("LGPL-3", "LGPL-3.0-only", true)),
];
const AGPL: &[(&str, License)] = &[("3", foss("AGPL-3", "AGPL-3.0-only", true))];
const APACHE: &[(&str, License)] = &[
    (
        "1.1",
        foss("Apache License version 1.1", "Apache-1.1", false),
    ),
    (
        "2.0",
        foss("Apache License version 2.0", "Apache-2.0", false),
    ),
];
const MPL: &[(&str, License)] = &[
    ("1.0", foss("MPL version 1.0", "MPL-1.0", false)),
    ("1.1", foss("MPL version 1.1", "MPL-1.1", false)),
    ("2.0", foss("MPL version 2.0", "MPL-2.0", false)),
];
const EPL: &[(&str, License)] = &[("1.0", foss("EPL version 1.0", "EPL-1.0", false))];

fn fixed(spec: &str) -> Option<License> {
    Some(match spec {
        "MIT" => foss("MIT", "MIT", true),
        "BSD_2_clause" => foss("BSD_2_clause", "BSD-2-Clause", true),
        "BSD_3_clause" => foss("BSD_3_clause", "BSD-3-Clause", true),
        "FreeBSD" => foss("FreeBSD", "BSD-2-Clause-FreeBSD", false),
        "CC0" => foss("CC0", "CC0-1.0", true),
        "Artistic-2.0" => foss("Artistic-2.0", "Artistic-2.0", false),
        "BSL-1.0" | "Boost Software License 1.0" | "Boost Software License version 1.0" => {
            foss("Boost Software License version 1.0", "BSL-1.0", false)
        }
        "Zlib" => foss("Zlib", "Zlib", true),
        "Unlimited" => foss("Unlimited", "", false),
        "CC BY 4.0" => foss("CC BY 4.0", "CC-BY-4.0", true),
        "CC BY-SA 4.0" => foss("CC BY-SA 4.0", "CC-BY-SA-4.0", true),
        "CC BY-NC 4.0" => License {
            foss: false,
            ..foss("CC BY-NC 4.0", "CC-BY-NC-4.0", true)
        },
        "CC BY-NC-SA 4.0" => License {
            foss: false,
            ..foss("CC BY-NC-SA 4.0", "CC-BY-NC-SA-4.0", true)
        },
        _ => return None,
    })
}

struct Component {
    name: String,
    pointer: Option<&'static str>,
    extension: bool,
    licenses: Vec<License>,
}

impl Component {
    fn parse(spec: &str) -> Option<Self> {
        let (base, extension) = spec
            .split_once(" + ")
            .map_or((spec, None), |(base, ext)| (base, Some(ext)));
        if let Some(pointer) = file_pointer(base) {
            return extension.is_none().then(|| Self {
                name: spec.to_owned(),
                pointer: Some(pointer),
                extension: false,
                licenses: Vec::new(),
            });
        }
        let pointer = match extension {
            Some(ext) if base != "Unlimited" => Some(file_pointer(ext)?),
            Some(_) => return None,
            None => None,
        };
        let (mut name, licenses) = if let Some(license) = fixed(base) {
            (base.to_owned(), vec![license])
        } else {
            versions(base)?
        };
        if let Some(pointer) = pointer {
            name.push_str(" + file ");
            name.push_str(pointer);
        }
        Some(Self {
            name,
            pointer,
            extension: extension.is_some(),
            licenses,
        })
    }

    fn expanded(&self) -> Vec<String> {
        if self.pointer.is_some() && !self.extension {
            return vec![self.name.clone()];
        }
        self.licenses
            .iter()
            .map(|lic| {
                if let Some(pointer) = self.pointer {
                    format!("{} + file {pointer}", lic.expanded)
                } else {
                    lic.expanded.to_owned()
                }
            })
            .collect()
    }
}

fn file_pointer(spec: &str) -> Option<&'static str> {
    match spec {
        "file LICENSE" => Some("LICENSE"),
        "file LICENCE" => Some("LICENCE"),
        _ => None,
    }
}

fn versions(spec: &str) -> Option<(String, Vec<License>)> {
    for (family, releases, separator, long_form) in [
        ("LGPL", LGPL, "-", false),
        ("GPL", GPL, "-", false),
        ("AGPL", AGPL, "-", false),
        ("Apache License", APACHE, " ", true),
        ("MPL", MPL, "-", true),
        ("EPL", EPL, "-", true),
    ] {
        let Some(suffix) = spec.strip_prefix(family) else {
            continue;
        };
        if suffix.is_empty() {
            return Some((
                spec.to_owned(),
                releases.iter().map(|&(_, lic)| lic).collect(),
            ));
        }
        if let Some((op, version, parts)) = constraint(suffix) {
            let versions = releases
                .iter()
                .filter_map(|&(release, lic)| {
                    let release = version_parts(release)?;
                    let ordering = compare_versions(&release, &parts);
                    let include = match op {
                        "==" => ordering == Ordering::Equal,
                        "!=" => ordering != Ordering::Equal,
                        ">=" => ordering != Ordering::Less,
                        "<=" => ordering != Ordering::Greater,
                        ">" => ordering == Ordering::Greater,
                        "<" => ordering == Ordering::Less,
                        _ => false,
                    };
                    include.then_some(lic)
                })
                .collect();
            return Some((format!("{family} ({op} {version})"), versions));
        }
        let version = suffix.strip_prefix(separator);
        if let Some((_, license)) = releases
            .iter()
            .find(|(v, lic)| version == Some(*v) || (long_form && lic.expanded == spec))
        {
            return Some((spec.to_owned(), vec![*license]));
        }
    }
    None
}

fn constraint(suffix: &str) -> Option<(&str, &str, Vec<u32>)> {
    let inner = suffix.trim().strip_prefix('(')?.strip_suffix(')')?.trim();
    let end = inner.find(|ch: char| !matches!(ch, '<' | '>' | '=' | '!'))?;
    let (op, version) = inner.split_at(end);
    if !matches!(op, "<" | ">" | "<=" | ">=" | "==" | "!=") {
        return None;
    }
    let version = version.trim();
    Some((op, version, version_parts(version)?))
}

fn version_parts(version: &str) -> Option<Vec<u32>> {
    version
        .split(['.', '-'])
        .map(|part| {
            if part.bytes().all(|ch| ch.is_ascii_digit()) {
                part.parse().ok()
            } else {
                None
            }
        })
        .collect()
}

fn compare_versions(a: &[u32], b: &[u32]) -> Ordering {
    (0..a.len().max(b.len()))
        .map(|i| a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0)))
        .find(|order| *order != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

fn alternative(spec: &str) -> Option<&'static str> {
    Some(match spec {
        "GPL 2" | "GPL version 2" => "GPL-2",
        "GPL 3" | "GPL version 3" => "GPL-3",
        "GPL 2 or later" | "GPL version 2 or later" => "GPL (>= 2)",
        "GPL 3 or later" | "GPL version 3 or later" => "GPL (>= 3)",
        "LGPL version 2" => "LGPL-2",
        "LGPL version 2.1" => "LGPL-2.1",
        "LGPL version 3" => "LGPL-3",
        "LGPL version 2.1 or later" => "LGPL (>= 2.1)",
        _ => return None,
    })
}

fn analyze(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let limit = ctx.host().limits().max_vector_bytes;
    let ids = ctx.session().str_slice(args.get(0))?;
    let source = match ids {
        [] => return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed")),
        [CharId::NA] => "",
        [id] => {
            let bytes = ctx.session().char_bytes(*id)?;
            if bytes.len() > limit / 64 {
                return Err(ctx.stop(call.err_call(), "licence expression size limit exceeded"));
            }
            std::str::from_utf8(bytes).unwrap_or_default()
        }
        _ => {
            return Err(ctx.stop(
                call.err_call(),
                format!("'length = {}' in coercion to 'logical(1)'", ids.len()),
            ));
        }
    }
    .trim();
    let empty = source.is_empty();
    let normalized = source
        .split_terminator('|')
        .map(|s| {
            s.split('+')
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(" + ")
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let mut parsed = normalized
        .iter()
        .map(|spec| Component::parse(spec))
        .collect::<Vec<_>>();
    let bad = normalized
        .iter()
        .zip(&parsed)
        .filter(|(_, c)| {
            c.as_ref()
                .is_none_or(|c| c.licenses.is_empty() && c.pointer.is_none())
        })
        .map(|(spec, c)| c.as_ref().map_or(spec.as_str(), |c| c.name.as_str()))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let canonical = !empty && bad.is_empty();
    let alias = if parsed.len() == 1 && parsed[0].is_none() {
        alternative(&normalized[0]).and_then(Component::parse)
    } else {
        None
    };
    let standardizable = canonical || alias.is_some();
    if alias.is_some() {
        parsed[0] = alias;
    }

    ctx.scoped(|ctx| {
        let expanded = !empty && parsed.iter().all(Option::is_some);
        let known = parsed
            .iter()
            .flatten()
            .flat_map(|c| &c.licenses)
            .collect::<Vec<_>>();
        let pointers = parsed
            .iter()
            .flatten()
            .filter_map(|c| c.pointer)
            .collect::<Vec<_>>();
        let verified = standardizable && known.iter().any(|lic| lic.foss);
        let file_only = parsed
            .iter()
            .flatten()
            .any(|c| c.pointer.is_some() && !c.extension);
        let foss_status = if !standardizable || (!verified && file_only) {
            RLogical::NA
        } else {
            logical(verified)
        };
        let restricts_use = foss_status
            .get()
            .map_or(RLogical::NA, |foss| logical(!foss));
        let components = normalized
            .iter()
            .zip(&parsed)
            .map(|(spec, c)| c.as_ref().map_or(spec.as_str(), |c| c.name.as_str()))
            .collect::<Vec<_>>();
        let standardized = components.join(" | ");
        let mut ids = Vec::new();
        for lic in known {
            if !lic.spdx.is_empty() && !ids.contains(&lic.spdx) {
                ids.push(lic.spdx);
            }
        }
        let spdx = ids.join(" OR ");

        let result = ctx.session_mut().alloc_list(FIELDS.len())?;
        ctx.protect(result);
        let names = chars(ctx, FIELDS)?;
        ctx.protect(names);
        set_attr(ctx, result, "names", names)?;
        let bad = chars(ctx, bad.iter().map(String::as_str))?;
        ctx.protect(bad);
        let components = if empty {
            SEXP_NIL
        } else {
            chars(ctx, components)?
        };
        ctx.protect(components);
        let na = ctx.session_mut().alloc_str_from(&[CharId::NA])?;
        ctx.protect(na);
        let standardization = if standardizable {
            ctx.session_mut().scalar_str(&standardized)?
        } else {
            na
        };
        ctx.protect(standardization);
        let spdx = if expanded && (!standardizable || (spdx.is_empty() && file_only && !verified)) {
            na
        } else if expanded {
            ctx.session_mut().scalar_str(&spdx)?
        } else {
            ctx.session_mut().scalar_str("")?
        };
        ctx.protect(spdx);

        let (expansions, extensions, pointers) = if expanded {
            let expansions = ctx.session_mut().alloc_list(parsed.len())?;
            ctx.protect(expansions);
            for (i, component) in parsed.iter().flatten().enumerate() {
                let lines = component.expanded();
                let entry = chars(ctx, lines.iter().map(String::as_str))?;
                ctx.session_mut().list_set(expansions, i, entry)?;
            }
            let extensions = build_extensions(ctx, &parsed)?;
            ctx.protect(extensions);
            let pointers = if pointers.is_empty() {
                SEXP_NIL
            } else {
                chars(ctx, pointers)?
            };
            ctx.protect(pointers);
            (expansions, extensions, pointers)
        } else {
            (SEXP_NIL, SEXP_NIL, SEXP_NIL)
        };

        for (index, flag) in [
            (0, logical(empty)),
            (1, logical(canonical)),
            (3, logical(standardizable)),
            (4, logical(verified)),
            (10, foss_status),
            (11, restricts_use),
        ] {
            let value = ctx.session_mut().scalar_lgl(flag)?;
            ctx.session_mut().list_set(result, index, value)?;
        }
        for (index, value) in [
            (2, bad),
            (5, standardization),
            (6, components),
            (7, expansions),
            (8, extensions),
            (9, pointers),
            (12, spdx),
        ] {
            ctx.session_mut().list_set(result, index, value)?;
        }
        Ok(result)
    })
}

fn build_extensions(ctx: &mut Ctx, components: &[Option<Component>]) -> EvalResult {
    let rows = components
        .iter()
        .flatten()
        .filter(|c| c.extension)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Ok(SEXP_NIL);
    }
    ctx.scoped(|ctx| {
        let extensions = ctx.session_mut().alloc_list(2)?;
        ctx.protect(extensions);
        let column = chars(ctx, rows.iter().map(|c| c.name.as_str()))?;
        ctx.session_mut().list_set(extensions, 0, column)?;
        let flags = rows
            .iter()
            .map(|c| logical(c.licenses.iter().any(|lic| lic.extensible)))
            .collect::<Vec<_>>();
        let column = ctx.session_mut().alloc_lgl_from(&flags)?;
        ctx.session_mut().list_set(extensions, 1, column)?;
        for (attr, values) in [
            ("names", &["components", "extensible"][..]),
            ("class", &["data.frame"][..]),
        ] {
            let value = chars(ctx, values.iter().copied())?;
            ctx.protect(value);
            set_attr(ctx, extensions, attr, value)?;
        }
        let names = ctx
            .session_mut()
            .alloc_int_from(&[NA_INTEGER, -(rows.len() as i32)])?;
        ctx.protect(names);
        set_attr(ctx, extensions, "row.names", names)?;
        Ok(extensions)
    })
}

fn logical(value: bool) -> RLogical {
    RLogical::from_option(Some(value))
}

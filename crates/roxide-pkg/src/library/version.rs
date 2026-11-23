use std::cmp::Ordering;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{BASE_NAMESPACE, RLogical, SEXP_NIL, SexpType, ValueApi};

use crate::description::{Dependency, Description, version_parts};
use crate::loader::{namespace_version, values::*};

fn compare(left: &[i32], right: &[i32]) -> Ordering {
    (0..left.len().max(right.len()))
        .map(|i| {
            left.get(i)
                .copied()
                .unwrap_or(0)
                .cmp(&right.get(i).copied().unwrap_or(0))
        })
        .find(|&order| order != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

fn relation(order: Ordering, op: &str) -> Option<bool> {
    Some(match op {
        "==" => order.is_eq(),
        "!=" => order.is_ne(),
        "<" => order.is_lt(),
        "<=" => order.is_le(),
        ">" => order.is_gt(),
        ">=" => order.is_ge(),
        _ => return None,
    })
}

fn check(
    ctx: &mut Ctx,
    pkg: &str,
    actual: &str,
    op: &str,
    required: &str,
    loaded: bool,
) -> EvalResult<()> {
    let actual_parts =
        version_parts(actual).ok_or_else(|| ctx.stop(SEXP_NIL, "invalid namespace version"))?;
    let required_parts =
        version_parts(required).ok_or_else(|| ctx.stop(SEXP_NIL, "invalid version requirement"))?;
    match relation(compare(&actual_parts, &required_parts), op) {
        Some(true) => Ok(()),
        Some(false) => {
            let status = if loaded {
                "already loaded"
            } else {
                "being loaded"
            };
            Err(ctx.stop(
                SEXP_NIL,
                format!("namespace ‘{pkg}’ {actual} is {status}, but {op} {required} is required"),
            ))
        }
        None => Err(ctx.stop(SEXP_NIL, "invalid version comparison")),
    }
}

pub(crate) fn check_dependency(
    ctx: &mut Ctx,
    dependency: &Dependency,
    actual: &str,
) -> EvalResult<()> {
    if let Some((op, required)) = &dependency.version {
        check(ctx, &dependency.package, actual, op, required, true)?;
    }
    Ok(())
}

pub(crate) fn version_check(
    ctx: &mut Ctx,
    package: &str,
    actual: &str,
    condition: SexpId,
    loaded: bool,
) -> EvalResult<()> {
    if condition == SEXP_NIL {
        return Ok(());
    }
    let op = field(ctx, condition, "op")?.unwrap_or(SEXP_NIL);
    let op = string(ctx, op)?;
    let wanted = field(ctx, condition, "version")?.unwrap_or(SEXP_NIL);
    let requested = parts(ctx, wanted)?;
    let requested = requested
        .first()
        .and_then(Option::as_deref)
        .ok_or_else(|| ctx.stop(SEXP_NIL, "invalid version requirement"))?;
    check(ctx, package, actual, &op, &display(requested), loaded)
}

fn display(parts: &[i32]) -> String {
    parts
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn parts(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<Option<Vec<i32>>>> {
    if ctx.session().type_of(value)? == SexpType::Vec {
        let entries = ctx.session().list_slice(value)?.to_vec();
        return entries
            .into_iter()
            .map(|entry| {
                let parts: Vec<_> = ctx.session().int(entry)?.iter().collect();
                Ok((!parts.is_empty()).then_some(parts))
            })
            .collect();
    }
    let (strs, _) = ctx.session_mut().coerce(value, SexpType::Str)?;
    names(ctx, strs)?
        .into_iter()
        .map(|s| match s {
            Some(s) => version_parts(&s)
                .map(Some)
                .ok_or_else(|| ctx.stop(SEXP_NIL, format!("invalid version specification ‘{s}’"))),
            None => Ok(None),
        })
        .collect()
}

fn make(ctx: &mut Ctx, parts: &[Option<Vec<i32>>], class: &[&str]) -> EvalResult<SexpId> {
    let mut entries = Vec::with_capacity(parts.len());
    for parts in parts {
        entries.push(
            ctx.session_mut()
                .alloc_int_from(parts.as_deref().unwrap_or_default())?,
        );
    }
    let value = list(ctx, &entries)?;
    let class = strings(ctx, class.iter().copied())?;
    set_attr(ctx, value, "class", class)?;
    Ok(value)
}

const NUMERIC: &[&str] = &["numeric_version"];
const PACKAGE: &[&str] = &["package_version", "numeric_version"];
const SYSTEM: &[&str] = &["R_system_version", "package_version", "numeric_version"];

pub(super) fn construct(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let input = args.get(0);
    let strict = flag(ctx, args.get(1))?;
    let kind = string(ctx, args.get(2))?;
    if kind == "system" {
        return make(ctx, &[version_parts(super::R_VERSION)], SYSTEM);
    }
    let class = if kind == "package" { PACKAGE } else { NUMERIC };
    let ty = ctx.session().type_of(input)?;
    if ty == SexpType::Vec {
        let input_class = attr(ctx, input, "class")?;
        if chars(ctx, input_class)?.iter().any(|name| name == class[0]) {
            return Ok(input);
        }
    }
    if input == SEXP_NIL {
        return make(ctx, &[], class);
    }
    if ty != SexpType::Str {
        return Err(ctx.stop(
            SEXP_NIL,
            format!(
                "invalid non-character version specification 'x' (type: {})",
                ty.type_name(false)
            ),
        ));
    }
    let mut invalid = Vec::new();
    let parsed: Vec<_> = names(ctx, input)?
        .into_iter()
        .map(|item| {
            let text = item.as_deref()?;
            let value = version_parts(text).filter(|parts| kind != "package" || parts.len() >= 2);
            if value.is_none() {
                invalid.push(text.to_owned());
            }
            value
        })
        .collect();
    if strict && !invalid.is_empty() {
        return Err(ctx.stop(
            SEXP_NIL,
            format!("invalid version specification ‘{}’", invalid.join("’, ‘")),
        ));
    }
    make(ctx, &parsed, class)
}

pub(super) fn ops(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let op = string(ctx, args.get(0))?;
    if relation(Ordering::Equal, &op).is_none() {
        return Err(ctx.stop(
            call.err_call(),
            format!("operator '{op}' not defined for numeric_version objects"),
        ));
    }
    let left = parts(ctx, args.get(1))?;
    let right = parts(ctx, args.get(2))?;
    let len = if left.is_empty() || right.is_empty() {
        0
    } else {
        left.len().max(right.len())
    };
    if len > 0 && !len.is_multiple_of(left.len().min(right.len())) {
        ctx.warn_now(
            call.err_call(),
            "longer object length is not a multiple of shorter object length",
        )?;
    }
    let result: Vec<_> = (0..len)
        .map(|i| {
            let value = left[i % left.len()]
                .as_ref()
                .zip(right[i % right.len()].as_ref())
                .and_then(|(left, right)| relation(compare(left, right), &op));
            RLogical::from_option(value)
        })
        .collect();
    Ok(ctx.session_mut().alloc_lgl_from(&result)?)
}

pub(super) fn format(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let parts = parts(ctx, args.get(0))?;
    let formatted: Vec<_> = parts
        .iter()
        .map(|parts| parts.as_deref().map(display))
        .collect();
    optional_strings(ctx, formatted.iter().map(Option::as_deref))
}

pub(super) fn installed(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let name = string(ctx, args.get(0))?;
    let paths = super::selected_libraries(ctx, args.get(1))?;
    let version = if let Some(path) = super::find(ctx, &name, &paths) {
        Description::read(ctx, &path)?.version
    } else if args.get(1) == SEXP_NIL
        && let Some(ns) = ctx.ns_lookup(&name)
    {
        if ns == BASE_NAMESPACE {
            super::R_VERSION.to_owned()
        } else {
            namespace_version(ctx, ns)?
        }
    } else {
        return Err(ctx.stop(
            call.err_call(),
            format!("there is no package called ‘{name}’"),
        ));
    };
    make(ctx, &[version_parts(&version)], PACKAGE)
}

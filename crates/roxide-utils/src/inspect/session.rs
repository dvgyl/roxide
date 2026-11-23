use roxide_base::scan::{base_call, r_strings};
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{RSession, SEXP_NIL, ValueApi};

use super::{set, strings};

const BASE: &[&str] = &[
    "stats",
    "graphics",
    "grDevices",
    "utils",
    "datasets",
    "methods",
    "base",
];

pub(super) fn info(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let host = ctx.host();
    let system = host.sys_info();
    let locale = host.locale().ctype;
    let timezone = host.env_var("TZ").filter(|tz| !tz.is_empty()).map_or_else(
        || "UTC".into(),
        |tz| String::from_utf8_lossy(&tz).into_owned(),
    );
    let running = format!("{} {}", system.sysname, system.release)
        .trim()
        .to_owned();
    let requested = if args.get(0) == SEXP_NIL {
        BASE.iter().map(|&pkg| pkg.to_owned()).collect()
    } else {
        r_strings(ctx, args.get(0))?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
    };
    let kind = base_call(ctx, call, "RNGkind", &[])?;
    ctx.protect(kind);
    let s = ctx.session_mut();
    let os = system
        .platform
        .rsplit('-')
        .next()
        .unwrap_or(&system.sysname);
    let sys = format!("{}, {os}", system.machine);
    let version = fields(
        s,
        &[
            ("platform", &system.platform),
            ("arch", &system.machine),
            ("os", os),
            ("system", &sys),
            ("status", ""),
            ("major", "4"),
            ("minor", "6.2"),
            ("year", "2026"),
            ("month", "10"),
            ("day", "29"),
            ("svn rev", "90784"),
            ("language", "R"),
            ("version.string", "R version 4.6.2 (2026-10-29)"),
            ("nickname", "Diversified Portfolio"),
        ],
    )?;
    let pkgs = strings(
        s,
        requested.iter().filter(|pkg| BASE.contains(&pkg.as_str())),
    )?;
    let compiler = fields(
        s,
        &[
            ("Package", "compiler"),
            ("Version", "4.6.2"),
            ("Priority", "base"),
        ],
    )?;
    let mut loaded = vec![("compiler", compiler)];
    for &pkg in BASE
        .iter()
        .filter(|&&pkg| !requested.iter().any(|name| name == pkg))
    {
        let description = fields(
            s,
            &[("Package", pkg), ("Version", "4.6.2"), ("Priority", "base")],
        )?;
        loaded.push((pkg, description));
    }
    let loaded = named(s, &loaded)?;
    let platform = s.scalar_str(&system.platform)?;
    let running = s.scalar_str(&running)?;
    let locale = strings(s, [&locale])?;
    let tz = s.scalar_str(&timezone)?;
    let tz_type = s.scalar_str("internal")?;
    let matprod = s.scalar_str("default")?;
    let unavailable = s.scalar_str("")?;
    let result = named(
        s,
        &[
            ("R.version", version),
            ("platform", platform),
            ("locale", locale),
            ("tzone", tz),
            ("tzcode_type", tz_type),
            ("running", running),
            ("RNGkind", kind),
            ("basePkgs", pkgs),
            ("loadedOnly", loaded),
            ("matprod", matprod),
            ("BLAS", unavailable),
            ("LAPACK", unavailable),
            ("LA_version", unavailable),
        ],
    )?;
    let class = s.scalar_str("sessionInfo")?;
    set(s, result, "class", class)?;
    Ok(result)
}

fn fields(s: &mut RSession, values: &[(&str, &str)]) -> EvalResult {
    let mut fields = Vec::with_capacity(values.len());
    for &(name, value) in values {
        fields.push((name, s.scalar_str(value)?));
    }
    named(s, &fields)
}

fn named(s: &mut RSession, entries: &[(&str, SexpId)]) -> EvalResult {
    let result = s.alloc_list(entries.len())?;
    for (i, &(_, value)) in entries.iter().enumerate() {
        s.list_set(result, i, value)?;
    }
    let names = strings(s, entries.iter().map(|&(name, _)| name))?;
    set(s, result, "names", names)?;
    Ok(result)
}

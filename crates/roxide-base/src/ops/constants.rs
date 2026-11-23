use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{RSession, ValueApi};

use crate::ops::common::{ch, flag, list, set_names};

pub(crate) const MODULE: Module = Module {
    prims: &[
        internal("rox_base_platform", platform, 0),
        internal("rox_base_machine", machine, 0),
        internal("rox_base_letters", letters, 1),
        internal("rox_base_months", months, 1),
    ],
    prelude: r#"
T <- TRUE
F <- FALSE
pi <- 3.141592653589793
letters <- .Internal(rox_base_letters(FALSE))
LETTERS <- .Internal(rox_base_letters(TRUE))
month.name <- .Internal(rox_base_months(FALSE))
month.abb <- .Internal(rox_base_months(TRUE))
.Platform <- .Internal(rox_base_platform())
.Library <- file.path(Sys.getenv("R_HOME", Sys.getenv("ROXIDE_R_HOME", ".")), "library")
.Machine <- .Internal(rox_base_machine())
"#,
};

const fn internal(name: &'static str, fun: roxide_eval::PrimFn, count: usize) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(count),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
    }
}

fn letters(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let first = if flag(ctx, args.get(0), false)? {
        b'A'
    } else {
        b'a'
    };
    let s = ctx.session_mut();
    let values = (0..26)
        .map(|i| s.mk_char(&[first + i], roxide_value::Encoding::Native))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(s.alloc_str_from(&values)?)
}

fn months(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let abbreviate = flag(ctx, args.get(0), false)?;
    let s = ctx.session_mut();
    let values = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ]
    .into_iter()
    .map(|name| ch(s, if abbreviate { &name[..3] } else { name }))
    .collect::<Result<Vec<_>, _>>()?;
    Ok(s.alloc_str_from(&values)?)
}

fn windows(ctx: &mut Ctx) -> bool {
    ctx.host()
        .sys_info()
        .sysname
        .eq_ignore_ascii_case("windows")
}

fn platform(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let windows = windows(ctx);
    let endian = if cfg!(target_endian = "little") {
        "little"
    } else {
        "big"
    };
    let fields = [
        ("OS.type", if windows { "windows" } else { "unix" }),
        ("file.sep", "/"),
        ("dynlib.ext", if windows { ".dll" } else { ".so" }),
        ("GUI", if windows { "RTerm" } else { "X11" }),
        ("endian", endian),
        ("pkgType", if windows { "win.binary" } else { "source" }),
        ("path.sep", if windows { ";" } else { ":" }),
        ("r_arch", ""),
    ];
    let s = ctx.session_mut();
    let values = fields
        .iter()
        .map(|&(n, v)| Ok((n, s.scalar_str(v)?)))
        .collect::<Result<Vec<_>, roxide_value::ValueError>>()?;
    named_list(s, &values)
}

fn machine(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let pointer = size_of::<usize>() as i32;
    let long = if windows(ctx) { 4 } else { pointer };
    let s = ctx.session_mut();
    let mut fields = Vec::new();
    for (name, value) in [
        ("double.eps", f64::EPSILON),
        ("double.neg.eps", f64::EPSILON / 2.0),
        ("double.xmin", f64::MIN_POSITIVE),
        ("double.xmax", f64::MAX),
    ] {
        fields.push((name, s.scalar_real(value)?));
    }
    for (name, value) in [
        ("double.base", f64::RADIX as i32),
        ("double.digits", f64::MANTISSA_DIGITS as i32),
        ("double.rounding", 5),
        ("double.guard", 0),
        ("double.ulp.digits", 1 - f64::MANTISSA_DIGITS as i32),
        ("double.neg.ulp.digits", -(f64::MANTISSA_DIGITS as i32)),
        ("double.exponent", 11),
        ("double.min.exp", f64::MIN_EXP - 1),
        ("double.max.exp", f64::MAX_EXP),
        ("integer.max", i32::MAX),
        ("sizeof.long", long),
        ("sizeof.longlong", 8),
        ("sizeof.longdouble", 8),
        ("sizeof.pointer", pointer),
        ("sizeof.time_t", 8),
    ] {
        fields.push((name, s.scalar_int(value)?));
    }
    named_list(s, &fields)
}

fn named_list(s: &mut RSession, fields: &[(&str, SexpId)]) -> EvalResult {
    let values: Vec<_> = fields.iter().map(|&(_, v)| v).collect();
    let result = list(s, &values)?;
    let names = fields
        .iter()
        .map(|&(n, _)| ch(s, n))
        .collect::<Result<Vec<_>, _>>()?;
    set_names(s, result, &names)?;
    Ok(result)
}

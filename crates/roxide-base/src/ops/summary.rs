use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{
    CharId, NA_INTEGER, NA_REAL, RComplex, RLogical, SEXP_MISSING, SexpType, ValueApi, int_add,
    is_na_real, nan_propagate,
};

use super::common::{copy_attrs, error, flag, formals, number, rank, tag};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reduce {
    All,
    Any,
    Sum,
    Prod,
    Min,
    Max,
    Range,
}

macro_rules! reduce {
    ($($name:ident:$op:ident),*) => {$(
        pub(super) fn $name(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            reduction(ctx, call, args, Reduce::$op)
        }
    )*};
}

reduce!(all:All, any:Any, sum:Sum, prod:Prod, min:Min, max:Max, range:Range);

fn reduction(ctx: &mut Ctx, call: &Call, args: &Args, op: Reduce) -> EvalResult {
    let na = args.named(ctx.session(), "na.rm").unwrap_or(SEXP_MISSING);
    let rm = flag(ctx, na, false)?;
    let inputs: Vec<_> = args
        .iter()
        .filter(|(name, _)| tag(ctx.session(), *name) != Some("na.rm"))
        .map(|(_, v)| v)
        .collect();
    if matches!(op, Reduce::All | Reduce::Any) {
        let all = op == Reduce::All;
        let mut missing = false;
        for x in inputs {
            let s = ctx.session();
            let ty = s.type_of(x)?;
            if s.xlength(x)? == 0 {
                continue;
            }
            if !matches!(ty, SexpType::Lgl | SexpType::Int) {
                ctx.warn_now(
                    call.err_call(),
                    format!(
                        "coercing argument of type '{}' to logical",
                        ty.type_name(false)
                    ),
                )?;
            }
            let (x, _) = ctx.session_mut().coerce(x, SexpType::Lgl)?;
            for v in ctx.session().lgl(x)?.iter() {
                if v.get() == Some(!all) {
                    return Ok(if all {
                        roxide_value::FALSE_SCALAR
                    } else {
                        roxide_value::TRUE_SCALAR
                    });
                }
                missing |= v.get().is_none();
            }
        }
        return Ok(ctx.session_mut().scalar_lgl(if missing && !rm {
            RLogical::NA
        } else {
            RLogical::from_option(Some(all))
        })?);
    }

    let extrema = matches!(op, Reduce::Min | Reduce::Max | Reduce::Range);
    let s = ctx.session_mut();
    let mut ty = SexpType::Int;
    for &x in &inputs {
        let kind = s.type_of(x)?;
        let valid = matches!(
            kind,
            SexpType::Nil | SexpType::Lgl | SexpType::Int | SexpType::Real
        ) || if extrema {
            kind == SexpType::Str
        } else {
            kind == SexpType::Cplx
        };
        if !valid {
            return Err(error(
                ctx,
                call,
                format!("invalid 'type' ({}) of argument", kind.type_name(false)),
            ));
        }
        if rank(kind) > rank(ty) {
            ty = kind;
        }
    }
    if ty == SexpType::Cplx {
        let mut acc = RComplex {
            re: if op == Reduce::Sum { 0.0 } else { 1.0 },
            im: 0.0,
        };
        for x in inputs {
            let (x, _) = s.coerce(x, ty)?;
            for v in s.cplx(x)?.iter() {
                if rm && (v.re.is_nan() || v.im.is_nan()) {
                    continue;
                }
                if acc.is_na() || v.is_na() {
                    acc = RComplex::NA;
                } else if op == Reduce::Sum {
                    acc = RComplex {
                        re: acc.re + v.re,
                        im: acc.im + v.im,
                    };
                } else {
                    acc = RComplex {
                        re: acc.re * v.re - acc.im * v.im,
                        im: acc.re * v.im + acc.im * v.re,
                    };
                }
            }
        }
        return Ok(s.alloc_cplx_from(&[acc])?);
    }
    if ty == SexpType::Str {
        let mut lo = None;
        let mut hi = None;
        let mut missing = false;
        for x in inputs {
            let (x, _) = s.coerce(x, ty)?;
            for &v in s.str_slice(x)? {
                if v == CharId::NA {
                    missing |= !rm;
                } else {
                    let bytes = s.char_bytes(v)?;
                    if lo.is_none_or(|x| s.char_bytes(x).ok().is_some_and(|b| bytes < b)) {
                        lo = Some(v);
                    }
                    if hi.is_none_or(|x| s.char_bytes(x).ok().is_some_and(|b| bytes > b)) {
                        hi = Some(v);
                    }
                }
            }
        }
        if lo.is_none() && !missing {
            warn_empty(ctx, call, op, true)?;
        }
        let lo = if missing {
            CharId::NA
        } else {
            lo.unwrap_or(CharId::NA)
        };
        let hi = if missing {
            CharId::NA
        } else {
            hi.unwrap_or(CharId::NA)
        };
        return Ok(ctx.session_mut().alloc_str_from(&ends(lo, hi, op))?);
    }
    if ty == SexpType::Int && op == Reduce::Sum {
        let mut total = 0i64;
        for x in inputs {
            let (x, _) = s.coerce(x, SexpType::Int)?;
            for v in s.int(x)?.to_vec() {
                if v == NA_INTEGER {
                    if !rm {
                        return Ok(s.scalar_int(NA_INTEGER)?);
                    }
                } else {
                    total += i64::from(v);
                }
            }
        }
        return Ok(match i32::try_from(total) {
            Ok(v) if v != NA_INTEGER => s.scalar_int(v)?,
            _ => s.scalar_real(total as f64)?,
        });
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut acc = if op == Reduce::Prod { 1.0 } else { 0.0 };
    let mut seen = false;
    let mut missing = None;
    for x in inputs {
        let (x, _) = s.coerce(x, SexpType::Real)?;
        for v in s.real(x)?.iter() {
            if v.is_nan() {
                if !rm && (missing.is_none() || is_na_real(v)) {
                    missing = Some(v);
                }
                continue;
            }
            seen = true;
            lo = lo.min(v);
            hi = hi.max(v);
            if op == Reduce::Sum {
                acc += v;
            }
            if op == Reduce::Prod {
                acc *= v;
            }
        }
    }
    if extrema {
        if let Some(v) = missing {
            lo = v;
            hi = v;
        }
        if !seen && missing.is_none() {
            warn_empty(ctx, call, op, false)?;
        }
        let ends = ends(lo, hi, op);
        let s = ctx.session_mut();
        if ty == SexpType::Int && (seen || missing.is_some()) {
            let values: Vec<_> = ends
                .into_iter()
                .map(|v| if v.is_nan() { NA_INTEGER } else { v as i32 })
                .collect();
            return Ok(s.alloc_int_from(&values)?);
        }
        return Ok(s.alloc_real_from(&ends)?);
    }
    Ok(s.scalar_real(missing.unwrap_or(acc))?)
}

fn ends<T>(lo: T, hi: T, op: Reduce) -> Vec<T> {
    match op {
        Reduce::Min => vec![lo],
        Reduce::Max => vec![hi],
        _ => vec![lo, hi],
    }
}

fn warn_empty(ctx: &mut Ctx, call: &Call, op: Reduce, character: bool) -> EvalResult<()> {
    for (enabled, name, value) in [
        (op != Reduce::Max, "min", "Inf"),
        (op != Reduce::Min, "max", "-Inf"),
    ] {
        if enabled {
            let value = if character { "NA" } else { value };
            ctx.warn_now(
                call.err_call(),
                format!("no non-missing arguments to {name}; returning {value}"),
            )?;
        }
    }
    Ok(())
}

macro_rules! cumulative {
    ($($name:ident:$op:ident),*) => {$(
        pub(super) fn $name(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            cumul(ctx, call, args, Reduce::$op)
        }
    )*};
}

cumulative!(cumsum:Sum, cumprod:Prod, cummin:Min, cummax:Max);

fn cumul(ctx: &mut Ctx, call: &Call, args: &Args, op: Reduce) -> EvalResult {
    let x = args.get(0);
    let mut overflow = false;
    let s = ctx.session_mut();
    let out = match s.type_of(x)? {
        SexpType::Lgl | SexpType::Int if op != Reduce::Prod => {
            let (v, _) = s.coerce(x, SexpType::Int)?;
            let mut acc = match op {
                Reduce::Min => i32::MAX,
                Reduce::Max => -i32::MAX,
                _ => 0,
            };
            let out: Vec<_> = s
                .int(v)?
                .iter()
                .map(|v| {
                    acc = if acc == NA_INTEGER || v == NA_INTEGER {
                        NA_INTEGER
                    } else {
                        match op {
                            Reduce::Min => acc.min(v),
                            Reduce::Max => acc.max(v),
                            _ => {
                                let (res, warn) = int_add(acc, v);
                                overflow |= warn;
                                res
                            }
                        }
                    };
                    acc
                })
                .collect();
            s.alloc_int_from(&out)?
        }
        SexpType::Cplx => {
            if !matches!(op, Reduce::Sum | Reduce::Prod) {
                return Err(error(ctx, call, "min/max not defined for complex numbers"));
            }
            let mut acc = RComplex {
                re: if op == Reduce::Sum { 0.0 } else { 1.0 },
                im: 0.0,
            };
            let out: Vec<_> = s
                .cplx(x)?
                .iter()
                .map(|v| {
                    acc = if acc.is_na() || v.is_na() {
                        RComplex::NA
                    } else if op == Reduce::Sum {
                        RComplex {
                            re: acc.re + v.re,
                            im: acc.im + v.im,
                        }
                    } else {
                        RComplex {
                            re: acc.re * v.re - acc.im * v.im,
                            im: acc.re * v.im + acc.im * v.re,
                        }
                    };
                    acc
                })
                .collect();
            s.alloc_cplx_from(&out)?
        }
        _ => {
            let (v, _) = s.coerce(x, SexpType::Real)?;
            let mut acc = match op {
                Reduce::Prod => 1.0,
                Reduce::Min => f64::INFINITY,
                Reduce::Max => f64::NEG_INFINITY,
                _ => 0.0,
            };
            let out: Vec<_> = s
                .real(v)?
                .iter()
                .map(|v| {
                    acc = if is_na_real(v) || is_na_real(acc) {
                        NA_REAL
                    } else if v.is_nan() || acc.is_nan() {
                        nan_propagate(acc, v)
                    } else {
                        match op {
                            Reduce::Prod => acc * v,
                            Reduce::Min => acc.min(v),
                            Reduce::Max => acc.max(v),
                            _ => acc + v,
                        }
                    };
                    acc
                })
                .collect();
            s.alloc_real_from(&out)?
        }
    };
    copy_attrs(s, x, out, |name| name == "names")?;
    if overflow {
        ctx.protect(out);
        ctx.warn_now(
            call.err_call(),
            "integer overflow in 'cumsum'; use 'cumsum(as.numeric(.))'",
        )?;
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Math {
    Abs,
    Floor,
    Ceiling,
    Trunc,
    Round,
    Sign,
    Sqrt,
    Exp,
    Log,
    Sin,
    Cos,
    Tan,
}

macro_rules! math {
    ($($name:ident:$op:ident),*) => {$(
        pub(super) fn $name(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            math_op(ctx, call, args, Math::$op)
        }
    )*};
}

math!(abs:Abs, floor:Floor, ceiling:Ceiling, trunc:Trunc, round:Round, sign:Sign,
    sqrt:Sqrt, exp:Exp, log:Log, sin:Sin, cos:Cos, tan:Tan);

fn math_op(ctx: &mut Ctx, call: &Call, args: &Args, op: Math) -> EvalResult {
    let parameter = if op == Math::Round { "digits" } else { "base" };
    let [x, extra] = formals(ctx.session(), args, ["x", parameter]);
    let s = ctx.session_mut();
    let ty = s.type_of(x)?;
    let mut warn = false;
    let out = match ty {
        SexpType::Lgl | SexpType::Int if op == Math::Abs => {
            let (v, _) = s.coerce(x, SexpType::Int)?;
            let values: Vec<_> = s.int(v)?.iter().map(i32::wrapping_abs).collect();
            s.alloc_int_from(&values)?
        }
        SexpType::Cplx if op == Math::Abs => {
            let values: Vec<_> = s
                .cplx(x)?
                .iter()
                .map(|c| if c.is_na() { NA_REAL } else { c.re.hypot(c.im) })
                .collect();
            s.alloc_real_from(&values)?
        }
        SexpType::Cplx
            if matches!(
                op,
                Math::Sqrt
                    | Math::Exp
                    | Math::Log
                    | Math::Sin
                    | Math::Cos
                    | Math::Tan
                    | Math::Round
            ) =>
        {
            let extra = if extra == SEXP_MISSING {
                if op == Math::Round {
                    0.0
                } else {
                    std::f64::consts::E
                }
            } else {
                number(s, extra)?
            };
            let values: Vec<_> = s
                .cplx(x)?
                .iter()
                .map(|z| complex_math(z, op, extra))
                .collect();
            s.alloc_cplx_from(&values)?
        }
        SexpType::Lgl | SexpType::Int | SexpType::Real => {
            let extra = if extra == SEXP_MISSING {
                if op == Math::Round {
                    0.0
                } else {
                    std::f64::consts::E
                }
            } else {
                number(s, extra)?
            };
            let (v, _) = s.coerce(x, SexpType::Real)?;
            let values: Vec<_> = s
                .real(v)?
                .iter()
                .map(|v| {
                    if v.is_nan() {
                        return v;
                    }
                    let res = match op {
                        Math::Abs => v.abs(),
                        Math::Floor => v.floor(),
                        Math::Ceiling => v.ceil(),
                        Math::Trunc => v.trunc(),
                        Math::Round => round_value(v, extra),
                        Math::Sign => {
                            if v == 0.0 {
                                0.0
                            } else {
                                v.signum()
                            }
                        }
                        Math::Sqrt => v.sqrt(),
                        Math::Exp => v.exp(),
                        Math::Log => {
                            if extra == std::f64::consts::E {
                                v.ln()
                            } else {
                                v.log(extra)
                            }
                        }
                        Math::Sin => v.sin(),
                        Math::Cos => v.cos(),
                        Math::Tan => v.tan(),
                    };
                    warn |= res.is_nan() && !extra.is_nan();
                    res
                })
                .collect();
            s.alloc_real_from(&values)?
        }
        _ => {
            return Err(error(
                ctx,
                call,
                "non-numeric argument to mathematical function",
            ));
        }
    };
    copy_attrs(s, x, out, |_| true)?;
    if warn {
        ctx.protect(out);
        ctx.warn_now(call.err_call(), "NaNs produced")?;
    }
    Ok(out)
}

fn round_value(x: f64, digits: f64) -> f64 {
    if digits.is_nan() {
        return nan_propagate(x, digits);
    }
    if !x.is_finite() || digits > 308.0 {
        return x;
    }
    if digits < -308.0 {
        return 0.0_f64.copysign(x);
    }
    let digits = digits.round() as i32;
    let scale = 10_f64.powi(digits.abs());
    let (scaled, residual) = if digits >= 0 {
        let scaled = x * scale;
        (scaled, x.mul_add(scale, -scaled))
    } else {
        let scaled = x / scale;
        (scaled, (-scaled).mul_add(scale, x))
    };
    if !scaled.is_finite() {
        return x;
    }
    // A rounded product can look like a tie. Use the residual to find its side.
    let rounded = if scaled.fract().abs() == 0.5 && residual != 0.0 {
        if residual.is_sign_positive() {
            scaled.ceil()
        } else {
            scaled.floor()
        }
    } else {
        scaled.round_ties_even()
    };
    if digits >= 0 {
        rounded / scale
    } else {
        rounded * scale
    }
}

fn complex_math(z: RComplex, op: Math, extra: f64) -> RComplex {
    if z.is_na() {
        return RComplex::NA;
    }
    let RComplex { re: x, im: y } = z;
    let (re, im) = match op {
        Math::Sqrt => {
            let r = (x.hypot(y) / 2.0 + x.abs() / 2.0).sqrt();
            if r == 0.0 {
                (0.0, y)
            } else if x >= 0.0 {
                (r, y / (2.0 * r))
            } else {
                (y.abs() / (2.0 * r), r.copysign(y))
            }
        }
        Math::Exp => (x.exp() * y.cos(), x.exp() * y.sin()),
        Math::Log => (x.hypot(y).ln() / extra.ln(), y.atan2(x) / extra.ln()),
        Math::Sin => (x.sin() * y.cosh(), x.cos() * y.sinh()),
        Math::Cos => (x.cos() * y.cosh(), -x.sin() * y.sinh()),
        Math::Tan => (
            (2.0 * x).sin() / ((2.0 * x).cos() + (2.0 * y).cosh()),
            if y.abs() > 350.0 {
                y.signum()
            } else {
                (2.0 * y).sinh() / ((2.0 * x).cos() + (2.0 * y).cosh())
            },
        ),
        _ => (round_value(x, extra), round_value(y, extra)),
    };
    RComplex { re, im }
}

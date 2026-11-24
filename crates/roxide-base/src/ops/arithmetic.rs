use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, NA_INTEGER, RComplex, RLogical, SEXP_NIL, SexpType, ValueApi, int_add, int_mul,
    int_sub, nan_propagate,
};
use std::cmp::Ordering;

use roxide_value::na::R_NAN;

use super::common::{attr, copy_attrs, error, rank, set, vector_limit};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Idiv,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Xor,
}

macro_rules! binary {
    ($($f:ident:$op:ident),* $(,)?) => {$(
        pub(super) fn $f(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            binary_op(ctx, call, args, Op::$op)
        }
    )*};
}
binary!(add:Add, sub:Sub, mul:Mul, div:Div, pow:Pow, idiv:Idiv, rem:Rem,
    eq:Eq, ne:Ne, lt:Lt, le:Le, gt:Gt, ge:Ge, and:And, or:Or, xor:Xor);

fn length(ctx: &mut Ctx, call: &Call, x: SexpId, y: SexpId) -> EvalResult<(usize, usize)> {
    let s = ctx.session();
    let a = s.xlength(x)?;
    let b = s.xlength(y)?;
    let dx = attr(s, x, "dim");
    let dy = attr(s, y, "dim");
    if dx != SEXP_NIL && dy != SEXP_NIL && s.int(dx)?.to_vec() != s.int(dy)?.to_vec() {
        return Err(error(ctx, call, "non-conformable arrays"));
    }
    if a != 0 && b != 0 && !a.max(b).is_multiple_of(a.min(b)) {
        ctx.warn_now(
            call.err_call(),
            "longer object length is not a multiple of shorter object length",
        )?;
    }
    Ok((a, b))
}

fn binary_op(ctx: &mut Ctx, call: &Call, args: &Args, op: Op) -> EvalResult {
    let mut x = args.get(0);
    if args.len() == 1 && matches!(op, Op::Add | Op::Sub) {
        return unary(ctx, call, x, op == Op::Sub);
    }
    let mut y = args.get(1);
    let compare = matches!(op, Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge);
    let tx = ctx.session().type_of(x)?;
    let ty = ctx.session().type_of(y)?;
    let compare_language = compare && (language(tx) || language(ty));
    if compare_language {
        (x, y) = language_operands(ctx, call, (x, tx), (y, ty), op)?;
    }
    let (nx, ny) = length(ctx, call, x, y)?;
    let len = if nx == 0 || ny == 0 { 0 } else { nx.max(ny) };
    vector_limit(ctx, call, len, size_of::<RComplex>())?;
    let tx = ctx.session().type_of(x)?;
    let ty = ctx.session().type_of(y)?;
    let kind = if compare_language {
        SexpType::Str
    } else if rank(tx) >= rank(ty) {
        tx
    } else {
        ty
    };
    let logic = matches!(op, Op::And | Op::Or | Op::Xor);
    let both_raw = tx == SexpType::Raw && ty == SexpType::Raw;
    let mut overflow = false;
    let mut accuracy = false;
    let s = ctx.session_mut();
    let result = if logic && both_raw && op != Op::Xor {
        recycled!(s, x, y, len, raw, alloc_raw_from, |a, b| if op == Op::And {
            a & b
        } else {
            a | b
        })
    } else if logic
        && rank(kind) <= rank(SexpType::Cplx)
        && tx != SexpType::Raw
        && ty != SexpType::Raw
    {
        let (a, _) = s.coerce(x, SexpType::Lgl)?;
        let (b, _) = s.coerce(y, SexpType::Lgl)?;
        recycled!(
            s,
            a,
            b,
            len,
            lgl,
            alloc_lgl_from,
            |a: RLogical, b| match op {
                Op::And => a.and(b),
                Op::Or => a.or(b),
                _ => a.and(b.not()).or(a.not().and(b)),
            }
        )
    } else if compare {
        compare_values(ctx, call, x, y, (nx, ny, len), kind, op)?
    } else if logic
        || rank(kind) > rank(SexpType::Cplx)
        || tx == SexpType::Raw
        || ty == SexpType::Raw
    {
        return Err(error(
            ctx,
            call,
            if logic {
                "operations are possible only for numeric, logical or complex types"
            } else {
                "non-numeric argument to binary operator"
            },
        ));
    } else if kind == SexpType::Cplx {
        if matches!(op, Op::Idiv | Op::Rem) {
            return Err(error(ctx, call, "unimplemented complex operation"));
        }
        let (a, _) = s.coerce(x, kind)?;
        let (b, _) = s.coerce(y, kind)?;
        recycled!(s, a, b, len, cplx, alloc_cplx_from, |a, b| complex_op(
            a, b, op
        ))
    } else if rank(kind) <= rank(SexpType::Int) && !matches!(op, Op::Div | Op::Pow) {
        let (a, _) = s.coerce(x, SexpType::Int)?;
        let (b, _) = s.coerce(y, SexpType::Int)?;
        recycled!(s, a, b, len, int, alloc_int_from, |a, b| {
            let (value, flag) = match op {
                Op::Add => int_add(a, b),
                Op::Sub => int_sub(a, b),
                Op::Mul => int_mul(a, b),
                Op::Idiv | Op::Rem if a != NA_INTEGER && b != NA_INTEGER && b != 0 => {
                    let q = a / b;
                    let r = a % b;
                    let adjust = r != 0 && (r < 0) != (b < 0);
                    (
                        if op == Op::Idiv {
                            q - i32::from(adjust)
                        } else if adjust {
                            r + b
                        } else {
                            r
                        },
                        false,
                    )
                }
                _ => (NA_INTEGER, false),
            };
            overflow |= flag;
            value
        })
    } else {
        let (a, _) = s.coerce(x, SexpType::Real)?;
        let (b, _) = s.coerce(y, SexpType::Real)?;
        recycled!(s, a, b, len, real, alloc_real_from, |a, b| real_op(
            a,
            b,
            op,
            &mut accuracy
        ))
    };
    ctx.protect(result);
    if overflow {
        ctx.warn_now(call.err_call(), "NAs produced by integer overflow")?;
    }
    if accuracy {
        ctx.warn_now(
            call.err_call(),
            "probable complete loss of accuracy in modulus",
        )?;
    }
    binary_attributes(ctx, result, x, y, (nx, ny, len), !logic && !compare)?;
    Ok(result)
}

fn language(ty: SexpType) -> bool {
    matches!(ty, SexpType::Sym | SexpType::Lang)
}

fn language_operands(
    ctx: &mut Ctx,
    call: &Call,
    (x, tx): (SexpId, SexpType),
    (y, ty): (SexpId, SexpType),
    op: Op,
) -> EvalResult<(SexpId, SexpId)> {
    let operator = match op {
        Op::Eq => "==",
        Op::Ne => "!=",
        Op::Lt => "<",
        Op::Le => "<=",
        Op::Gt => ">",
        _ => ">=",
    };
    if !matches!(op, Op::Eq | Op::Ne) {
        return Err(error(
            ctx,
            call,
            format!("comparison ({operator}) is not possible for language types"),
        ));
    }
    let mut convert = |value, ty| {
        let s = ctx.session_mut();
        let out = if language(ty) {
            let opts = roxide_value::DeparseOpts {
                width_cutoff: usize::MAX,
                ..Default::default()
            };
            let text = roxide_value::deparse(s, value, opts)?.join(" ");
            s.scalar_str(&text)?
        } else if ty == SexpType::Expr {
            let values = s.list_slice(value)?.to_vec();
            let out = super::common::list(s, &values)?;
            copy_attrs(s, value, out, |name| {
                matches!(name, "names" | "dim" | "dimnames")
            })?;
            out
        } else if super::common::atomic(ty) || matches!(ty, SexpType::Nil | SexpType::Vec) {
            value
        } else {
            return Err(error(
                ctx,
                call,
                format!("comparison ({operator}) is possible only for atomic and list types"),
            ));
        };
        ctx.protect(out);
        Ok(out)
    };
    Ok((convert(x, tx)?, convert(y, ty)?))
}

fn binary_attributes(
    ctx: &mut Ctx,
    out: SexpId,
    x: SexpId,
    y: SexpId,
    (nx, ny, len): (usize, usize, usize),
    arithmetic: bool,
) -> EvalResult<()> {
    let s = ctx.session_mut();
    let (first, second) = if nx >= ny { (x, y) } else { (y, x) };
    if arithmetic {
        if nx == ny || len == 0 {
            copy_attrs(s, second, out, |n| {
                !matches!(n, "names" | "dim" | "dimnames")
            })?;
        }
        for (sym, val) in s.attrs(first)? {
            let name = s.sym_name(sym)?;
            if !matches!(name, "names" | "dim" | "dimnames") {
                let class_s4 = (name == "class").then(|| s.is_s4(first)).transpose()?;
                s.set_attrib(out, sym, val)?;
                if let Some(s4) = class_s4 {
                    s.set_s4_flag(out, s4)?;
                }
            }
        }
    }
    let array = [first, second]
        .into_iter()
        .find(|&v| s.xlength(v).ok() == Some(len) && attr(s, v, "dim") != SEXP_NIL);
    if let Some(array) = array {
        set(s, out, "dim", attr(s, array, "dim"))?;
        let other = if array == x { y } else { x };
        let names = attr(s, array, "dimnames");
        let names = if names == SEXP_NIL && attr(s, array, "dim") == attr(s, other, "dim") {
            attr(s, other, "dimnames")
        } else {
            names
        };
        set(s, out, "dimnames", names)?;
    } else {
        for src in [first, second] {
            let n = attr(s, src, "names");
            if s.xlength(src)? == len && n != SEXP_NIL {
                set(s, out, "names", n)?;
                break;
            }
        }
    }
    Ok(())
}

macro_rules! recycled {
    ($s:ident, $x:ident, $y:ident, $len:ident, $read:ident, $alloc:ident, $f:expr) => {{
        let a = $s.$read($x)?;
        let b = $s.$read($y)?;
        let values = recycle_values(a, b, $len, $f);
        $s.$alloc(&values)?
    }};
}
use recycled;

fn recycle_values<A: roxide_value::Elem, B: roxide_value::Elem, C>(
    a: roxide_value::VecView<'_, A>,
    b: roxide_value::VecView<'_, B>,
    len: usize,
    mut apply: impl FnMut(A, B) -> C,
) -> Vec<C> {
    (0..len)
        .map(|i| apply(a.get(i % a.len()), b.get(i % b.len())))
        .collect()
}

fn real_op(a: f64, b: f64, op: Op, accuracy: &mut bool) -> f64 {
    if op == Op::Pow && (b == 0.0 || a == 1.0) {
        return 1.0;
    }
    if a.is_nan() || b.is_nan() {
        return nan_propagate(a, b);
    }
    match op {
        Op::Add => a + b,
        Op::Sub => a - b,
        Op::Mul => a * b,
        Op::Div => a / b,
        Op::Pow => {
            if a == 0.0 && b < 0.0 {
                return f64::INFINITY;
            }
            if a.is_finite() && a < 0.0 && b.is_infinite() {
                return R_NAN;
            }
            a.powf(b)
        }
        Op::Idiv | Op::Rem => {
            if b == 0.0 {
                return if op == Op::Idiv { a / b } else { R_NAN };
            }
            if b.is_infinite() && a.is_finite() {
                let opposite = a != 0.0 && a.is_sign_negative() != b.is_sign_negative();
                return if op == Op::Idiv {
                    if opposite { -1.0 } else { 0.0 }
                } else if opposite {
                    b
                } else {
                    a
                };
            }
            let q = a / b;
            let residual = a - q.floor() * b;
            if op == Op::Idiv {
                if q.is_infinite() {
                    q
                } else {
                    q.floor() + (residual / b).floor()
                }
            } else {
                *accuracy |= q.is_finite() && q.abs() > 1.0 / f64::EPSILON;
                residual - (residual / b).floor() * b
            }
        }
        _ => R_NAN,
    }
}

fn complex_op(a: RComplex, b: RComplex, op: Op) -> RComplex {
    let RComplex { re: x, im: y } = a;
    let RComplex { re: u, im: v } = b;
    let (re, im) = match op {
        Op::Add => (real_op(x, u, op, &mut false), real_op(y, v, op, &mut false)),
        Op::Sub => (real_op(x, u, op, &mut false), real_op(y, v, op, &mut false)),
        Op::Mul => (x * u - y * v, x * v + y * u),
        Op::Div if u == 0.0 && v == 0.0 => (x / 0.0, y / 0.0),
        Op::Div if u.abs() >= v.abs() => {
            let r = v / u;
            let den = 1.0 + r * r;
            ((x / u + (y / u) * r) / den, (y / u - (x / u) * r) / den)
        }
        Op::Div => {
            let r = u / v;
            let den = 1.0 + r * r;
            (((x / v) * r + y / v) / den, ((y / v) * r - x / v) / den)
        }
        Op::Pow if u == 0.0 && v == 0.0 => (1.0, 0.0),
        Op::Pow if u == 1.0 && v == 0.0 => (x, y),
        Op::Pow if a.is_na() => return RComplex::NA,
        Op::Pow if b.is_na() => (R_NAN, R_NAN),
        Op::Pow if x == 0.0 && y == 0.0 => (0.0_f64.powf(u), if v == 0.0 { 0.0 } else { R_NAN }),
        Op::Pow if v == 0.0 && u.fract() == 0.0 && u.abs() <= i32::MAX as f64 => {
            let mut power = u.abs() as u32;
            let mut base = a;
            let mut out = RComplex { re: 1.0, im: 0.0 };
            while power != 0 {
                if power & 1 == 1 {
                    out = complex_op(out, base, Op::Mul);
                }
                power >>= 1;
                if power != 0 {
                    base = complex_op(base, base, Op::Mul);
                }
            }
            if u < 0.0 {
                return complex_op(RComplex { re: 1.0, im: 0.0 }, out, Op::Div);
            }
            return out;
        }
        Op::Pow => {
            let log = x.hypot(y).ln();
            let theta = y.atan2(x);
            let mag = (u * log - v * theta).exp();
            let angle = v * log + u * theta;
            (mag * angle.cos(), mag * angle.sin())
        }
        _ => (R_NAN, R_NAN),
    };
    if (a.is_na() || b.is_na()) && matches!(op, Op::Mul | Op::Div) {
        RComplex::NA
    } else {
        RComplex { re, im }
    }
}

fn compare_values(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    y: SexpId,
    (nx, ny, len): (usize, usize, usize),
    kind: SexpType,
    op: Op,
) -> EvalResult {
    let combine = |v: Option<Ordering>| {
        RLogical::from_option(v.map(|o| match op {
            Op::Eq => o.is_eq(),
            Op::Ne => o.is_ne(),
            Op::Lt => o.is_lt(),
            Op::Le => o.is_le(),
            Op::Gt => o.is_gt(),
            _ => o.is_ge(),
        }))
    };
    let s = ctx.session_mut();
    if len == 0 {
        return Ok(s.alloc_lgl_from(&[])?);
    }
    Ok(match kind {
        SexpType::Nil => s.alloc_lgl_from(&[])?,
        SexpType::Str => {
            let (a, _) = s.coerce(x, kind)?;
            let (b, _) = s.coerce(y, kind)?;
            let a = s.str_slice(a)?;
            let b = s.str_slice(b)?;
            let out: Vec<_> = (0..len)
                .map(|i| {
                    let (a, b) = (a[i % nx], b[i % ny]);
                    combine(if a == CharId::NA || b == CharId::NA {
                        None
                    } else {
                        Some(s.char_bytes(a).ok().cmp(&s.char_bytes(b).ok()))
                    })
                })
                .collect();
            s.alloc_lgl_from(&out)?
        }
        SexpType::Cplx => {
            if !matches!(op, Op::Eq | Op::Ne) {
                return Err(error(ctx, call, "invalid comparison with complex values"));
            }
            let (a, _) = s.coerce(x, kind)?;
            let (b, _) = s.coerce(y, kind)?;
            recycled!(
                s,
                a,
                b,
                len,
                cplx,
                alloc_lgl_from,
                |a: RComplex, b: RComplex| {
                    combine(
                        a.re.partial_cmp(&b.re)
                            .zip(a.im.partial_cmp(&b.im))
                            .map(|(re, im)| re.then(im)),
                    )
                }
            )
        }
        SexpType::Raw => {
            let (a, _) = s.coerce(x, kind)?;
            let (b, _) = s.coerce(y, kind)?;
            recycled!(s, a, b, len, raw, alloc_lgl_from, |a: u8, b| combine(Some(
                a.cmp(&b)
            )))
        }
        SexpType::Real | SexpType::Int | SexpType::Lgl => {
            let (a, _) = s.coerce(x, SexpType::Real)?;
            let (b, _) = s.coerce(y, SexpType::Real)?;
            recycled!(s, a, b, len, real, alloc_lgl_from, |a: f64, b| combine(
                a.partial_cmp(&b)
            ))
        }
        _ => {
            return Err(error(
                ctx,
                call,
                "comparison is possible only for atomic and list types",
            ));
        }
    })
}

fn unary(ctx: &mut Ctx, call: &Call, x: SexpId, neg: bool) -> EvalResult {
    let s = ctx.session_mut();
    let out = match s.type_of(x)? {
        SexpType::Int | SexpType::Lgl => {
            let (a, _) = s.coerce(x, SexpType::Int)?;
            let vals: Vec<_> = s
                .int(a)?
                .iter()
                .map(|v| if neg && v != NA_INTEGER { -v } else { v })
                .collect();
            s.alloc_int_from(&vals)?
        }
        SexpType::Real => {
            let vals: Vec<_> = s
                .real(x)?
                .iter()
                .map(|v| if neg { -v } else { v })
                .collect();
            s.alloc_real_from(&vals)?
        }
        SexpType::Cplx => {
            let vals: Vec<_> = s
                .cplx(x)?
                .iter()
                .map(|v| {
                    if neg {
                        RComplex {
                            re: -v.re,
                            im: -v.im,
                        }
                    } else {
                        v
                    }
                })
                .collect();
            s.alloc_cplx_from(&vals)?
        }
        _ => return Err(error(ctx, call, "invalid argument to unary operator")),
    };
    copy_attrs(s, x, out, |_| true)?;
    Ok(out)
}

pub(super) fn not(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let s = ctx.session_mut();
    let out = match s.type_of(x)? {
        SexpType::Raw => {
            let vals: Vec<_> = s.raw(x)?.iter().map(|v| !v).collect();
            s.alloc_raw_from(&vals)?
        }
        ty if ty != SexpType::Nil && (rank(ty) <= rank(SexpType::Cplx) || s.xlength(x)? == 0) => {
            let (a, _) = if s.xlength(x)? == 0 {
                s.coerce(SEXP_NIL, SexpType::Lgl)?
            } else {
                s.coerce(x, SexpType::Lgl)?
            };
            let vals: Vec<_> = s.lgl(a)?.iter().map(RLogical::not).collect();
            s.alloc_lgl_from(&vals)?
        }
        _ => return Err(error(ctx, call, "invalid argument type")),
    };
    copy_attrs(s, x, out, |n| matches!(n, "names" | "dim" | "dimnames"))?;
    Ok(out)
}

fn short(ctx: &mut Ctx, call: &Call, args: &Args, and: bool) -> EvalResult {
    let mut result = RLogical::from_option(Some(and));
    for (i, &expr) in args.values().iter().take(2).enumerate() {
        let x = ctx.eval(expr, call.env)?;
        let s = ctx.session_mut();
        if !matches!(
            s.type_of(x)?,
            SexpType::Lgl | SexpType::Int | SexpType::Real | SexpType::Cplx
        ) {
            let name = if and { "&&" } else { "||" };
            return Err(error(
                ctx,
                call,
                format!(
                    "invalid '{}' type in 'x {name} y'",
                    if i == 0 { "x" } else { "y" }
                ),
            ));
        }
        let len = s.xlength(x)?;
        if len != 1 {
            return Err(error(
                ctx,
                call,
                format!("'length = {len}' in coercion to 'logical(1)'"),
            ));
        }
        let (x, _) = s.coerce(x, SexpType::Lgl)?;
        let val = s.lgl(x)?.iter().next().unwrap_or(RLogical::NA);
        result = if and { result.and(val) } else { result.or(val) };
        if result.get() == Some(!and) {
            break;
        }
    }
    Ok(ctx.session_mut().scalar_lgl(result)?)
}

pub(super) fn short_and(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    short(ctx, call, args, true)
}
pub(super) fn short_or(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    short(ctx, call, args, false)
}
pub(super) fn coalesce(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let left = ctx.eval(args.get(0), call.env)?;
    if left == SEXP_NIL {
        ctx.eval(args.get(1), call.env)
    } else {
        Ok(left)
    }
}

macro_rules! complex_projection {
    ($($f:ident: $project:expr),* $(,)?) => {$(
        pub(super) fn $f(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            let x = args.get(0);
            let s = ctx.session_mut();
            if !matches!(s.type_of(x)?, SexpType::Lgl | SexpType::Int | SexpType::Real | SexpType::Cplx) {
                return Err(error(ctx, call, "non-numeric argument to function"));
            }
            let (a, _) = s.coerce(x, SexpType::Cplx)?;
            let values: Vec<_> = s.cplx(a)?.iter().map($project).collect();
            let out = s.alloc_real_from(&values)?;
            copy_attrs(s, x, out, |_| true)?;
            Ok(out)
        }
    )*};
}
complex_projection!(re:|v| v.re, im:|v| v.im, modulus:|v| if v.is_na() { roxide_value::NA_REAL } else { v.re.hypot(v.im) }, arg:|v| if v.is_na() { roxide_value::NA_REAL } else { v.im.atan2(v.re) });

pub(super) fn conj(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let s = ctx.session_mut();
    let out = match s.type_of(x)? {
        SexpType::Cplx => {
            let values: Vec<_> = s
                .cplx(x)?
                .iter()
                .map(|v| RComplex {
                    re: v.re,
                    im: -v.im,
                })
                .collect();
            s.alloc_cplx_from(&values)?
        }
        SexpType::Real => return Ok(x),
        SexpType::Lgl | SexpType::Int => s.coerce(x, SexpType::Real)?.0,
        _ => return Err(error(ctx, call, "non-numeric argument to function")),
    };
    copy_attrs(s, x, out, |_| true)?;
    Ok(out)
}

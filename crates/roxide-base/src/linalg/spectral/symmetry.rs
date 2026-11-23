use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{IdenticalOpts, RLogical, SEXP_NIL, SexpType, ValueApi, identical};

use crate::ops::common::{attr, error, flag, number, string};

use super::super::{axes, dimensions};

pub(super) const TOLERANCE: f64 = 100.0 * f64::EPSILON;

pub(in crate::linalg) fn is_symmetric(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, tol, tol1, trans, attrs] = *args.values() else {
        return Err(error(ctx, call, "invalid symmetry arguments"));
    };
    let Some((n, m)) = dimensions(ctx.session(), x) else {
        return logical(ctx, false);
    };
    if n != m {
        return logical(ctx, false);
    }
    if flag(ctx, attrs, true)? {
        let s = ctx.session();
        let [row, col] = axes(s, x);
        let names = crate::ops::common::names(s, attr(s, x, "dimnames"));
        if !identical(s, row, col, IdenticalOpts::default())? || names.first() != names.get(1) {
            return logical(ctx, false);
        }
    }
    let ty = ctx.session().type_of(x)?;
    let matches = match ty {
        SexpType::Int | SexpType::Real | SexpType::Cplx => {
            let tol = tolerance(ctx, call, tol)?;
            let tol1 = if n > 1 && tol1 != SEXP_NIL {
                tolerance(ctx, call, tol1)?
            } else {
                f64::INFINITY
            };
            if ty == SexpType::Cplx {
                let trans = string(ctx.session(), trans)
                    .ok_or_else(|| error(ctx, call, "missing value where TRUE/FALSE needed"))?;
                let conjugate = trans == "C";
                let values = super::super::complex(ctx, call, x)?;
                test(n, tol, tol1, |i, j| {
                    let a = values[j * n + i];
                    let b = values[i * n + j];
                    let bi = if conjugate { -b.im } else { b.im };
                    let (a_na, b_na) =
                        (a.re.is_nan() || a.im.is_nan(), b.re.is_nan() || bi.is_nan());
                    if (a.re == b.re && a.im == bi) || (a_na && b_na) {
                        Pair::Equal
                    } else if a_na || b_na {
                        Pair::Different
                    } else {
                        difference(a.re.hypot(a.im), (a.re - b.re).hypot(a.im - bi))
                    }
                })
            } else {
                ctx.protect(x);
                let values = super::super::real(ctx, call, x)?;
                real(&values, n, tol, tol1)
            }
        }
        SexpType::Lgl => {
            let a = ctx.session().lgl(x)?;
            exact(n, |i, j| a.get(j * n + i) == a.get(i * n + j))
        }
        SexpType::Str => {
            let a = ctx.session().str_slice(x)?;
            exact(n, |i, j| a[j * n + i] == a[i * n + j])
        }
        SexpType::Raw => {
            let a = ctx.session().raw(x)?;
            exact(n, |i, j| a.get(j * n + i) == a.get(i * n + j))
        }
        SexpType::Vec | SexpType::Expr => {
            let s = ctx.session();
            let mut equal = true;
            for j in 0..n {
                for i in j + 1..n {
                    let a = s.list_get(x, j * n + i)?;
                    let b = s.list_get(x, i * n + j)?;
                    equal &= identical(s, a, b, IdenticalOpts::default())?;
                }
            }
            equal
        }
        _ => false,
    };
    logical(ctx, matches)
}

fn tolerance(ctx: &mut Ctx, call: &Call, x: roxide_gc::SexpId) -> EvalResult<f64> {
    let s = ctx.session();
    if !matches!(s.type_of(x)?, SexpType::Int | SexpType::Real) {
        return Err(error(ctx, call, "'tolerance' should be numeric"));
    }
    if s.xlength(x)? != 1 {
        return Err(error(ctx, call, "length(tolerance) == 1L is not TRUE"));
    }
    Ok(number(ctx.session_mut(), x)?)
}

fn logical(ctx: &mut Ctx, value: bool) -> EvalResult {
    Ok(ctx.session_mut().scalar_lgl(if value {
        RLogical::TRUE
    } else {
        RLogical::FALSE
    })?)
}

fn exact(n: usize, eq: impl Fn(usize, usize) -> bool) -> bool {
    (0..n).all(|j| (j + 1..n).all(|i| eq(i, j)))
}

pub(super) fn real(values: &[f64], n: usize, tol: f64, tol1: f64) -> bool {
    test(n, tol, tol1, |i, j| {
        let a = values[j * n + i];
        let b = values[i * n + j];
        if a == b || (a.is_nan() && b.is_nan()) {
            Pair::Equal
        } else {
            difference(a.abs(), (a - b).abs())
        }
    })
}

enum Pair {
    Equal,
    Different,
    Mismatch { scale: f64, delta: f64 },
}

fn difference(scale: f64, delta: f64) -> Pair {
    if scale.is_finite() && delta.is_finite() {
        Pair::Mismatch { scale, delta }
    } else {
        Pair::Different
    }
}

fn test(n: usize, tol: f64, tol1: f64, compare: impl Fn(usize, usize) -> Pair) -> bool {
    if n > 1 {
        for row in [0, 1, n - 2, n - 1] {
            if !within((0..n).map(|col| compare(row, col)), tol1) {
                return false;
            }
        }
    }
    within(
        (0..n).flat_map(|j| {
            (0..n).map({
                let compare = &compare;
                move |i| compare(i, j)
            })
        }),
        tol,
    )
}

fn within(values: impl Iterator<Item = Pair>, tol: f64) -> bool {
    let (mut count, mut scale, mut delta) = (0.0, 0.0, 0.0);
    for pair in values {
        match pair {
            Pair::Equal => {}
            Pair::Different => return false,
            Pair::Mismatch { scale: a, delta: b } => {
                count += 1.0;
                scale += (a - scale) / count;
                delta += (b - delta) / count;
            }
        }
    }
    count == 0.0
        || if scale > tol && scale != 0.0 {
            delta / scale <= tol
        } else {
            delta <= tol
        }
}

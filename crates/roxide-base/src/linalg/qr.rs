use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_math::{
    MatRef, MathError,
    linalg::{
        factor::{QrLapack, dgeqp3},
        linpack::{QrLinpack, dqrdc2},
    },
};
use roxide_value::{CharId, NA_REAL, SEXP_NIL, SexpType, ValueApi};

use crate::ops::{
    attributes::has_class,
    common::{attr, error, flag, number, set, string, vector_limit},
};

use super::{Dense, dimensions, labels, math_error, named_list, real, set_axis_keys, shape, size};

enum Factors {
    Linpack(QrLinpack),
    Lapack(QrLapack),
}

macro_rules! each {
    ($x:expr, $f:ident => $body:expr) => {
        match $x {
            Factors::Linpack($f) => $body,
            Factors::Lapack($f) => $body,
        }
    };
}

impl Factors {
    fn compute(ctx: &mut Ctx, call: &Call, a: Dense, tol: f64, lapack: bool) -> EvalResult<Self> {
        vector_limit(ctx, call, a.cols, size_of::<usize>())?;
        if !lapack && a.values.iter().any(|x| !x.is_finite()) {
            return Err(error(
                ctx,
                call,
                "NA/NaN/Inf in foreign function call (arg 1)",
            ));
        }
        if !lapack && !tol.is_finite() {
            return Err(error(
                ctx,
                call,
                "NA/NaN/Inf in foreign function call (arg 5)",
            ));
        }
        if lapack && a.rows == 0 {
            return Err(error(
                ctx,
                call,
                "BLAS/LAPACK routine 'DGEQP3' gave error code -4",
            ));
        }
        let result = if lapack {
            dgeqp3(a.values, a.rows, a.cols).map(Self::Lapack)
        } else {
            dqrdc2(a.values, a.rows, a.cols, tol).map(Self::Linpack)
        };
        result.map_err(|e| math_error(ctx, call, e))
    }

    fn read(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<(Self, [SexpId; 2], [CharId; 2])> {
        if !has_class(ctx.session(), x, "qr") {
            return Err(error(ctx, call, "argument is not a QR decomposition"));
        }
        let s = ctx.session();
        let field = |name| {
            crate::ops::common::names(s, x)
                .iter()
                .position(|&c| s.char_bytes(c).ok() == Some(name))
                .and_then(|i| s.list_get(x, i).ok())
                .unwrap_or(SEXP_NIL)
        };
        let (packed, rank, qraux, pivot) = (
            field(b"qr"),
            field(b"rank"),
            field(b"qraux"),
            field(b"pivot"),
        );
        let lapack = attr(s, x, "useLAPACK");
        if dimensions(s, packed).is_none() {
            return Err(error(ctx, call, "invalid QR decomposition"));
        }
        let a = Dense::read(ctx, call, packed)?;
        vector_limit(ctx, call, ctx.session().xlength(pivot)?, size_of::<usize>())?;
        let rank = number(ctx.session_mut(), rank)? as usize;
        let qraux = real(ctx, call, qraux)?;
        let (pivot, _) = ctx.session_mut().coerce(pivot, SexpType::Int)?;
        let pivot: Vec<_> = ctx
            .session()
            .int(pivot)?
            .iter()
            .map(|p| (p as usize).wrapping_sub(1))
            .collect();
        if pivot.len() != a.cols || pivot.iter().any(|&p| p >= a.cols) || rank > a.rows.min(a.cols)
        {
            return Err(error(ctx, call, "invalid QR decomposition"));
        }
        let (qr, nrow, ncol) = (a.values, a.rows, a.cols);
        let f = if flag(ctx, lapack, false)? {
            Self::Lapack(QrLapack {
                qr,
                nrow,
                ncol,
                rank,
                qraux,
                pivot,
            })
        } else {
            Self::Linpack(QrLinpack {
                qr,
                nrow,
                ncol,
                rank,
                qraux,
                pivot,
                tol: 1e-7,
            })
        };
        Ok((f, a.labels, a.keys))
    }

    fn dims(&self) -> (usize, usize, usize) {
        each!(self, f => (f.nrow,f.ncol,f.rank))
    }

    fn pivot(&self) -> &[usize] {
        each!(self, f => &f.pivot)
    }

    fn is_lapack(&self) -> bool {
        matches!(self, Self::Lapack(_))
    }
}

pub(super) fn qr(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, tol, lapack] = args.values() else {
        return Err(error(ctx, call, "invalid QR arguments"));
    };
    if ctx.session().type_of(*x)? == SexpType::Cplx {
        return Err(error(ctx, call, "complex QR is not supported"));
    }
    let a = Dense::read(ctx, call, *x)?;
    let [rows, cols] = a.labels;
    let keys = a.keys;
    let tol = number(ctx.session_mut(), *tol)?;
    let lapack = flag(ctx, *lapack, false)?;
    let f = Factors::compute(ctx, call, a, tol, lapack)?;
    let (nr, nc, rank) = f.dims();
    let s = ctx.session_mut();
    let cols = labels(s, cols, f.pivot().iter().copied())?;
    let data = each!(&f, f => s.alloc_real_from(&f.qr))?;
    let packed = shape(s, data, nr, nc, [rows, cols])?;
    set_axis_keys(s, packed, &keys)?;
    let rank = s.scalar_int(rank as i32)?;
    let qraux = each!(&f, f => s.alloc_real_from(&f.qraux))?;
    let pivot = s.alloc_int_from(&f.pivot().iter().map(|&p| p as i32 + 1).collect::<Vec<_>>())?;
    let out = named_list(
        s,
        &[
            ("qr", packed),
            ("rank", rank),
            ("qraux", qraux),
            ("pivot", pivot),
        ],
        "qr",
    )?;
    if lapack {
        let yes = s.scalar_lgl(roxide_value::RLogical::TRUE)?;
        set(s, out, "useLAPACK", yes)?;
        let class = s.scalar_str("qr")?;
        set(s, out, "class", SEXP_NIL)?;
        set(s, out, "class", class)?;
    }
    Ok(out)
}

pub(super) fn extract(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, complete, extra, op] = args.values() else {
        return Err(error(ctx, call, "invalid QR arguments"));
    };
    let (f, [row_names, col_names], keys) = Factors::read(ctx, call, *x)?;
    let (nr, nc, _) = f.dims();
    let complete = flag(ctx, *complete, false)?;
    let m = if complete { nr } else { nr.min(nc) };
    let op = string(ctx.session(), *op);
    let (data, rows, cols, names) = match op.as_deref() {
        Some("Q") => {
            size(ctx, call, nr, m, size_of::<f64>())?;
            let scales = if *extra == SEXP_NIL {
                None
            } else {
                let mut scales = real(ctx, call, *extra)?;
                scales.resize(m, NA_REAL);
                if !f.is_lapack() && scales.iter().any(|x| !x.is_finite()) && nr > 1 && nc > 0 {
                    return Err(error(
                        ctx,
                        call,
                        "NA/NaN/Inf in foreign function call (arg 5)",
                    ));
                }
                Some(scales)
            };
            let data = each!(&f, f => f.q_mat(complete,scales.as_deref()));
            (data, nr, m, [SEXP_NIL; 2])
        }
        Some("R") => {
            size(ctx, call, m, nc, size_of::<f64>())?;
            let names = [labels(ctx.session_mut(), row_names, 0..m)?, col_names];
            (each!(&f, f => f.r_mat(complete)), m, nc, names)
        }
        _ => {
            let width = if *extra == SEXP_NIL {
                m
            } else {
                let v = number(ctx.session_mut(), *extra)?;
                if !v.is_finite() || v <= -1.0 || v > i32::MAX as f64 {
                    return Err(error(ctx, call, "invalid 'ncol' argument"));
                }
                v as usize
            };
            if f.pivot().iter().take(width).any(|&p| p >= width) {
                return Err(error(
                    ctx,
                    call,
                    "need larger value of 'ncol' as pivoting occurred",
                ));
            }
            let n = size(ctx, call, nr, width, size_of::<f64>())?;
            let mut upper = vec![0.0; n];
            each!(&f, f => {
                let a = MatRef::new(&f.qr,nr,nc).map_err(|e| math_error(ctx,call,e))?;
                for (j,&p) in f.pivot.iter().enumerate().filter(|&(_,p)| *p < width) {
                    let len = nr.min(j+1);
                    upper[p*nr..p*nr+len].copy_from_slice(&a.col(j)[..len]);
                }
            });
            let view = MatRef::new(&upper, nr, width).map_err(|e| math_error(ctx, call, e))?;
            let names = if width <= nc {
                let mut order = vec![0; nc];
                for (j, &p) in f.pivot().iter().enumerate() {
                    order[p] = j;
                }
                [
                    row_names,
                    labels(ctx.session_mut(), col_names, order.into_iter().take(width))?,
                ]
            } else {
                [SEXP_NIL; 2]
            };
            (each!(&f, f => f.qy(view,false)), nr, width, names)
        }
    };
    let data = data.map_err(|e| math_error(ctx, call, e))?;
    let s = ctx.session_mut();
    let out = s.alloc_real_from(&data)?;
    shape(s, out, rows, cols, names)?;
    set_axis_keys(s, out, &keys)
}

fn rhs(ctx: &mut Ctx, call: &Call, f: &Factors, y: SexpId, transform: bool) -> EvalResult<Dense> {
    let y = Dense::read(ctx, call, y)?;
    let (rows, _, _) = f.dims();
    if y.rows != rows {
        let message = if transform && f.is_lapack() {
            format!("right-hand side should have {rows} not {} rows", y.rows)
        } else {
            "'qr' and 'y' must have the same number of rows".into()
        };
        return Err(error(ctx, call, message));
    }
    if !f.is_lapack() && y.values.iter().any(|v| !v.is_finite()) {
        return Err(error(
            ctx,
            call,
            "NA/NaN/Inf in foreign function call (arg 5)",
        ));
    }
    Ok(y)
}

fn coefficients(
    ctx: &mut Ctx,
    call: &Call,
    f: &Factors,
    y: &Dense,
    packed_names: SexpId,
    zero: bool,
) -> EvalResult {
    let (_, nc, rank) = f.dims();
    let n = size(ctx, call, nc, y.cols, size_of::<f64>())?;
    let mut data = each!(f, f => f.coef(y.view())).map_err(|e| match e {
        MathError::Singular { info } if f.is_lapack() => error(
            ctx,
            call,
            format!("exact singularity in 'qr.coef': U[{info},{info}] = 0"),
        ),
        MathError::Singular { .. } => error(ctx, call, "exact singularity in 'qr.coef'"),
        _ => math_error(ctx, call, e),
    })?;
    if f.is_lapack() {
        for j in 0..y.cols {
            for i in rank..nc {
                data[j * nc + f.pivot()[i]] = f64::NAN;
            }
        }
    }
    for x in &mut data {
        if x.is_nan() {
            *x = if zero { 0.0 } else { NA_REAL };
        }
    }
    let s = ctx.session_mut();
    let mut order = vec![0; nc];
    for (j, &p) in f.pivot().iter().enumerate() {
        order[p] = j;
    }
    let names = labels(s, packed_names, order)?;
    let out = s.alloc_real_from(&data[..n])?;
    if y.matrix {
        shape(s, out, nc, y.cols, [names, y.labels[1]])
    } else {
        set(s, out, "names", names)?;
        Ok(out)
    }
}

pub(super) fn apply(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, y, op] = args.values() else {
        return Err(error(ctx, call, "invalid QR arguments"));
    };
    let (mut f, [_, cols], _) = Factors::read(ctx, call, *x)?;
    let op_is_str = ctx.session().type_of(*op)? == SexpType::Str;
    let op_str = string(ctx.session(), *op);
    let transform = matches!(op_str.as_deref(), Some("qty" | "qy"));
    if f.is_lapack() && !transform && op_str.as_deref() != Some("coef") {
        return Err(error(ctx, call, "not supported for LAPACK QR"));
    }
    let y = rhs(ctx, call, &f, *y, transform)?;
    if op_str.as_deref() == Some("coef") {
        return coefficients(ctx, call, &f, &y, cols, false);
    }
    if op_str.as_deref() == Some("resid") && f.dims().2 == 0 {
        return Ok(args.values()[1]);
    }
    let data = if transform {
        each!(&f, f => f.qy(y.view(), op_str.as_deref() == Some("qty")))
    } else if let Factors::Linpack(f) = &mut f {
        if op_is_str {
            f.resid(y.view())
        } else {
            let k = number(ctx.session_mut(), *op)?;
            if !k.is_finite() || k < 0.0 || k >= f.nrow.min(f.ncol) as f64 + 1.0 {
                return Err(error(ctx, call, "invalid 'k' argument"));
            }
            if k < 1.0 {
                return Ok(args.values()[1]);
            }
            f.rank = k as usize;
            f.fitted(y.view())
        }
    } else {
        return Err(error(ctx, call, "not supported for LAPACK QR"));
    };
    let data = data.map_err(|e| math_error(ctx, call, e))?;
    y.result(ctx.session_mut(), &data, y.matrix || f.is_lapack())
}

pub(super) fn solve(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [a, b, tol, inverse] = args.values() else {
        return Err(error(ctx, call, "invalid QR arguments"));
    };
    let (f, packed_names) = if has_class(ctx.session(), *a, "qr") {
        let (f, [_, cols], _) = Factors::read(ctx, call, *a)?;
        (f, cols)
    } else {
        let a = Dense::read(ctx, call, *a)?;
        let names = a.labels[1];
        let tol = number(ctx.session_mut(), *tol)?;
        let f = Factors::compute(ctx, call, a, tol, false)?;
        let cols = labels(ctx.session_mut(), names, f.pivot().iter().copied())?;
        (f, cols)
    };
    let (nr, nc, rank) = f.dims();
    if rank < nr.min(nc) {
        return Err(error(ctx, call, "singular matrix 'a' in solve"));
    }
    ctx.protect(packed_names);
    let y = if flag(ctx, *inverse, false)? {
        if nr != nc {
            return Err(error(ctx, call, "only square matrices can be inverted"));
        }
        super::factor::identity(ctx, call, nr)?
    } else {
        if *b == SEXP_NIL {
            return Err(error(
                ctx,
                call,
                "'data' must be of a vector type, was 'NULL'",
            ));
        }
        rhs(ctx, call, &f, *b, false)?
    };
    coefficients(ctx, call, &f, &y, packed_names, true)
}

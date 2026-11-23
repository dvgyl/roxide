use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{
    Cplx, MatMut, MatRef, MathError,
    linalg::{
        factor::{
            Lu, dgetrf, dpotrf_upper, dpotri_upper, dpstrf_upper, dtrsm, matrix_norm_inf,
            matrix_norm_one, zgetrf,
        },
        linpack::dqrdc2,
    },
};
use roxide_value::{
    RComplex, SEXP_NIL, SexpType, ValueApi,
    format::{FormatOpts, format_real_scalar},
};

use crate::ops::common::{error, flag, number, set, string, vector_limit};

use super::{Dense, axes, complex, dimensions, labels, math_error, named_list, shape, size};

pub(super) fn identity(ctx: &mut Ctx, call: &Call, n: usize) -> EvalResult<Dense> {
    let len = size(ctx, call, n, n, size_of::<f64>())?;
    let mut values = vec![0.0; len];
    for i in 0..n {
        values[i * (n + 1)] = 1.0;
    }
    Ok(Dense {
        values,
        rows: n,
        cols: n,
        labels: [SEXP_NIL; 2],
        keys: [roxide_value::CharId::BLANK; 2],
        matrix: true,
    })
}

pub(super) fn chol(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, pivot, tol] = args.values() else {
        return Err(error(ctx, call, "invalid Cholesky arguments"));
    };
    if ctx.session().type_of(*x)? == SexpType::Cplx {
        return Err(error(
            ctx,
            call,
            "complex matrices not permitted at present",
        ));
    }
    let mut a = Dense::read(ctx, call, *x)?;
    if a.rows != a.cols {
        return Err(error(ctx, call, "'a' must be a square matrix"));
    }
    let n = a.rows;
    if n == 0 {
        return Err(error(ctx, call, "'a' must have dims > 0"));
    }
    let pivot = flag(ctx, *pivot, false)?;
    let tol = number(ctx.session_mut(), *tol)?;
    let mut matrix = MatMut::new(&mut a.values, n, n).map_err(|e| math_error(ctx, call, e))?;
    let pivots = if pivot {
        vector_limit(ctx, call, n, size_of::<usize>())?;
        let (pivots, rank) =
            dpstrf_upper(&mut matrix, tol).map_err(|e| math_error(ctx, call, e))?;
        if rank < n {
            ctx.warn_now(
                call.err_call(),
                "the matrix is either rank-deficient or not positive definite",
            )?;
        }
        a.labels[1] = labels(ctx.session_mut(), a.labels[1], pivots.iter().copied())?;
        Some((pivots, rank))
    } else {
        dpotrf_upper(&mut matrix).map_err(|e| match e {
            MathError::NotPositiveDefinite { minor } => error(
                ctx,
                call,
                format!("the leading minor of order {minor} is not positive"),
            ),
            _ => math_error(ctx, call, e),
        })?;
        None
    };
    let s = ctx.session_mut();
    let out = a.result(s, &a.values, true)?;
    if let Some((pivots, rank)) = pivots {
        let p = s.alloc_int_from(&pivots.into_iter().map(|i| i as i32 + 1).collect::<Vec<_>>())?;
        let rank = s.scalar_int(rank as i32)?;
        set(s, out, "pivot", p)?;
        set(s, out, "rank", rank)?;
    }
    Ok(out)
}

pub(super) fn chol2inv(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, k] = args.values() else {
        return Err(error(ctx, call, "invalid Cholesky arguments"));
    };
    let k = number(ctx.session_mut(), *k)?;
    if !k.is_finite() || k < 1.0 || k > i32::MAX as f64 {
        return Err(error(
            ctx,
            call,
            "'size' argument must be a positive integer",
        ));
    }
    let k = k as usize;
    let a = Dense::read(ctx, call, *x)?;
    if k > a.cols {
        return Err(error(
            ctx,
            call,
            format!("'size' cannot exceed ncol(x) = {}", a.cols),
        ));
    }
    if k > a.rows {
        return Err(error(
            ctx,
            call,
            format!("'size' cannot exceed nrow(x) = {}", a.rows),
        ));
    }
    let n = size(ctx, call, k, k, size_of::<f64>())?;
    let mut upper = vec![0.0; n];
    for j in 0..k {
        upper[j * k..j * k + j + 1].copy_from_slice(&a.values[j * a.rows..j * a.rows + j + 1]);
    }
    let r = MatRef::new(&upper, k, k).map_err(|e| math_error(ctx, call, e))?;
    let data = dpotri_upper(r).map_err(|e| match e {
        MathError::Singular { info } => error(
            ctx,
            call,
            format!("element ({info}, {info}) is zero, so the inverse cannot be computed"),
        ),
        _ => math_error(ctx, call, e),
    })?;
    let s = ctx.session_mut();
    let out = s.alloc_real_from(&data)?;
    shape(s, out, k, k, [SEXP_NIL; 2])
}

fn norm(a: MatRef<'_>, one: bool) -> f64 {
    if one {
        matrix_norm_one(a)
    } else {
        matrix_norm_inf(a)
    }
}

fn lu(ctx: &mut Ctx, call: &Call, a: Vec<f64>, n: usize) -> EvalResult<Lu> {
    vector_limit(ctx, call, n, size_of::<usize>())?;
    dgetrf(a, n).map_err(|e| math_error(ctx, call, e))
}

pub(super) fn solve(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [a, b, tol, inverse] = args.values() else {
        return Err(error(ctx, call, "invalid solve arguments"));
    };
    let inverse = flag(ctx, *inverse, false)?;
    let s = ctx.session();
    let (n, cols) = dimensions(s, *a).unwrap_or((s.xlength(*a)?, 1));
    if n == 0 {
        return Err(error(ctx, call, "'a' is 0-diml"));
    }
    if n != cols {
        return Err(error(
            ctx,
            call,
            format!("'a' ({n} x {cols}) must be square"),
        ));
    }
    let [a_rows, a_cols] = axes(s, *a);
    let b_dim = dimensions(s, *b);
    let (rows, cols) = if inverse {
        (n, n)
    } else {
        b_dim.unwrap_or((s.xlength(*b)?, 1))
    };
    if cols == 0 {
        return Err(error(ctx, call, "no right-hand side in 'b'"));
    }
    if rows != n {
        return Err(error(
            ctx,
            call,
            format!("'b' ({rows} x {cols}) must be compatible with 'a' ({n} x {n})"),
        ));
    }
    let labels = [a_cols, if inverse { a_rows } else { axes(s, *b)[1] }];
    let is_complex = s.type_of(*a)? == SexpType::Cplx || s.type_of(*b)? == SexpType::Cplx;
    let tol = number(ctx.session_mut(), *tol)?;
    let out = if is_complex {
        let a = complex(ctx, call, *a)?;
        let finite = a.iter().all(|v| v.re.is_finite() && v.im.is_finite());
        let anorm = if finite && tol > 0.0 {
            complex_norm(&a, n, true)
        } else {
            0.0
        };
        let mut b = if inverse {
            let len = size(ctx, call, n, n, size_of::<Cplx>())?;
            let mut values = vec![Cplx::default(); len];
            for i in 0..n {
                values[i * (n + 1)].re = 1.0;
            }
            values
        } else {
            complex(ctx, call, *b)?
        };
        if b.len() != rows.saturating_mul(cols) {
            return Err(error(ctx, call, "invalid matrix dimensions"));
        }
        vector_limit(ctx, call, n, size_of::<usize>())?;
        let lu = zgetrf(a, n).map_err(|e| math_error(ctx, call, e))?;
        lu.solve(&mut b)
            .map_err(|e| solve_error(ctx, call, e, "zgesv"))?;
        if finite && tol > 0.0 {
            check_condition(ctx, call, lu.rcond(anorm, true), tol)?;
        }
        let data: Vec<_> = b
            .into_iter()
            .map(|v| RComplex { re: v.re, im: v.im })
            .collect();
        ctx.session_mut().alloc_cplx_from(&data)?
    } else {
        let a = Dense::read(ctx, call, *a)?;
        let mut b = if inverse {
            identity(ctx, call, n)?.values
        } else {
            super::real(ctx, call, *b)?
        };
        let finite = a.values.iter().all(|x| x.is_finite());
        let anorm = matrix_norm_one(a.view());
        let lu = lu(ctx, call, a.values, n)?;
        let mut rhs = MatMut::new(&mut b, rows, cols).map_err(|e| math_error(ctx, call, e))?;
        lu.solve(&mut rhs)
            .map_err(|e| solve_error(ctx, call, e, "dgesv"))?;
        if finite && tol > 0.0 {
            check_condition(ctx, call, lu.rcond(anorm, true), tol)?;
        }
        ctx.session_mut().alloc_real_from(&b)?
    };
    let s = ctx.session_mut();
    if inverse || b_dim.is_some() {
        shape(s, out, rows, cols, labels)
    } else {
        set(s, out, "names", labels[0])?;
        Ok(out)
    }
}

fn solve_error(ctx: &mut Ctx, call: &Call, e: MathError, routine: &str) -> roxide_eval::Unwind {
    match e {
        MathError::Singular { .. } => error(ctx, call, format!("Lapack routine {routine}: {e}")),
        _ => math_error(ctx, call, e),
    }
}

fn check_condition(ctx: &mut Ctx, call: &Call, estimate: f64, tol: f64) -> EvalResult<()> {
    if estimate < tol {
        let message = format_real_scalar(
            estimate,
            FormatOpts {
                digits: 6,
                ..FormatOpts::default()
            },
        );
        return Err(error(
            ctx,
            call,
            format!("system is computationally singular: reciprocal condition number = {message}"),
        ));
    }
    Ok(())
}

fn complex_norm(a: &[Cplx], n: usize, one: bool) -> f64 {
    let mut max = 0.0_f64;
    for i in 0..n {
        let sum = (0..n)
            .map(|j| {
                let z = a[if one { i * n + j } else { j * n + i }];
                z.re.hypot(z.im)
            })
            .sum::<f64>();
        if sum.is_nan() {
            return sum;
        }
        max = max.max(sum);
    }
    max
}

pub(super) fn rcond(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, kind] = args.values() else {
        return Err(error(ctx, call, "invalid rcond arguments"));
    };
    if *kind != SEXP_NIL && ctx.session().xlength(*kind)? != 1 {
        return Err(error(ctx, call, "'arg' must be of length 1"));
    }
    let one = match string(ctx.session(), *kind).as_deref() {
        Some("O" | "1" | "") => true,
        None if *kind == SEXP_NIL => true,
        Some("I") => false,
        _ => return Err(error(ctx, call, "'arg' should be one of “O”, “I”, “1”")),
    };
    let Some((rows, cols)) = dimensions(ctx.session(), *x) else {
        return Err(error(ctx, call, "length(d <- dim(x)) == 2L is not TRUE"));
    };
    if rows.min(cols) == 0 {
        return Ok(ctx.session_mut().scalar_real(f64::INFINITY)?);
    }
    if ctx.session().type_of(*x)? == SexpType::Cplx {
        if rows != cols {
            return Err(error(
                ctx,
                call,
                "rectangular complex condition estimates are not supported",
            ));
        }
        let a = complex(ctx, call, *x)?;
        if a.len() != rows * cols {
            return Err(error(ctx, call, "invalid matrix dimensions"));
        }
        let norm = complex_norm(&a, rows, one);
        if !norm.is_finite() {
            return Err(error(
                ctx,
                call,
                "error code -5 from Lapack routine 'zgecon()'",
            ));
        }
        vector_limit(ctx, call, rows, size_of::<usize>())?;
        let lu = zgetrf(a, rows).map_err(|e| math_error(ctx, call, e))?;
        return Ok(ctx.session_mut().scalar_real(lu.rcond(norm, one))?);
    }
    let mut a = Dense::read(ctx, call, *x)?;
    let n = a.rows.min(a.cols);
    if a.rows != a.cols {
        if a.values.iter().any(|v| !v.is_finite()) {
            return Err(error(
                ctx,
                call,
                "NA/NaN/Inf in foreign function call (arg 1)",
            ));
        }
        if a.rows < a.cols {
            let mut data = vec![0.0; a.values.len()];
            for j in 0..a.cols {
                for i in 0..a.rows {
                    data[i * a.cols + j] = a.values[j * a.rows + i];
                }
            }
            a.values = data;
            (a.rows, a.cols) = (a.cols, a.rows);
        }
        vector_limit(ctx, call, n, size_of::<usize>())?;
        let qr = dqrdc2(a.values, a.rows, a.cols, 1e-7).map_err(|e| math_error(ctx, call, e))?;
        a.values = qr.r_mat(false).map_err(|e| math_error(ctx, call, e))?;
        a.rows = n;
    }
    let norm = norm(a.view(), one);
    if !norm.is_finite() {
        return Err(error(
            ctx,
            call,
            "error code -5 from Lapack routine 'dgecon()'",
        ));
    }
    let lu = lu(ctx, call, a.values, n)?;
    Ok(ctx.session_mut().scalar_real(lu.rcond(norm, one))?)
}

pub(super) fn determinant(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, log] = args.values() else {
        return Err(error(ctx, call, "invalid determinant arguments"));
    };
    if ctx.session().type_of(*x)? == SexpType::Cplx {
        return Err(error(
            ctx,
            call,
            "'determinant' not currently defined for complex matrices",
        ));
    }
    let a = Dense::read(ctx, call, *x)?;
    if !a.matrix || a.rows != a.cols {
        return Err(error(ctx, call, "'x' must be a square matrix"));
    }
    let log = flag(ctx, *log, true)?;
    let n = a.rows;
    let lu = lu(ctx, call, a.values, n)?;
    let (mut modulus, sign) = lu.log_modulus();
    if !log {
        modulus = if lu.zero_diag_index().is_some() {
            0.0
        } else {
            (0..n).map(|i| lu.lu[i * (n + 1)].abs()).product()
        };
    }
    let s = ctx.session_mut();
    let modulus = s.scalar_real(modulus)?;
    let logarithm = s.scalar_lgl(roxide_value::RLogical::from_option(Some(log)))?;
    set(s, modulus, "logarithm", logarithm)?;
    let sign = s.scalar_int(i32::from(sign))?;
    named_list(s, &[("modulus", modulus), ("sign", sign)], "det")
}

pub(super) fn backsolve(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [r, b, k, upper, trans] = args.values() else {
        return Err(error(ctx, call, "invalid triangular solve arguments"));
    };
    let r = Dense::read(ctx, call, *r)?;
    let mut b = Dense::read(ctx, call, *b)?;
    let k = number(ctx.session_mut(), *k)?;
    if !k.is_finite() || k < 1.0 || k.trunc() > r.rows.min(r.cols).min(b.rows) as f64 {
        return Err(error(ctx, call, "invalid 'k' argument"));
    }
    let k = k as usize;
    let upper = flag(ctx, *upper, true)?;
    let trans = flag(ctx, *trans, false)?;
    let mut rhs =
        MatMut::new(&mut b.values, b.rows, b.cols).map_err(|e| math_error(ctx, call, e))?;
    dtrsm(upper, trans, r.view(), &mut rhs, k).map_err(|e| match e {
        MathError::Singular { info } => error(
            ctx,
            call,
            format!("singular matrix in 'backsolve'. First zero in diagonal [{info}]"),
        ),
        _ => math_error(ctx, call, e),
    })?;
    let len = size(ctx, call, k, b.cols, size_of::<f64>())?;
    let mut data = Vec::with_capacity(len);
    for col in b.values.chunks_exact(b.rows) {
        data.extend_from_slice(&col[..k]);
    }
    let s = ctx.session_mut();
    let out = s.alloc_real_from(&data)?;
    if b.matrix {
        shape(s, out, k, b.cols, [SEXP_NIL; 2])
    } else {
        Ok(out)
    }
}

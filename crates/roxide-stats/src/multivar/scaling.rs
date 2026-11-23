use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{
    MatRef,
    linalg::spectral::{general_eigen, symmetric_eigen},
};
use roxide_value::{SEXP_NIL, ValueApi};

use crate::distn::support::{attr, logical, number, set_attr};

use super::distance::expand;
use super::support::{Matrix, area, dimnames, list, matrix, real, size};

fn center(values: &mut [f64], n: usize) {
    for row in 0..n {
        let mean = (0..n).map(|col| values[col * n + row]).sum::<f64>() / n as f64;
        for col in 0..n {
            values[col * n + row] -= mean;
        }
    }
    for col in values.chunks_exact_mut(n) {
        let mean = col.iter().sum::<f64>() / n as f64;
        for value in col {
            *value -= mean;
        }
    }
}

fn additive_constant(ctx: &mut Ctx, call: &Call, d: &[f64], n: usize) -> EvalResult<f64> {
    if n == 3 && (0..n).all(|j| (0..j).all(|i| d[j * n + i] == d[i * n + j])) {
        // With three points, the triangle inequalities determine the offset.
        let mut edges = [d[1], d[2], d[5]];
        edges.sort_by(f64::total_cmp);
        return Ok(((edges[2] - edges[1]) - edges[0]).max(0.0));
    }
    let side = area::<f64>(ctx, call, 2, n)?;
    let len = area::<f64>(ctx, call, side, side)?;
    let mut linear = d.to_vec();
    let mut quadratic: Vec<_> = d.iter().map(|v| v * v).collect();
    center(&mut linear, n);
    center(&mut quadratic, n);
    // The squared distances give the constant term of the matrix polynomial.
    // The linear term comes from the cross term in (d + c)^2.
    let mut companion = vec![0.0; len];
    for j in 0..n {
        companion[(j + n) * side + j] = 1.0;
        for i in 0..n {
            companion[j * side + i + n] = quadratic[j * n + i];
            companion[(j + n) * side + i + n] = 2.0 * linear[j * n + i];
        }
    }
    let view = MatRef {
        data: &companion,
        nrow: side,
        ncol: side,
    };
    let eig = general_eigen(view, true).map_err(|e| ctx.stop(call.err_call(), e.to_string()))?;
    Ok(eig.values.iter().map(|v| v.re).fold(0.0, f64::max))
}

pub(super) fn cmdscale(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let d = args.get(0);
    let explicit_size = attr(ctx, d, "Size")?;
    let (n, mut distances, names) = if explicit_size == SEXP_NIL {
        let a = Matrix::read(ctx, call, d)?;
        if a.rows != a.cols {
            return Err(ctx.stop(
                call.err_call(),
                "distances must be result of 'dist' or a square matrix",
            ));
        }
        (a.rows, a.values, dimnames(ctx, d)?)
    } else {
        let n = size(ctx, call, d)?;
        (
            n,
            expand(ctx, call, d, n)?,
            (attr(ctx, d, "Labels")?, SEXP_NIL),
        )
    };
    if distances.iter().any(|x| x.is_nan()) {
        return Err(ctx.stop(call.err_call(), "NA values not allowed in 'd'"));
    }

    let k = number(ctx, call, args.get(1))?;
    if !k.is_finite() || k < 1.0 || k.trunc() >= n as f64 {
        return Err(ctx.stop(call.err_call(), "'k' must be in {1, 2, ..  n - 1}"));
    }
    let k = k as usize;
    let mut flags = [false; 4];
    for (i, flag) in flags.iter_mut().enumerate() {
        *flag = logical(ctx, call, args.get(i + 2))?
            .ok_or_else(|| ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"))?;
    }
    let [eig, add, x_ret, list_ret] = flags;
    if distances.iter().any(|x| !x.is_finite()) {
        return Err(ctx.stop(call.err_call(), "infinite or missing values in 'x'"));
    }
    let ac = if add {
        let constant = additive_constant(ctx, call, &distances, n)?;
        for (i, d) in distances.iter_mut().enumerate() {
            if i / n != i % n {
                *d += constant;
            } else {
                *d = 0.0;
            }
        }
        constant
    } else {
        0.0
    };
    for d in &mut distances {
        *d *= *d;
    }
    center(&mut distances, n);
    if distances.iter().any(|x| !x.is_finite()) {
        return Err(ctx.stop(call.err_call(), "infinite or missing values in 'x'"));
    }
    let gram: Vec<_> = distances.iter().map(|v| v * -0.5).collect();
    let view = MatRef {
        data: &gram,
        nrow: n,
        ncol: n,
    };
    let result =
        symmetric_eigen(view, false).map_err(|e| ctx.stop(call.err_call(), e.to_string()))?;
    let cols = result.values[..k].iter().take_while(|&&v| v > 0.0).count();
    if cols < k {
        ctx.warn_now(
            call.err_call(),
            format!("only {cols} of the first {k} eigenvalues are > 0"),
        )?;
    }
    let mut coords = result.vectors;
    coords.truncate(n * cols);
    for (col, &value) in coords.chunks_exact_mut(n).zip(&result.values) {
        let factor = value.sqrt();
        for x in col {
            *x *= factor;
        }
    }
    let labels = names.0;
    let points = real(ctx, &coords)?;
    matrix(ctx, call, points, (n, cols), (labels, SEXP_NIL))?;
    if labels == SEXP_NIL {
        let names = ctx.session_mut().alloc_list(2)?;
        ctx.protect(names);
        set_attr(ctx, points, "dimnames", names)?;
    }
    if !list_ret {
        return Ok(points);
    }
    let goodness = result.values[..cols].iter().sum::<f64>();
    let absolute = result.values.iter().map(|x| x.abs()).sum::<f64>();
    let positive = result.values.iter().map(|x| x.max(0.0)).sum::<f64>();
    let gof = real(ctx, &[goodness / absolute, goodness / positive])?;
    let eigen = if eig {
        real(ctx, &result.values)?
    } else {
        SEXP_NIL
    };
    let x = if x_ret {
        let x = real(ctx, &distances)?;
        matrix(
            ctx,
            call,
            x,
            (n, n),
            if explicit_size == SEXP_NIL {
                names
            } else {
                (SEXP_NIL, SEXP_NIL)
            },
        )?
    } else {
        SEXP_NIL
    };
    let ac = real(ctx, &[ac])?;
    list(
        ctx,
        &[
            ("points", points),
            ("eig", eigen),
            ("x", x),
            ("ac", ac),
            ("GOF", gof),
        ],
        None,
    )
}

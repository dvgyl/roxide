use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_math::{
    MatRef,
    linalg::{
        factor::{dgetrf, matrix_norm_inf, matrix_norm_one},
        linpack::dqrdc2,
    },
};
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use crate::ops::common::{error, flag, string, vector_limit};

use super::{Dense, decompose, finite, input, math_error, size};

pub(in crate::linalg) fn kappa(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [z, exact, norm, method, triangular, uplo] = *args.values() else {
        return Err(error(ctx, call, "invalid kappa arguments"));
    };
    let exact = flag(ctx, exact, false)?;
    let a = input(
        ctx,
        call,
        z,
        "complex condition numbers are not yet implemented",
        exact,
    )?;
    let value = if a.values.is_empty() {
        0.0
    } else if exact {
        finite(ctx, call, &a)?;
        let d = decompose(a.view(), 0, 0)
            .map_err(|e| math_error(ctx, call, e))?
            .d;
        let min = d[d.len() - 1];
        if min == 0.0 {
            f64::INFINITY
        } else {
            d[0] / min
        }
    } else {
        let direct = direct(ctx, call, method)?;
        let triangular = flag(ctx, triangular, false)?;
        let kind = norm_name(ctx, call, norm)?;
        if direct && a.rows == a.cols {
            if triangular {
                let upper = string(ctx.session(), uplo).as_deref() == Some("U");
                triangular_kappa(ctx, call, a.view(), &kind, upper, true)?
            } else {
                direct_kappa(ctx, call, a, &kind)?
            }
        } else {
            if a.values.iter().any(|v| !v.is_finite()) {
                return Err(error(
                    ctx,
                    call,
                    "NA/NaN/Inf in foreign function call (arg 1)",
                ));
            }
            let r = qr_triangle(ctx, call, a)?;
            triangular_kappa(ctx, call, r.view(), &kind, true, false)?
        }
    };
    Ok(ctx.session_mut().scalar_real(value)?)
}

fn norm_name(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<String> {
    if x == SEXP_NIL {
        return Ok("1".to_string());
    }
    let (x, _) = ctx.session_mut().coerce(x, SexpType::Str)?;
    string(ctx.session(), x)
        .ok_or_else(|| error(ctx, call, "missing value where TRUE/FALSE needed"))
}

fn direct(ctx: &mut Ctx, call: &Call, method: SexpId) -> EvalResult<bool> {
    let s = ctx.session();
    let name = string(s, method);
    let len = s.xlength(method)?;
    let names = crate::ops::common::chars(s, method);
    let default = names.len() == 2
        && s.char_bytes(names[0]).ok() == Some(b"qr")
        && s.char_bytes(names[1]).ok() == Some(b"direct");
    if len > 1 && !default {
        return Err(error(ctx, call, "'arg' must be of length 1"));
    }
    match name.as_deref() {
        None if method == SEXP_NIL => Ok(false),
        Some("qr" | "q") => Ok(false),
        Some(value) if !value.is_empty() && "direct".starts_with(value) => Ok(true),
        _ => Err(error(ctx, call, "'arg' should be one of “qr”, “direct”")),
    }
}

fn direct_kappa(ctx: &mut Ctx, call: &Call, a: Dense, kind: &str) -> EvalResult<f64> {
    let one = match kind {
        "I" | "i" => false,
        "1" | "O" | "o" => true,
        _ => {
            return Err(error(
                ctx,
                call,
                format!("argument type[1]='{kind}' must be one of '1','O', or 'I'"),
            ));
        }
    };
    finite(ctx, call, &a)?;
    let norm = if one {
        matrix_norm_one(a.view())
    } else {
        matrix_norm_inf(a.view())
    };
    let n = a.rows;
    vector_limit(ctx, call, n, size_of::<usize>())?;
    let lu = dgetrf(a.values, n).map_err(|e| math_error(ctx, call, e))?;
    Ok(1.0 / lu.rcond(norm, one))
}

fn qr_triangle(ctx: &mut Ctx, call: &Call, mut a: Dense) -> EvalResult<Dense> {
    if a.rows < a.cols {
        let len = size(ctx, call, a.cols, a.rows, size_of::<f64>())?;
        let mut data = vec![0.0; len];
        for j in 0..a.rows {
            for i in 0..a.cols {
                data[j * a.cols + i] = a.values[i * a.rows + j];
            }
        }
        a.values = data;
        std::mem::swap(&mut a.rows, &mut a.cols);
    }
    let n = a.cols;
    vector_limit(ctx, call, n, size_of::<usize>())?;
    size(ctx, call, n, n, size_of::<f64>())?;
    let qr = dqrdc2(a.values, a.rows, n, 1e-7).map_err(|e| math_error(ctx, call, e))?;
    a.values = qr.r_mat(false).map_err(|e| math_error(ctx, call, e))?;
    a.rows = n;
    Ok(a)
}

fn triangular_kappa(
    ctx: &mut Ctx,
    call: &Call,
    a: MatRef<'_>,
    kind: &str,
    upper: bool,
    direct: bool,
) -> EvalResult<f64> {
    let transpose = kind == "I";
    let warning = match kind {
        "1" | "O" | "o" | "I" => None,
        "2" if !direct => Some("using 1-norm approximation for approximate 2-norm".to_string()),
        _ => Some(format!(
            "norm=\"{kind}\" not available here; using norm=\"1\""
        )),
    };
    if let Some(warning) = warning {
        ctx.warn_now(call.err_call(), warning)?;
    }
    let n = a.nrow;
    vector_limit(ctx, call, n, size_of::<f64>())?;
    roxide_math::linalg::spectral::triangular_condition(a, upper, transpose)
        .map_err(|e| math_error(ctx, call, e))
}

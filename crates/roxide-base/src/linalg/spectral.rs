#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod condition;
mod symmetry;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_math::{
    Cplx, MatRef,
    linalg::spectral::{MatrixNorm, general_eigen, matrix_norm, svd as decompose, symmetric_eigen},
};
use roxide_value::{NA_REAL, RComplex, RLogical, SEXP_NIL, SexpType, ValueApi, is_na_real};

use crate::ops::common::{ch, error, flag, list, number, set, set_names, string, vector_limit};

use super::{Dense, complex, dimensions, math_error, size};

pub(super) use condition::kappa;
pub(super) use symmetry::is_symmetric;

const NONFINITE: &str = "infinite or missing values in 'x'";

pub(super) fn svd(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, nu, nv, transposed] = *args.values() else {
        return Err(error(ctx, call, "invalid SVD arguments"));
    };
    let transposed = flag(ctx, transposed, false)?;
    let a = input(ctx, call, x, "complex SVD is not yet implemented", true)?;
    if a.rows == 0 || a.cols == 0 {
        return Err(error(ctx, call, "a dimension is zero"));
    }
    finite(ctx, call, &a)?;
    let nu = columns(ctx, call, nu, a.rows)?;
    let nv = columns(ctx, call, nv, a.cols)?;
    size(ctx, call, a.rows, nu, size_of::<f64>())?;
    size(ctx, call, a.cols, nv, size_of::<f64>())?;
    let factors = decompose(a.view(), nu, nv).map_err(|e| math_error(ctx, call, e))?;
    let d = ctx.session_mut().alloc_real_from(&factors.d)?;
    ctx.protect(d);
    let mut items = vec![("d", d)];
    if nu != 0 {
        let u = ctx.session_mut().alloc_real_from(&factors.u)?;
        items.push(("u", matrix(ctx, u, a.rows, nu)?));
    }
    if nv != 0 {
        if transposed {
            let vt: Vec<_> = (0..a.cols)
                .flat_map(|i| {
                    (0..nv).map({
                        let v = &factors.v;
                        move |j| v[j * a.cols + i]
                    })
                })
                .collect();
            let v = ctx.session_mut().alloc_real_from(&vt)?;
            items.push(("vt", matrix(ctx, v, nv, a.cols)?));
        } else {
            let v = ctx.session_mut().alloc_real_from(&factors.v)?;
            items.push(("v", matrix(ctx, v, a.cols, nv)?));
        }
    }
    result(ctx, &items, None)
}

fn columns(ctx: &mut Ctx, call: &Call, x: SexpId, cap: usize) -> EvalResult<usize> {
    let s = ctx.session();
    let ty = s.type_of(x)?;
    if ty == SexpType::Cplx {
        return Err(error(ctx, call, "invalid comparison with complex values"));
    }
    if !matches!(ty, SexpType::Lgl | SexpType::Int | SexpType::Real) {
        return Err(error(ctx, call, "invalid 'x' type in 'x || y'"));
    }
    let n = s.xlength(x)?;
    if n > 1 {
        return Err(error(
            ctx,
            call,
            format!("'length = {n}' in coercion to 'logical(1)'"),
        ));
    }
    let n = number(ctx.session_mut(), x)?;
    if n.is_nan() {
        return Err(error(ctx, call, "missing value where TRUE/FALSE needed"));
    }
    if n < 0.0 {
        return Err(error(
            ctx,
            call,
            "argument must be coercible to non-negative integer",
        ));
    }
    Ok((n as usize).min(cap))
}

pub(super) fn eigen(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, symmetric, only, detect] = *args.values() else {
        return Err(error(ctx, call, "invalid eigen arguments"));
    };
    let a = input(ctx, call, x, "complex eigen is not yet implemented", true)?;
    let n = a.rows;
    if n != a.cols {
        return Err(error(ctx, call, "non-square matrix in 'eigen'"));
    }
    if n == 0 {
        return Err(error(ctx, call, "0 x 0 matrix"));
    }
    finite(ctx, call, &a)?;
    let symmetric = if flag(ctx, detect, false)? {
        symmetry::real(&a.values, n, symmetry::TOLERANCE, symmetry::TOLERANCE * 8.0)
    } else {
        flag(ctx, symmetric, false)?
    };
    if ctx.session().xlength(only)? > 1 {
        return Err(error(ctx, call, "the condition has length > 1"));
    }
    let (only, _) = ctx.session_mut().coerce(only, SexpType::Lgl)?;
    let Some(only) = ctx
        .session()
        .lgl(only)?
        .iter()
        .next()
        .and_then(RLogical::get)
    else {
        return Err(error(ctx, call, "invalid 'only.values' argument"));
    };
    let (values, vectors) = if symmetric {
        let e = symmetric_eigen(a.view(), only).map_err(|e| math_error(ctx, call, e))?;
        let values = ctx.session_mut().alloc_real_from(&e.values)?;
        ctx.protect(values);
        let vectors = if only {
            SEXP_NIL
        } else {
            ctx.session_mut().alloc_real_from(&e.vectors)?
        };
        (values, vectors)
    } else {
        vector_limit(ctx, call, n, size_of::<Cplx>())?;
        if !only {
            size(ctx, call, n, n, size_of::<Cplx>())?;
        }
        let e = general_eigen(a.view(), only).map_err(|e| math_error(ctx, call, e))?;
        // R keeps complex storage if any imaginary part is significant.
        let complex = e
            .values
            .iter()
            .any(|v| v.im.abs() > (10.0 * f64::EPSILON) * v.re.abs());
        let values = eigen_vector(ctx, &e.values, complex)?;
        ctx.protect(values);
        let vectors = if only {
            SEXP_NIL
        } else {
            if complex || e.values.iter().all(|v| v.im == 0.0) {
                eigen_vector(ctx, &e.vectors, complex)?
            } else {
                let packed: Vec<_> = e
                    .values
                    .iter()
                    .zip(e.vectors.chunks_exact(n))
                    .flat_map(|(value, column)| {
                        column
                            .iter()
                            .map(|v| if value.im < 0.0 { -v.im } else { v.re })
                    })
                    .collect();
                ctx.session_mut().alloc_real_from(&packed)?
            }
        };
        (values, vectors)
    };
    let vectors = if only {
        vectors
    } else {
        matrix(ctx, vectors, n, n)?
    };
    result(
        ctx,
        &[("values", values), ("vectors", vectors)],
        (!only).then_some("eigen"),
    )
}

fn eigen_vector(ctx: &mut Ctx, data: &[Cplx], complex: bool) -> EvalResult {
    let s = ctx.session_mut();
    if complex {
        let a: Vec<_> = data
            .iter()
            .map(|v| RComplex { re: v.re, im: v.im })
            .collect();
        Ok(s.alloc_cplx_from(&a)?)
    } else {
        let a: Vec<_> = data.iter().map(|v| v.re).collect();
        Ok(s.alloc_real_from(&a)?)
    }
}

fn matrix(ctx: &mut Ctx, value: SexpId, rows: usize, cols: usize) -> EvalResult {
    ctx.protect(value);
    let dim = ctx
        .session_mut()
        .alloc_int_from(&[rows as i32, cols as i32])?;
    ctx.protect(dim);
    set(ctx.session_mut(), value, "dim", dim)?;
    Ok(value)
}

fn result(ctx: &mut Ctx, fields: &[(&str, SexpId)], class: Option<&str>) -> EvalResult {
    let values: Vec<_> = fields.iter().map(|&(_, x)| x).collect();
    let out = list(ctx.session_mut(), &values)?;
    ctx.protect(out);
    let s = ctx.session_mut();
    let names = fields
        .iter()
        .map(|&(name, _)| ch(s, name))
        .collect::<Result<Vec<_>, _>>()?;
    set_names(s, out, &names)?;
    if let Some(class) = class {
        let class = ctx.session_mut().scalar_str(class)?;
        ctx.protect(class);
        set(ctx.session_mut(), out, "class", class)?;
    }
    Ok(out)
}

fn input(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    complex_error: &str,
    numeric_only: bool,
) -> EvalResult<Dense> {
    let ty = ctx.session().type_of(x)?;
    if ty == SexpType::Cplx {
        return Err(error(ctx, call, complex_error));
    }
    if ty == SexpType::Nil {
        return Err(error(
            ctx,
            call,
            "'data' must be of a vector type, was 'NULL'",
        ));
    }
    if matches!(ty, SexpType::Vec | SexpType::Expr) {
        return Err(error(
            ctx,
            call,
            format!(
                "default method not implemented for type '{}'",
                ty.type_name(false)
            ),
        ));
    }
    if numeric_only && matches!(ty, SexpType::Str | SexpType::Raw) && ctx.session().xlength(x)? != 0
    {
        return Err(error(ctx, call, NONFINITE));
    }
    ctx.protect(x);
    Dense::read(ctx, call, x)
}

fn finite(ctx: &mut Ctx, call: &Call, a: &Dense) -> EvalResult<()> {
    if a.values.iter().any(|v| !v.is_finite()) {
        return Err(error(ctx, call, NONFINITE));
    }
    Ok(())
}

fn norm_type(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<MatrixNorm> {
    let s = ctx.session();
    if s.type_of(x)? != SexpType::Str || s.xlength(x)? == 0 {
        return Err(error(ctx, call, "'type' must be a character string"));
    }
    let name = string(s, x).unwrap_or_else(|| "NA".to_string());
    if name.len() != 1 {
        return Err(error(
            ctx,
            call,
            format!("argument type[1]='{name}' must be a character string of string length 1"),
        ));
    }
    match name.as_bytes()[0].to_ascii_uppercase() {
        b'1' | b'O' => Ok(MatrixNorm::One),
        b'I' => Ok(MatrixNorm::Infinity),
        b'F' | b'E' => Ok(MatrixNorm::Frobenius),
        b'M' => Ok(MatrixNorm::Maximum),
        _ => Err(error(
            ctx,
            call,
            format!("argument type[1]='{name}' must be one of 'M','1','O','I','F' or 'E'"),
        )),
    }
}

pub(super) fn norm(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, kind] = *args.values() else {
        return Err(error(ctx, call, "invalid norm arguments"));
    };
    let s = ctx.session();
    let spectral = s.type_of(kind)? == SexpType::Str
        && s.xlength(kind)? == 1
        && string(s, kind).as_deref() == Some("2");
    let ty = s.type_of(x)?;
    let (rows, cols) = dimensions(s, x).unwrap_or((s.xlength(x)?, 1));
    let kind = if spectral {
        MatrixNorm::Spectral
    } else {
        if !super::numeric(ty) {
            return Err(error(
                ctx,
                call,
                format!("invalid 'x': type \"{}\"", ty.type_name(false)),
            ));
        }
        if dimensions(ctx.session(), x).is_none() {
            return Err(error(ctx, call, "'A' must be a numeric matrix"));
        }
        norm_type(ctx, call, kind)?
    };
    let value = if ty == SexpType::Cplx {
        let a = complex(ctx, call, x)?;
        complex_norm(ctx, call, &a, rows, cols, kind)?
    } else if ty == SexpType::Nil {
        0.0
    } else if ty == SexpType::Str
        && ctx
            .session()
            .str_slice(x)?
            .contains(&roxide_value::CharId::NA)
    {
        NA_REAL
    } else {
        let a = input(ctx, call, x, "complex SVD is not yet implemented", true)?;
        real_norm(ctx, call, a.view(), kind)?
    };
    Ok(ctx.session_mut().scalar_real(value)?)
}

fn real_norm(ctx: &mut Ctx, call: &Call, a: MatRef<'_>, kind: MatrixNorm) -> EvalResult<f64> {
    let nan = match kind {
        MatrixNorm::Spectral if a.data.iter().any(|v| v.is_nan()) => Some(NA_REAL),
        MatrixNorm::One | MatrixNorm::Maximum => a.data.iter().rfind(|v| v.is_nan()).copied(),
        MatrixNorm::Infinity => a.data.iter().find(|v| is_na_real(**v)).copied(),
        _ => None,
    };
    if let Some(nan) = nan {
        return Ok(nan);
    }
    matrix_norm(a, kind).map_err(|e| {
        if kind == MatrixNorm::Spectral {
            error(ctx, call, NONFINITE)
        } else {
            math_error(ctx, call, e)
        }
    })
}

fn complex_norm(
    ctx: &mut Ctx,
    call: &Call,
    a: &[Cplx],
    rows: usize,
    cols: usize,
    kind: MatrixNorm,
) -> EvalResult<f64> {
    if kind != MatrixNorm::Spectral {
        let data: Vec<_> = a
            .iter()
            .map(|v| {
                if v.re.is_nan() {
                    v.re
                } else if v.im.is_nan() {
                    v.im
                } else {
                    v.re.hypot(v.im)
                }
            })
            .collect();
        return real_norm(
            ctx,
            call,
            MatRef {
                data: &data,
                nrow: rows,
                ncol: cols,
            },
            kind,
        );
    }
    if a.iter().any(|v| v.re.is_nan() || v.im.is_nan()) {
        return Ok(NA_REAL);
    }
    if a.iter().any(|v| !v.re.is_finite() || !v.im.is_finite()) {
        return Err(error(ctx, call, NONFINITE));
    }
    let doubled = |x: usize| x.checked_mul(2);
    let (Some(m), Some(n)) = (doubled(rows), doubled(cols)) else {
        return Err(error(ctx, call, "vector size limit exceeded"));
    };
    let len = size(ctx, call, m, n, size_of::<f64>())?;
    let mut data = vec![0.0; len];
    for j in 0..cols {
        for i in 0..rows {
            let v = a[j * rows + i];
            let k = j * m + i;
            let l = (j + cols) * m + i;
            data[k] = v.re;
            data[k + rows] = v.im;
            data[l] = -v.im;
            data[l + rows] = v.re;
        }
    }
    real_norm(
        ctx,
        call,
        MatRef {
            data: &data,
            nrow: m,
            ncol: n,
        },
        kind,
    )
}

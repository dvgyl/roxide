use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::linalg::linpack::dqrdc2;
use roxide_value::{SEXP_NIL, ValueApi};

use super::{list, strings};
use crate::distn::support::{
    attr, limit, logical, number, numbers, set_attr, set_dim, set_dimnames,
};

pub(super) fn poly(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let points = numbers(ctx, call, args.get(0))?;
    let degree = number(ctx, call, args.get(1))?;
    if !degree.is_finite() || degree < 1.0 || degree.fract() != 0.0 || degree >= f64::from(i32::MAX)
    {
        return Err(ctx.stop(call.err_call(), "'degree' must be at least 1"));
    }
    let degree = degree as usize;
    let coefs = args.get(2);
    let raw = logical(ctx, call, args.get(3))? == Some(true);
    let simple = logical(ctx, call, args.get(4))? == Some(true);
    let n = points.len();
    limit::<f64>(ctx, call, n.saturating_mul(degree))?;
    limit::<String>(ctx, call, degree)?;
    let mut values = Vec::with_capacity(n * degree);
    let mut new_coefs = SEXP_NIL;

    if raw {
        for k in 1..=degree {
            values.extend(points.iter().map(|x| x.powi(k as i32)));
        }
    } else {
        let estimate = coefs == SEXP_NIL;
        if estimate {
            let Fit {
                basis,
                alpha,
                norm2,
            } = fit(ctx, call, &points, degree)?;
            values = basis;
            let a = ctx.session_mut().alloc_real_from(&alpha)?;
            let b = ctx.session_mut().alloc_real_from(&norm2)?;
            new_coefs = list(ctx.session_mut(), &[a, b])?;
            let names = strings(ctx.session_mut(), ["alpha", "norm2"])?;
            set_attr(ctx, new_coefs, "names", names)?;
        } else {
            let (alpha, norm2) = coefficients(ctx, call, coefs, degree)?;
            // Evaluate the monic polynomials with the three-term recurrence.
            let mut previous = vec![0.0; n];
            let mut current = vec![1.0; n];
            for k in 0..degree {
                let ratio = if k == 0 { 0.0 } else { norm2[k + 1] / norm2[k] };
                for ((value, &z), &x) in previous.iter_mut().zip(&current).zip(&points) {
                    *value = (x - alpha[k]) * z - ratio * *value;
                }
                std::mem::swap(&mut previous, &mut current);
                let scale = norm2[k + 2].sqrt();
                values.extend(current.iter().map(|v| v / scale));
            }
            new_coefs = coefs;
        }
    }
    let result = ctx.session_mut().alloc_real_from(&values)?;
    set_dim(ctx, call, result, n, degree)?;
    let column_names = (1..=degree).map(|k| k.to_string()).collect::<Vec<_>>();
    let column_names = strings(ctx.session_mut(), column_names.iter().map(String::as_str))?;
    let row_names = attr(ctx, args.get(0), "names")?;
    set_dimnames(ctx, result, row_names, column_names)?;
    if !simple {
        set_attr(ctx, result, "coefs", new_coefs)?;
        let degree = ctx.session_mut().alloc_compact_intseq(1, degree, 1)?;
        set_attr(ctx, result, "degree", degree)?;
        let class = strings(ctx.session_mut(), ["poly", "matrix"])?;
        set_attr(ctx, result, "class", class)?;
    }
    Ok(result)
}

struct Fit {
    basis: Vec<f64>,
    alpha: Vec<f64>,
    norm2: Vec<f64>,
}

fn fit(ctx: &mut Ctx, site: &Call, points: &[f64], degree: usize) -> EvalResult<Fit> {
    if points.iter().any(|x| x.is_nan()) {
        return Err(ctx.stop(site.err_call(), "missing values are not allowed in 'poly'"));
    }
    let mut unique = points.to_vec();
    sort_distinct(&mut unique);
    if degree >= unique.len() {
        return Err(ctx.stop(
            site.err_call(),
            "'degree' must be less than number of unique points",
        ));
    }
    let n = points.len();
    let width = degree + 1;
    limit::<f64>(ctx, site, n.saturating_mul(width))?;
    let origin = points.iter().map(|x| x / n as f64).sum::<f64>();
    let origin = origin + points.iter().map(|x| (x - origin) / n as f64).sum::<f64>();
    let mut powers = vec![1.0; n * width];
    for j in 1..width {
        let (previous, rest) = powers.split_at_mut(j * n);
        for ((out, &p), &x) in rest.iter_mut().zip(&previous[(j - 1) * n..]).zip(points) {
            *out = p * (x - origin);
        }
    }
    if powers.iter().any(|x| !x.is_finite()) {
        return Err(ctx.stop(
            site.err_call(),
            "NA/NaN/Inf in foreign function call (arg 1)",
        ));
    }

    // A positive diagonal in R makes the leading coefficient of each
    // orthogonal polynomial positive. Its square is the squared monic norm.
    let qr = dqrdc2(powers, n, width, 0.0).map_err(|e| ctx.stop(site.err_call(), e.to_string()))?;
    let mut norm2 = vec![1.0];
    let signs = (0..width)
        .map(|j| {
            let diagonal = qr.qr[j * (n + 1)];
            norm2.push(diagonal * diagonal);
            diagonal.signum()
        })
        .collect::<Vec<_>>();
    let mut basis = qr
        .q_mat(false, Some(&signs))
        .map_err(|e| ctx.stop(site.err_call(), e.to_string()))?;
    let alpha = basis
        .chunks_exact(n)
        .take(degree)
        .map(|col| {
            origin
                + points
                    .iter()
                    .zip(col)
                    .map(|(x, p)| (x - origin) * p * p)
                    .sum::<f64>()
        })
        .collect();
    basis.drain(..n);
    Ok(Fit {
        basis,
        alpha,
        norm2,
    })
}

fn coefficients(
    ctx: &mut Ctx,
    site: &Call,
    coefs: roxide_gc::SexpId,
    degree: usize,
) -> EvalResult<(Vec<f64>, Vec<f64>)> {
    let names = attr(ctx, coefs, "names")?;
    let fields = ctx.session().list_slice(coefs)?.to_vec();
    let get = |name: &str| {
        ctx.session().str_slice(names).ok().and_then(|names| {
            names.iter().zip(&fields).find_map(|(&tag, &value)| {
                (ctx.session().char_bytes(tag).ok() == Some(name.as_bytes())).then_some(value)
            })
        })
    };
    let (Some(a), Some(b)) = (get("alpha"), get("norm2")) else {
        return Err(ctx.stop(site.err_call(), "invalid polynomial coefficients"));
    };
    let alpha = numbers(ctx, site, a)?;
    let norm2 = numbers(ctx, site, b)?;
    if alpha.len() < degree || norm2.len() < degree + 2 {
        return Err(ctx.stop(site.err_call(), "invalid polynomial coefficients"));
    }
    Ok((alpha, norm2))
}

fn sort_distinct(values: &mut Vec<f64>) {
    values.sort_by(f64::total_cmp);
    values.dedup_by(|x, y| *x == *y);
}

pub(super) fn distinct(ctx: &mut Ctx, site: &Call, args: &mut Args) -> EvalResult {
    let mut values = numbers(ctx, site, args.get(0))?;
    sort_distinct(&mut values);
    Ok(ctx.session_mut().alloc_real_from(&values)?)
}

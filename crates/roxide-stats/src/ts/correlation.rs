use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{MatMut, linalg::factor::dgetrf};
use roxide_value::{NA_REAL, ValueApi};

use crate::distn::support::{limit, logical, number, numbers};

use super::{array, shape};

pub(super) fn acf(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let (rows, cols) = shape(ctx, args.get(0))?;
    let lag = number(ctx, call, args.get(1))? as usize;
    let correlation = logical(ctx, call, args.get(2))? == Some(true);
    let demean = logical(ctx, call, args.get(3))? == Some(true);
    let frequency = number(ctx, call, args.get(4))?;
    let count = lag
        .checked_add(1)
        .and_then(|n| n.checked_mul(cols))
        .and_then(|n| n.checked_mul(cols))
        .ok_or_else(|| ctx.stop(call.err_call(), "vector size limit exceeded"))?;
    limit::<f64>(ctx, call, count)?;
    let mut x = numbers(ctx, call, args.get(0))?;
    if rows == 0 || cols == 0 || lag >= rows || rows.checked_mul(cols) != Some(x.len()) {
        return Err(ctx.stop(call.err_call(), "invalid time series"));
    }
    if demean {
        for column in x.chunks_exact_mut(rows) {
            let (sum, count) = column
                .iter()
                .filter(|v| !v.is_nan())
                .fold((0.0, 0usize), |(sum, count), v| (sum + v, count + 1));
            for value in column {
                *value -= sum / count as f64;
            }
        }
    }
    let mut data = Vec::with_capacity(count);
    let mut lags = Vec::with_capacity(count);
    let std: Vec<_> = x
        .chunks_exact(rows)
        .map(|col| covariance(col, col, 0).sqrt())
        .collect();
    for (j, b) in x.chunks_exact(rows).enumerate() {
        for (i, a) in x.chunks_exact(rows).enumerate() {
            let sign = if i > j { -1.0 } else { 1.0 };
            for k in 0..=lag {
                let mut value = covariance(a, b, k);
                if correlation {
                    value = if rows == 1 && i == j {
                        1.0
                    } else {
                        (value / (std[i] * std[j])).clamp(-1.0, 1.0)
                    };
                }
                data.push(value);
                lags.push(sign * k as f64 / frequency);
            }
        }
    }
    let dims = [lag + 1, cols, cols];
    let data = array(ctx, call, &data, &dims)?;
    let lags = array(ctx, call, &lags, &dims)?;
    let result = ctx.session_mut().alloc_list(2)?;
    ctx.session_mut().list_set(result, 0, data)?;
    ctx.session_mut().list_set(result, 1, lags)?;
    Ok(result)
}

fn covariance(a: &[f64], b: &[f64], lag: usize) -> f64 {
    let (mut sum, mut count) = (0.0, 0);
    for (&a, &b) in a[lag..].iter().zip(b) {
        if !a.is_nan() && !b.is_nan() {
            sum += a * b;
            count += 1;
        }
    }
    if count == 0 {
        NA_REAL
    } else {
        sum / (count + lag) as f64
    }
}

pub(super) fn pacf(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let lag = number(ctx, call, args.get(1))? as usize;
    let cols = number(ctx, call, args.get(2))? as usize;
    let values = numbers(ctx, call, args.get(0))?;
    let count = lag
        .checked_mul(cols)
        .and_then(|n| n.checked_mul(cols))
        .ok_or_else(|| ctx.stop(call.err_call(), "vector size limit exceeded"))?;
    if lag == 0 || cols == 0 || count.checked_add(cols.saturating_mul(cols)) != Some(values.len()) {
        return Err(ctx.stop(call.err_call(), "invalid partial correlation dimensions"));
    }
    let mut output = vec![0.0; count];
    if cols == 1 {
        levinson(&values, &mut output);
    } else {
        let width = lag * cols;
        let max_square = width
            .checked_mul(width)
            .ok_or_else(|| ctx.stop(call.err_call(), "vector size limit exceeded"))?;
        limit::<f64>(ctx, call, max_square)?;
        multivariate(&values, lag, cols, &mut output)
            .map_err(|_| ctx.stop(call.err_call(), "Singular matrix in qr_solve"))?;
    }
    array(ctx, call, &output, &[lag, cols, cols])
}

fn levinson(r: &[f64], out: &mut [f64]) {
    let mut coeff = vec![0.0; out.len()];
    let mut error = r[0];
    for k in 0..out.len() {
        let prediction = (0..k).map(|j| coeff[j] * r[k - j]).sum::<f64>();
        let reflection = (r[k + 1] - prediction) / error;
        for j in 0..k / 2 {
            let i = k - j - 1;
            let (a, b) = (coeff[j], coeff[i]);
            coeff[j] = a - reflection * b;
            coeff[i] = b - reflection * a;
        }
        if !k.is_multiple_of(2) {
            coeff[k / 2] *= 1.0 - reflection;
        }
        coeff[k] = reflection;
        out[k] = reflection;
        error *= 1.0 - reflection * reflection;
    }
}

fn multivariate(
    r: &[f64],
    lag: usize,
    cols: usize,
    out: &mut [f64],
) -> Result<(), roxide_math::MathError> {
    let get = |k, i, j| r[(j * cols + i) * (lag + 1) + k];
    for order in 1..=lag {
        let width = order * cols;
        let mut gram = vec![0.0; width * width];
        let mut rhs = vec![0.0; width * cols];
        for j in 0..width {
            let (vj, tj) = (j % cols, j / cols);
            for i in 0..width {
                let (vi, ti) = (i % cols, i / cols);
                gram[j * width + i] = if ti <= tj {
                    get(tj - ti, vi, vj)
                } else {
                    get(ti - tj, vj, vi)
                };
            }
            for v in 0..cols {
                rhs[v * width + j] = get(tj + 1, v, vj);
            }
        }
        let factors = dgetrf(gram, width)?;
        factors.solve(&mut MatMut::new(&mut rhs, width, cols)?)?;
        for j in 0..cols {
            for i in 0..cols {
                out[(j * cols + i) * lag + order - 1] = rhs[i * width + width - cols + j];
            }
        }
    }
    Ok(())
}

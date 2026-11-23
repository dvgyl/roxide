use super::{list, reals};
use crate::distn::support::{number, numbers};
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::NA_REAL;

pub(super) fn approx(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = numbers(ctx, call, args.get(0))?;
    let y = numbers(ctx, call, args.get(1))?;
    let query = numbers(ctx, call, args.get(2))?;
    let ends = numbers(ctx, call, args.get(3))?;
    let f = number(ctx, call, args.get(4))?;
    if x.is_empty() || x.len() != y.len() || ends.len() != 2 {
        return Err(ctx.stop(call.err_call(), "invalid interpolation data"));
    }
    let result: Vec<_> = query
        .into_iter()
        .map(|q| {
            if q.is_nan() {
                return q;
            }
            let i = x.partition_point(|&v| v <= q);
            if i > 0 && q == x[i - 1] {
                return y[i - 1];
            }
            if i == 0 {
                return ends[0];
            }
            if i == x.len() {
                return ends[1];
            }
            let weight = if f < 0.0 {
                (q - x[i - 1]) / (x[i] - x[i - 1])
            } else {
                f
            };
            match weight {
                0.0 => y[i - 1],
                1.0 => y[i],
                _ if f < 0.0 => y[i - 1] + weight * (y[i] - y[i - 1]),
                _ => (1.0 - weight) * y[i - 1] + weight * y[i],
            }
        })
        .collect();
    reals(ctx, &result)
}

fn tridiagonal(lower: &[f64], diagonal: &mut [f64], upper: &[f64], rhs: &mut [f64]) {
    for i in 1..rhs.len() {
        let scale = lower[i - 1] / diagonal[i - 1];
        diagonal[i] -= scale * upper[i - 1];
        rhs[i] -= scale * rhs[i - 1];
    }
    for i in (0..rhs.len()).rev() {
        if i + 1 < rhs.len() {
            rhs[i] -= upper[i] * rhs[i + 1];
        }
        rhs[i] /= diagonal[i];
    }
}

fn second_derivatives(x: &[f64], h: &[f64], slope: &[f64], method: i32) -> Vec<f64> {
    let n = x.len();
    let m = n - 2;
    let mut diagonal: Vec<_> = h.windows(2).map(|w| 2.0 * (w[0] + w[1])).collect();
    let side = &h[1..n - 2];
    let mut rhs: Vec<_> = slope.windows(2).map(|w| 6.0 * (w[1] - w[0])).collect();
    let (left, right) = match method {
        2 => {
            let mut edge = vec![0.0; m];
            edge[0] += h[0];
            edge[m - 1] += h[n - 2];
            let mut copy = diagonal.clone();
            tridiagonal(side, &mut copy, side, &mut edge);
            tridiagonal(side, &mut diagonal, side, &mut rhs);
            let boundary =
                (6.0 * (slope[0] - slope[n - 2]) - h[0] * rhs[0] - h[n - 2] * rhs[m - 1])
                    / (2.0 * (h[0] + h[n - 2]) - h[0] * edge[0] - h[n - 2] * edge[m - 1]);
            for (v, e) in rhs.iter_mut().zip(edge) {
                *v -= boundary * e;
            }
            (boundary, boundary)
        }
        3 => {
            tridiagonal(side, &mut diagonal, side, &mut rhs);
            (0.0, 0.0)
        }
        _ => {
            // Estimate the third derivative at each end from four adjacent knots.
            let third = |i: usize| {
                6.0 * ((slope[i + 2] - slope[i + 1]) / (x[i + 3] - x[i + 1])
                    - (slope[i + 1] - slope[i]) / (x[i + 2] - x[i]))
                    / (x[i + 3] - x[i])
            };
            let left_diff = if n > 3 { h[0] * third(0) } else { 0.0 };
            let right_diff = if n > 3 { h[n - 2] * third(n - 4) } else { 0.0 };
            diagonal[0] += h[0];
            diagonal[m - 1] += h[n - 2];
            rhs[0] += h[0] * left_diff;
            rhs[m - 1] -= h[n - 2] * right_diff;
            tridiagonal(side, &mut diagonal, side, &mut rhs);
            (rhs[0] - left_diff, rhs[m - 1] + right_diff)
        }
    };
    let mut result = Vec::with_capacity(n);
    result.push(left);
    result.extend(rhs);
    result.push(right);
    result
}

fn monotone_slopes(slope: &[f64], b: &mut [f64]) {
    b[0] = slope[0];
    b[slope.len()] = slope[slope.len() - 1];
    for (i, m) in b[1..slope.len()].iter_mut().enumerate() {
        *m = (slope[i] + slope[i + 1]) * 0.5;
    }
    for (i, &s) in slope.iter().enumerate() {
        if s == 0.0 {
            b[i] = 0.0;
            b[i + 1] = 0.0;
            continue;
        }
        let alpha = b[i] / s;
        let beta = b[i + 1] / s;
        let norm = alpha.hypot(beta);
        if norm > 3.0 {
            b[i] *= 3.0 / norm;
            b[i + 1] *= 3.0 / norm;
        }
    }
}

pub(super) fn cubic(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = numbers(ctx, call, args.get(0))?;
    let y = numbers(ctx, call, args.get(1))?;
    let method = number(ctx, call, args.get(2))? as i32;
    let n = x.len();
    if n == 0 || n != y.len() || x.windows(2).any(|v| v[0] >= v[1]) {
        return Err(ctx.stop(call.err_call(), "invalid spline data"));
    }
    let h: Vec<_> = x.windows(2).map(|w| w[1] - w[0]).collect();
    let slope: Vec<_> = y
        .windows(2)
        .zip(&h)
        .map(|(w, h)| (w[1] - w[0]) / h)
        .collect();
    let mut b = vec![0.0; n];
    if n == 2 {
        b.fill(slope[0]);
    } else if n > 2 && method == 4 {
        monotone_slopes(&slope, &mut b);
    } else if n > 2 {
        let z = second_derivatives(&x, &h, &slope, method);
        for i in 0..n - 1 {
            b[i] = slope[i] - h[i] * (2.0 * z[i] + z[i + 1]) / 6.0;
        }
        b[n - 1] = slope[n - 2] + h[n - 2] * (z[n - 2] + 2.0 * z[n - 1]) / 6.0;
        if method == 5 {
            for i in 0..n {
                let left = slope[i.saturating_sub(1)];
                let right = slope[i.min(n - 2)];
                let limit = 3.0 * left.abs().min(right.abs());
                b[i] = if left < 0.0 || right < 0.0 {
                    b[i].clamp(-limit, 0.0)
                } else {
                    b[i].clamp(0.0, limit)
                };
            }
        }
    }
    let mut c = vec![0.0; n];
    let mut d = vec![0.0; n];
    for i in 0..n - 1 {
        c[i] = (3.0 * slope[i] - 2.0 * b[i] - b[i + 1]) / h[i];
        d[i] = (b[i] + b[i + 1] - 2.0 * slope[i]) / (h[i] * h[i]);
    }
    if n > 1 && method != 3 {
        d[n - 1] = d[n - 2];
        c[n - 1] = c[n - 2] + 3.0 * h[n - 2] * d[n - 2];
    }
    let b = reals(ctx, &b)?;
    let c = reals(ctx, &c)?;
    let d = reals(ctx, &d)?;
    list(ctx, &[b, c, d])
}

pub(super) fn spline(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = numbers(ctx, call, args.get(0))?;
    let y = numbers(ctx, call, args.get(1))?;
    let b = numbers(ctx, call, args.get(2))?;
    let c = numbers(ctx, call, args.get(3))?;
    let d = numbers(ctx, call, args.get(4))?;
    let options = numbers(ctx, call, args.get(5))?;
    let n = x.len();
    if n == 0 || [y.len(), b.len(), c.len(), d.len()].iter().any(|&m| m != n) || options.len() < 2 {
        return Err(ctx.stop(call.err_call(), "invalid spline data"));
    }
    let method = options[0] as i32;
    let deriv = options[1] as i32;
    let mut interval = 0;
    let result: Vec<_> = options[2..]
        .iter()
        .map(|&q| {
            if q.is_nan() {
                return q;
            }
            let q = if method == 2 && n > 1 {
                (q - x[0]).rem_euclid(x[n - 1] - x[0]) + x[0]
            } else {
                q
            };
            if q.is_nan() {
                return NA_REAL;
            }
            if method == 4 && (q < x[0] || q >= x[n - 1]) {
                let i = if q < x[0] { 0 } else { n - 1 };
                return match deriv {
                    0 => y[i] + (q - x[i]) * b[i],
                    1 => b[i],
                    _ => 0.0,
                };
            }
            if method != 4 && (q < x[interval] || (interval + 1 < n && q > x[interval + 1])) {
                interval = x.partition_point(|&v| v <= q).saturating_sub(1);
            }
            let i = if method == 4 {
                x.partition_point(|&v| v <= q).saturating_sub(1)
            } else {
                interval
            };
            let h = q - x[i];
            let (c, d) = if method == 3 && q <= x[0] {
                (0.0, 0.0)
            } else {
                (c[i], d[i])
            };
            match deriv {
                0 => y[i] + h * (b[i] + h * (c + h * d)),
                1 => b[i] + h * (2.0 * c + h * 3.0 * d),
                2 => 2.0 * c + h * 6.0 * d,
                3 if method == 4 => {
                    let width = x.get(i + 1).map_or(0.0, |next| next - x[i]);
                    6.0 * d * width
                }
                _ => 6.0 * d,
            }
        })
        .collect();
    reals(ctx, &result)
}

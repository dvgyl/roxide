//! Golub–Kahan reduction and implicit QR on the resulting bidiagonal matrix.

use crate::{MatRef, MathError, linalg::factor::dlarfg};

use super::{area, eye, givens, reflect, rotate, scale};

/// Singular values and vectors of a real matrix.
#[derive(Debug)]
pub struct Svd {
    /// Singular values in decreasing order.
    pub d: Vec<f64>,
    /// Requested left singular vectors in column-major order.
    pub u: Vec<f64>,
    /// Requested right singular vectors in column-major order.
    pub v: Vec<f64>,
}

/// Compute the real singular value decomposition of a nonempty matrix.
///
/// The result has `nu` left and `nv` right columns. Zero skips that set of vectors.
/// The requested column counts must not exceed their respective matrix dimensions.
pub fn svd(a: MatRef<'_>, nu: usize, nv: usize) -> Result<Svd, MathError> {
    let (m, n) = (a.nrow, a.ncol);
    MatRef::new(a.data, m, n)?;
    if m == 0 || n == 0 || nu > m || nv > n {
        return Err(MathError::Dim);
    }
    let scale = scale(a.data.iter().copied())?;
    let len = area::<f64>(m, n)?;
    if m >= n {
        let packed = a.data.iter().map(|x| x / scale).collect();
        tall(packed, m, n, nu, nv, scale)
    } else {
        let mut packed = vec![0.0; len];
        for j in 0..m {
            for i in 0..n {
                packed[j * n + i] = a.get(j, i) / scale;
            }
        }
        let Svd { d, u, v } = tall(packed, n, m, nv, nu, scale)?;
        Ok(Svd { d, u: v, v: u })
    }
}

fn tall(
    mut a: Vec<f64>,
    m: usize,
    n: usize,
    nu: usize,
    nv: usize,
    scale: f64,
) -> Result<Svd, MathError> {
    let (mut d, mut e) = (vec![0.0; n], vec![0.0; n]);
    let (mut left, mut right) = (vec![0.0; n], vec![0.0; n]);
    let mut row = vec![0.0; n];
    for k in 0..n {
        let (packed, remaining) = a.split_at_mut((k + 1) * m);
        let x = &mut packed[k * (m + 1)..];
        let (alpha, tail) = x.split_at_mut(1);
        left[k] = dlarfg(&mut alpha[0], tail);
        d[k] = alpha[0];
        for col in remaining.chunks_exact_mut(m) {
            reflect(left[k], tail, &mut col[k..]);
        }
        if k + 1 < n {
            let p = k + 1;
            for j in p..n {
                row[j] = a[j * m + k];
            }
            let (alpha, tail) = row[p..].split_at_mut(1);
            right[k] = dlarfg(&mut alpha[0], tail);
            e[k] = alpha[0];
            for i in p..m {
                let dot = (p..n)
                    .map(|j| a[j * m + i] * if j == p { 1.0 } else { row[j] })
                    .sum::<f64>()
                    * right[k];
                for j in p..n {
                    a[j * m + i] -= dot * if j == p { 1.0 } else { row[j] };
                }
            }
            for j in p + 1..n {
                a[j * m + k] = row[j];
            }
        }
    }

    let mut u = eye(m, if nu == 0 { 0 } else { nu.max(n) })?;
    let mut v = eye(n, if nv == 0 { 0 } else { n })?;
    bidiagonal_qr(&mut d, &mut e, &mut u, &mut v, m)?;
    for (k, value) in d.iter_mut().enumerate() {
        if value.is_sign_negative() {
            *value = -*value;
            if !v.is_empty() {
                for x in &mut v[k * n..(k + 1) * n] {
                    *x = -*x;
                }
            }
        }
    }

    let mut order: Vec<_> = (0..n).collect();
    order.sort_by(|&i, &j| d[j].total_cmp(&d[i]));
    let mut u = select(&u, m, nu, &order)?;
    let mut v = select(&v, n, nv, &order)?;
    for k in (0..n).rev() {
        let tail = &a[k * (m + 1) + 1..(k + 1) * m];
        for col in u.chunks_exact_mut(m) {
            reflect(left[k], tail, &mut col[k..]);
        }
        if k + 1 < n && !v.is_empty() {
            for j in k + 2..n {
                row[j] = a[j * m + k];
            }
            for col in v.chunks_exact_mut(n) {
                reflect(right[k], &row[k + 2..], &mut col[k + 1..]);
            }
        }
    }
    Ok(Svd {
        d: order.iter().map(|&i| d[i] * scale).collect(),
        u,
        v,
    })
}

fn select(
    matrix: &[f64],
    rows: usize,
    cols: usize,
    order: &[usize],
) -> Result<Vec<f64>, MathError> {
    let mut selected = Vec::with_capacity(area::<f64>(rows, cols)?);
    for j in 0..cols {
        let k = order.get(j).copied().unwrap_or(j);
        selected.extend_from_slice(&matrix[k * rows..(k + 1) * rows]);
    }
    Ok(selected)
}

fn bidiagonal_qr(
    d: &mut [f64],
    e: &mut [f64],
    u: &mut [f64],
    v: &mut [f64],
    rows: usize,
) -> Result<(), MathError> {
    let n = d.len();
    let mut end = n;
    let mut budget = n.saturating_mul(128);
    while end > 1 {
        let last = end - 1;
        let mut start = last;
        while start > 0 {
            let k = start - 1;
            if e[k].abs() <= f64::EPSILON * (d[k].abs() + d[start].abs()) {
                e[k] = 0.0;
                break;
            }
            start -= 1;
        }
        if start == last {
            end -= 1;
            continue;
        }
        if budget == 0 {
            return Err(MathError::NoConvergence { info: 1 });
        }
        budget -= 1;
        if d[last] == 0.0 {
            let mut f = e[last - 1];
            e[last - 1] = 0.0;
            for j in (start..last).rev() {
                let (c, s, r) = givens(d[j], f);
                d[j] = r;
                rotate(v, n, j, last, c, s);
                if j > start {
                    f = -s * e[j - 1];
                    e[j - 1] *= c;
                }
            }
        } else if let Some(k) = (start..last).rev().find(|&k| d[k] == 0.0) {
            let mut f = e[k];
            e[k] = 0.0;
            for j in k + 1..end {
                let (c, s, r) = givens(d[j], f);
                d[j] = r;
                f = -s * e[j];
                e[j] *= c;
                rotate(u, rows, j, k, c, s);
            }
        } else {
            // Shift from the trailing 2-by-2 block of B B^T. Normalize the
            // local terms before forming products, and suppress small shifts.
            let p = last - 1;
            let size = [d[start], e[start], d[p], e[p], d[last]]
                .into_iter()
                .map(f64::abs)
                .fold(0.0, f64::max);
            let (a, b, c) = (d[p] / size, e[p] / size, d[last] / size);
            let delta = ((a - c) * (a + c) + b * b) / 2.0;
            let off = b * c;
            let shift = if off == 0.0 {
                c * c
            } else {
                c * c - off * (off / (delta + delta.hypot(off).copysign(delta)))
            };
            let x = d[start] / size;
            let y = e[start] / size;
            let min = d[start..end].iter().map(|x| x.abs()).fold(size, f64::min);
            let (mut f, mut g) = if min / size <= f64::EPSILON.sqrt() {
                (d[start], e[start])
            } else {
                (x * x - shift, x * y)
            };
            for j in start..last {
                let (c, s, r) = givens(f, g);
                if j > start {
                    e[j - 1] = r;
                }
                f = c * d[j] + s * e[j];
                e[j] = c * e[j] - s * d[j];
                g = s * d[j + 1];
                d[j + 1] *= c;
                rotate(v, n, j, j + 1, c, s);
                let (c, s, r) = givens(f, g);
                d[j] = r;
                f = c * e[j] + s * d[j + 1];
                d[j + 1] = c * d[j + 1] - s * e[j];
                g = s * e[j + 1];
                e[j + 1] *= c;
                rotate(u, rows, j, j + 1, c, s);
            }
            e[last - 1] = f;
        }
    }
    Ok(())
}

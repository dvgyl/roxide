//! Householder tridiagonal reduction and implicit shifted QL.

use crate::{MatRef, MathError, linalg::factor::dlarfg};

use super::{area, eye, givens, reflect_right, rotate, scale};

/// Spectral decomposition of a real symmetric matrix.
#[derive(Debug)]
pub struct SymmetricEigen {
    /// Eigenvalues in decreasing order.
    pub values: Vec<f64>,
    /// Column-major eigenvectors, or an empty vector for a values-only call.
    pub vectors: Vec<f64>,
}

/// Diagonalize a nonempty square matrix using its lower triangle.
pub fn symmetric_eigen(a: MatRef<'_>, only_values: bool) -> Result<SymmetricEigen, MathError> {
    let n = a.nrow;
    MatRef::new(a.data, n, a.ncol)?;
    if n == 0 || n != a.ncol {
        return Err(MathError::Dim);
    }
    let scale = scale((0..n).flat_map(|j| a.col(j)[j..].iter().copied()))?;
    let mut work = vec![0.0; area::<f64>(n, n)?];
    for j in 0..n {
        for i in j..n {
            work[j * n + i] = a.get(i, j) / scale;
        }
    }
    let mut vectors = eye(n, if only_values { 0 } else { n })?;
    let mut e = vec![0.0; n];
    let mut w = vec![0.0; n];
    for (j, edge) in e.iter_mut().enumerate().take(n - 1) {
        let p = j + 1;
        let (old, trailing) = work.split_at_mut(p * n);
        let (alpha, tail) = old[j * n + p..].split_at_mut(1);
        let tau = dlarfg(&mut alpha[0], tail);
        *edge = alpha[0];
        if tau == 0.0 {
            continue;
        }

        // A <- A - v w^T - w v^T, where w = tau*A*v - (tau^2/2)*(v^T*A*v)*v.
        let v = |i| if i == p { 1.0 } else { tail[i - p - 1] };
        w[p..].fill(0.0);
        for k in p..n {
            let vk = v(k);
            for i in k..n {
                let a = trailing[(k - p) * n + i];
                w[i] += a * vk;
                if i != k {
                    w[k] += a * v(i);
                }
            }
        }
        let correction = (tau / 2.0) * (p..n).map(|i| v(i) * w[i]).sum::<f64>();
        for (i, w) in w.iter_mut().enumerate().skip(p) {
            *w = tau * (*w - correction * v(i));
        }
        for k in p..n {
            for i in k..n {
                trailing[(k - p) * n + i] -= v(i) * w[k] + w[i] * v(k);
            }
        }
        reflect_right(&mut vectors, n, n, p, tau, tail);
    }
    let mut d: Vec<_> = (0..n).map(|i| work[i * (n + 1)]).collect();
    ql(&mut d, &mut e, &mut vectors)?;
    let mut order: Vec<_> = (0..n).collect();
    order.sort_by(|&i, &j| d[j].total_cmp(&d[i]));
    let vectors = if only_values {
        vectors
    } else {
        order
            .iter()
            .flat_map(|&j| vectors[j * n..(j + 1) * n].iter().copied())
            .collect()
    };
    Ok(SymmetricEigen {
        values: order.iter().map(|&i| d[i] * scale).collect(),
        vectors,
    })
}

fn ql(d: &mut [f64], e: &mut [f64], vectors: &mut [f64]) -> Result<(), MathError> {
    let n = d.len();
    for start in 0..n {
        for iteration in 0..=128 {
            let mut end = start;
            while end + 1 < n && e[end].abs() > f64::EPSILON * (d[end].abs() + d[end + 1].abs()) {
                end += 1;
            }
            e[end] = 0.0;
            if end == start {
                break;
            }
            if iteration == 128 {
                return Err(MathError::NoConvergence { info: 1 });
            }

            let delta = (d[start + 1] - d[start]) / 2.0;
            let edge = e[start];
            let shift = edge * (edge / (delta + delta.hypot(edge).copysign(delta)));
            let mut g = d[end] - d[start] + shift;
            let (mut s, mut c, mut p) = (1.0, 1.0, 0.0);
            for i in (start..end).rev() {
                let f = s * e[i];
                let b = c * e[i];
                (c, s, e[i + 1]) = givens(g, f);
                g = d[i + 1] - p;
                let r = (d[i] - g) * s + 2.0 * c * b;
                p = s * r;
                d[i + 1] = g + p;
                g = c * r - b;
                rotate(vectors, n, i + 1, i, c, s);
            }
            d[start] -= p;
            e[start] = g;
            e[end] = 0.0;
        }
    }
    Ok(())
}

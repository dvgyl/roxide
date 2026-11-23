use crate::{Cplx, MathError};

use super::{ONE, Reflector, ZERO, euclidean, scaled_matrix};

/// Eigenvalues and right eigenvectors of a general complex matrix.
pub struct ComplexEigen {
    /// Eigenvalues sorted by decreasing modulus.
    pub values: Vec<Cplx>,
    /// Columns of unit eigenvectors, or an empty vector for values only.
    pub vectors: Vec<Cplx>,
}

/// Balance, reduce to Hessenberg form, and use shifted QR to find the
/// complex Schur form. Compute right eigenvectors by back substitution.
pub fn complex_general_eigen(
    data: &[Cplx],
    n: usize,
    only_values: bool,
) -> Result<ComplexEigen, MathError> {
    let (mut a, scale) = scaled_matrix(data, n, n)?;
    let balance = balance(&mut a, n);
    let mut q = if only_values {
        Vec::new()
    } else {
        vec![ZERO; n * n]
    };
    if !only_values {
        for i in 0..n {
            q[i * (n + 1)] = ONE;
        }
    }
    for k in 0..n.saturating_sub(2) {
        if let Some(h) = Reflector::new(k + 1, a[k * n + k + 1..(k + 1) * n].to_vec()) {
            h.left(&mut a[k * n..], n);
            h.right(&mut a, n);
            h.right(&mut q, n);
            a[k * n + k + 2..(k + 1) * n].fill(ZERO);
        }
    }
    schur(&mut a, &mut q, n)?;
    let mut order: Vec<_> = (0..n).collect();
    order.sort_by(|&i, &j| {
        a[j * (n + 1)]
            .modulus()
            .total_cmp(&a[i * (n + 1)].modulus())
    });
    let values = order
        .iter()
        .map(|&i| a[i * (n + 1)].scaled(scale))
        .collect();
    let mut vectors = if only_values {
        Vec::new()
    } else {
        vec![ZERO; n * n]
    };
    for (&k, col) in order.iter().zip(vectors.chunks_exact_mut(n)) {
        eigenvector(&a, &q, &balance, n, k, col);
    }
    Ok(ComplexEigen { values, vectors })
}

fn balance(a: &mut [Cplx], n: usize) -> Vec<f64> {
    let mut scales = vec![1.0; n];
    for _ in 0..32 {
        let mut changed = false;
        for i in 0..n {
            let (mut col, mut row) = (0.0, 0.0);
            for j in (0..n).filter(|&j| j != i) {
                col += a[i * n + j].modulus();
                row += a[j * n + i].modulus();
            }
            if col == 0.0 || row == 0.0 {
                continue;
            }
            let shift = ((row.log2() - col.log2()) * 0.5).round().clamp(-32.0, 32.0);
            let f = 2.0_f64.powi(shift as i32);
            let scale = scales[i] * f;
            if !(2.0_f64.powi(-256)..=2.0_f64.powi(256)).contains(&scale)
                || col * f + row / f >= 0.95 * (col + row)
            {
                continue;
            }
            scales[i] = scale;
            changed = true;
            for j in (0..n).filter(|&j| j != i) {
                a[i * n + j] = a[i * n + j].scaled(f);
                a[j * n + i] = a[j * n + i].divided_by(f);
            }
        }
        if !changed {
            break;
        }
    }
    scales
}

fn schur(a: &mut [Cplx], q: &mut [Cplx], n: usize) -> Result<(), MathError> {
    let mut hi = n - 1;
    let mut steps = 0;
    while hi > 0 {
        let mut lo = hi;
        while lo > 0 {
            let i = lo;
            let adjacent = a[i * (n + 1)].modulus() + a[(i - 1) * (n + 1)].modulus();
            let below = a[(i - 1) * n + i].modulus();
            let above = a[i * n + i - 1].modulus();
            if below == 0.0
                || (below <= f64::EPSILON * adjacent
                    && below * above <= f64::EPSILON * adjacent * adjacent)
            {
                a[(i - 1) * n + i] = ZERO;
                break;
            }
            lo -= 1;
        }
        if lo == hi {
            hi -= 1;
            steps = 0;
            continue;
        }
        steps += 1;
        if steps > 120 {
            return Err(MathError::NoConvergence {
                info: i32::try_from(hi + 1).unwrap_or(i32::MAX),
            });
        }
        let mut shift = wilkinson(a, n, hi);
        if steps % 12 == 0 {
            shift = a[hi * (n + 1)].plus(Cplx::new(0.75 * a[(hi - 1) * n + hi].modulus(), 0.0));
        }
        for i in lo..hi {
            let col = if i == lo {
                vec![a[i * (n + 1)].minus(shift), a[i * n + i + 1]]
            } else {
                vec![a[(i - 1) * n + i], a[(i - 1) * n + i + 1]]
            };
            if let Some(h) = Reflector::new(i, col) {
                h.left(&mut a[i.saturating_sub(1) * n..], n);
                h.right(a, n);
                h.right(q, n);
                if i > lo {
                    a[(i - 1) * n + i + 1] = ZERO;
                }
            }
        }
    }
    Ok(())
}

fn wilkinson(a: &[Cplx], n: usize, hi: usize) -> Cplx {
    let d = a[hi * (n + 1)];
    let delta = a[(hi - 1) * (n + 1)].minus(d).scaled(0.5);
    let product = a[hi * n + hi - 1].times(a[(hi - 1) * n + hi]);
    let z = delta.times(delta).plus(product);
    let real = ((z.modulus() + z.re.abs()) * 0.5).sqrt();
    let square_root = if real == 0.0 {
        ZERO
    } else if z.re >= 0.0 {
        Cplx::new(real, z.im / (2.0 * real))
    } else {
        Cplx::new(z.im.abs() / (2.0 * real), real.copysign(z.im))
    };
    let plus = delta.plus(square_root);
    let minus = delta.minus(square_root);
    let den = if plus.modulus() >= minus.modulus() {
        plus
    } else {
        minus
    };
    if den == ZERO {
        d
    } else {
        d.minus(product.quotient(den))
    }
}

fn eigenvector(a: &[Cplx], q: &[Cplx], balance: &[f64], n: usize, k: usize, col: &mut [Cplx]) {
    let lambda = a[k * (n + 1)];
    let minimum = (f64::EPSILON * lambda.modulus()).max(f64::MIN_POSITIVE);
    let mut x = vec![ZERO; k + 1];
    x[k] = ONE;
    for i in (0..k).rev() {
        let mut rhs = ZERO;
        for j in i + 1..=k {
            rhs = rhs.minus(a[j * n + i].times(x[j]));
        }
        let mut den = a[i * (n + 1)].minus(lambda);
        if den.modulus() < minimum {
            den = den.phase().scaled(minimum);
        }
        let size = rhs.modulus();
        let threshold = 1e100 * den.modulus();
        if size > threshold {
            let scale = threshold / size;
            for z in &mut x[i + 1..] {
                *z = z.scaled(scale);
            }
            rhs = rhs.scaled(scale);
        }
        x[i] = rhs.quotient(den);
    }
    for i in 0..n {
        col[i] = x
            .iter()
            .enumerate()
            .fold(ZERO, |s, (j, &v)| s.plus(q[j * n + i].times(v)))
            .scaled(balance[i]);
    }
    let norm = euclidean(col);
    if let Some((i, &largest)) = col
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.modulus().total_cmp(&b.modulus()))
    {
        let phase = largest.phase().conjugate();
        for z in col.iter_mut() {
            *z = z.divided_by(norm).times(phase);
        }
        col[i] = Cplx::new(largest.modulus() / norm, 0.0);
    }
}

use crate::{
    Cplx, MatRef, MathError,
    linalg::spectral::{svd, symmetric_eigen},
};

use super::{
    ONE, Reflector, ZERO, adjoint, area, basis, check_matrix, diagonal, lift, scaled_matrix,
};

/// Complex singular values and vectors.
pub struct ComplexSvd {
    /// Real singular values in descending order.
    pub d: Vec<f64>,
    /// Requested columns of the left singular vectors.
    pub u: Vec<Cplx>,
    /// Conjugate transpose of the requested right singular vectors.
    pub vt: Vec<Cplx>,
}

/// Factor a matrix with unitary bidiagonal reduction and real bidiagonal QR.
///
/// The matrix and all results use column-major order.
pub fn complex_svd(
    data: &[Cplx],
    rows: usize,
    cols: usize,
    nu: usize,
    nv: usize,
) -> Result<ComplexSvd, MathError> {
    check_matrix(data, rows, cols)?;
    let (nu, nv) = (nu.min(rows), nv.min(cols));
    if diagonal(data, rows) {
        let mut order: Vec<_> = (0..rows.min(cols)).collect();
        order.sort_by(|&i, &j| {
            data[j * (rows + 1)]
                .modulus()
                .total_cmp(&data[i * (rows + 1)].modulus())
        });
        let mut u = basis(rows, nu, &order)?;
        for (j, &i) in order.iter().take(nu).enumerate() {
            u[j * rows + i] = data[i * (rows + 1)].phase();
        }
        return Ok(ComplexSvd {
            d: order
                .iter()
                .map(|&i| data[i * (rows + 1)].modulus())
                .collect(),
            u,
            vt: adjoint(&basis(cols, nv, &order)?, cols, nv),
        });
    }
    let (a, scale) = scaled_matrix(data, rows, cols)?;
    let (mut d, u, v) = if rows >= cols {
        tall(a, rows, cols, nu, nv)?
    } else {
        let (d, v, u) = tall(adjoint(&a, rows, cols), cols, rows, nv, nu)?;
        (d, u, v)
    };
    for value in &mut d {
        *value *= scale;
    }
    Ok(ComplexSvd {
        d,
        u,
        vt: adjoint(&v, cols, nv),
    })
}

type TallFactors = (Vec<f64>, Vec<Cplx>, Vec<Cplx>);

fn tall(
    mut a: Vec<Cplx>,
    m: usize,
    n: usize,
    nu: usize,
    nv: usize,
) -> Result<TallFactors, MathError> {
    area(m, nu)?;
    area(n, nv)?;
    let mut left = Vec::new();
    let mut right = Vec::new();
    for k in 0..n {
        let col = a[k * m + k..(k + 1) * m].to_vec();
        if let Some(h) = Reflector::new(k, col) {
            h.left(&mut a[k * m..], m);
            left.push(h);
        }
        if k + 1 < n {
            let row = (k + 1..n).map(|j| a[j * m + k].conjugate()).collect();
            if let Some(h) = Reflector::new(k + 1, row) {
                h.right(&mut a, m);
                right.push(h);
            }
        }
    }
    let mut b = vec![0.0; m * n];
    let mut u_phase = vec![ONE; m];
    let mut v_phase = vec![ONE; n];
    for k in 0..n {
        let diag = a[k * m + k];
        u_phase[k] = diag.phase().times(v_phase[k]);
        b[k * m + k] = diag.modulus();
        if k + 1 < n {
            let upper = a[(k + 1) * m + k];
            v_phase[k + 1] = upper.phase().conjugate().times(u_phase[k]);
            b[(k + 1) * m + k] = upper.modulus();
        }
    }
    let s = svd(MatRef::new(&b, m, n)?, nu, nv)?;
    Ok((s.d, lift(s.u, &u_phase, &left), lift(s.v, &v_phase, &right)))
}

/// Eigenvalues and eigenvectors of a complex Hermitian matrix.
pub struct HermitianEigen {
    /// Real eigenvalues in descending order.
    pub values: Vec<f64>,
    /// Columns of unit eigenvectors, or an empty vector for values only.
    pub vectors: Vec<Cplx>,
}

/// Reduce the lower triangle to a real symmetric tridiagonal matrix.
/// The diagonal uses only its real part.
pub fn hermitian_eigen(
    data: &[Cplx],
    n: usize,
    only_values: bool,
) -> Result<HermitianEigen, MathError> {
    if n == 0 || data.len() != area(n, n)? {
        return Err(MathError::Dim);
    }
    let mut triangle = data.to_vec();
    for j in 0..n {
        triangle[j * n + j].im = 0.0;
        for i in j + 1..n {
            triangle[i * n + j] = triangle[j * n + i].conjugate();
        }
    }
    check_matrix(&triangle, n, n)?;
    if diagonal(&triangle, n) {
        let mut order: Vec<_> = (0..n).collect();
        order.sort_by(|&i, &j| {
            triangle[j * (n + 1)]
                .re
                .total_cmp(&triangle[i * (n + 1)].re)
        });
        return Ok(HermitianEigen {
            values: order.iter().map(|&i| triangle[i * (n + 1)].re).collect(),
            vectors: basis(n, if only_values { 0 } else { n }, &order)?,
        });
    }
    let (mut a, scale) = scaled_matrix(&triangle, n, n)?;
    let mut reflectors = Vec::new();
    for k in 0..n.saturating_sub(2) {
        if let Some(h) = Reflector::new(k + 1, a[k * n + k + 1..(k + 1) * n].to_vec()) {
            h.left(&mut a[k * n..], n);
            h.right(&mut a, n);
            a[k * n + k + 2..(k + 1) * n].fill(ZERO);
            for i in k + 2..n {
                a[i * n + k] = ZERO;
            }
            if !only_values {
                reflectors.push(h);
            }
        }
    }
    let mut t = vec![0.0; n * n];
    let mut phases = vec![ONE; n];
    for k in 0..n {
        t[k * n + k] = a[k * n + k].re;
        if k + 1 < n {
            let off = a[k * n + k + 1];
            phases[k + 1] = phases[k].times(off.phase());
            t[k * n + k + 1] = off.modulus();
            t[(k + 1) * n + k] = off.modulus();
        }
    }
    let e = symmetric_eigen(MatRef::new(&t, n, n)?, only_values)?;
    Ok(HermitianEigen {
        values: e.values.into_iter().map(|d| d * scale).collect(),
        vectors: lift(e.vectors, &phases, &reflectors),
    })
}

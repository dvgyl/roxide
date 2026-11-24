//! Tests of LINPACK storage, job flags, and numerical results.

use roxide_math::linalg::linpack_capi::*;

const PAD: f64 = -987.0;

fn close(actual: f64, expected: f64, tol: f64) {
    assert!(
        (actual - expected).abs() <= tol,
        "{actual} vs {expected} (tolerance {tol})"
    );
}

fn close_slice(actual: &[f64], expected: &[f64], tol: f64) {
    assert_eq!(actual.len(), expected.len());
    for (&a, &b) in actual.iter().zip(expected) {
        close(a, b, tol);
    }
}

fn padded(rows: usize, ld: usize, dense: &[f64]) -> Vec<f64> {
    let mut result = Vec::new();
    for col in dense.chunks_exact(rows) {
        result.extend_from_slice(col);
        result.resize(result.len() + ld - rows, PAD);
    }
    result
}

fn padding(matrix: &[f64], rows: usize, ld: usize) {
    for col in matrix.chunks(ld) {
        assert!(col[rows..].iter().all(|&v| v == PAD));
    }
}

fn dense_spd() -> Vec<f64> {
    padded(3, 5, &[4.0, PAD, PAD, 2.0, 5.0, PAD, -2.0, 1.0, 6.0])
}

#[test]
fn dense_cholesky_factor_solve_and_failed_minor() {
    let mut a = dense_spd();
    assert_eq!(dpofa(&mut a[..13], 5, 3), 0);
    let expected = padded(3, 5, &[2.0, PAD, PAD, 1.0, 2.0, PAD, -1.0, 1.0, 2.0]);
    assert_eq!(a, expected);
    let mut b = [-6.0, -11.0, 7.0, PAD];
    dposl(&a[..13], 5, 3, &mut b);
    close_slice(&b, &[1.0, -3.0, 2.0, PAD], 1e-14);

    let mut bad = dense_spd();
    bad[6] = 0.5;
    assert_eq!(dpofa(&mut bad, 5, 3), 2);
    assert_eq!(bad[6], 0.5);
    assert_eq!(&bad[10..], &dense_spd()[10..]);
    padding(&bad, 3, 5);
}

#[test]
fn band_factor_and_solve_preserve_unused_rows() {
    let mut band = padded(2, 4, &[PAD, 4.0, 2.0, 5.0, 2.0, 5.0, 2.0, 5.0]);
    assert_eq!(dpbfa(&mut band, 4, 4, 1), 0);
    assert_eq!(
        band,
        padded(2, 4, &[PAD, 2.0, 1.0, 2.0, 1.0, 2.0, 1.0, 2.0])
    );
    let mut rhs = [8.0, 18.0, 27.0, 26.0, PAD];
    dpbsl(&band, 4, 4, 1, &mut rhs);
    close_slice(&rhs, &[1.0, 2.0, 3.0, 4.0, PAD], 1e-14);

    let mut bad = padded(2, 4, &[PAD, 4.0, 2.0, 1.0, 2.0, 5.0]);
    assert_eq!(dpbfa(&mut bad, 4, 3, 1), 2);
    assert_eq!(&bad[8..], &[2.0, 5.0, PAD, PAD]);
    assert_eq!(bad[0], PAD);
    padding(&bad, 2, 4);
}

#[test]
fn band_factor_with_two_superdiagonals() {
    let mut band = padded(
        3,
        4,
        &[PAD, PAD, 4.0, PAD, 2.0, 5.0, -2.0, 1.0, 6.0, 1.0, 3.0, 4.5],
    );
    assert_eq!(dpbfa(&mut band, 4, 4, 2), 0);
    let height = 2.6875_f64.sqrt();
    let expected = [
        PAD, PAD, 2.0, PAD, 1.0, 2.0, -1.0, 1.0, 2.0, 0.5, 1.25, height,
    ];
    close_slice(&band, &padded(3, 4, &expected), 1e-14);
    let mut rhs = [2.0, 19.0, 30.0, 29.0];
    dpbsl(&band, 4, 4, 2, &mut rhs);
    close_slice(&rhs, &[1.0, 2.0, 3.0, 4.0], 1e-14);
}

#[test]
fn cholesky_inverse_and_determinant_jobs() {
    let mut factor = dense_spd();
    dpofa(&mut factor, 5, 3);
    for job in [0, 1, 10, 11] {
        let mut a = factor.clone();
        let mut det = [PAD; 2];
        dpodi(&mut a, 5, 3, &mut det, job);
        if job >= 10 {
            close_slice(&det, &[6.4, 1.0], 1e-14);
        } else {
            assert_eq!(det, [PAD; 2]);
        }
        if job % 10 == 0 {
            assert_eq!(a, factor);
            continue;
        }
        let original = dense_spd();
        for j in 0_usize..3 {
            for i in 0_usize..3 {
                let value = (0..3)
                    .map(|k| original[i.max(k) * 5 + i.min(k)] * a[j.max(k) * 5 + j.min(k)])
                    .sum::<f64>();
                close(value, f64::from(i == j), 1e-14);
            }
            assert!(a[j * 5 + j + 1..j * 5 + 3].iter().all(|&v| v == PAD));
        }
        padding(&a, 3, 5);
    }
}

#[test]
fn determinant_scales_each_factor_before_squaring() {
    for (factor, expected) in [
        ([2e200, 3e200], [3.6, 801.0]),
        ([2e-200, 3e-200], [3.6, -799.0]),
        ([2e200, 3e-200], [3.6, 1.0]),
        ([0.0, 1e200], [0.0, 0.0]),
    ] {
        let mut a = [factor[0], PAD, 0.0, factor[1]];
        let mut det = [PAD; 2];
        dpodi(&mut a, 2, 2, &mut det, 10);
        close_slice(&det, &expected, 1e-12);
    }
}

#[test]
fn positive_definite_condition_estimate_and_vector() {
    let mut z = [PAD; 4];
    for (diag, expected) in [
        ([2.0, 2.0, 2.0], 1.0),
        ([2.0, 2e-14, 2.0], 1e-14),
        ([2e-280, 2e-280, 2e-280], 1.0),
    ] {
        let mut a = padded(
            3,
            5,
            &[diag[0], PAD, PAD, 0.0, diag[1], PAD, 0.0, 0.0, diag[2]],
        );
        let (rcond, info) = dpoco(&mut a, 5, 3, &mut z);
        assert_eq!(info, 0);
        close(rcond / expected, 1.0, 1e-12);
        close(z[..3].iter().map(|v| v.abs()).sum(), 1.0, 1e-14);
        assert_eq!(z[3], PAD);
        padding(&a, 3, 5);
    }
    let mut a = dense_spd();
    let (rcond, info) = dpoco(&mut a, 5, 3, &mut z);
    assert_eq!(info, 0);
    let exact = 1.0 / (9.0 * 0.859375);
    assert!((exact - 1e-14..2.0 * exact).contains(&rcond));
    assert_eq!(
        a,
        padded(3, 5, &[2.0, PAD, PAD, 1.0, 2.0, PAD, -1.0, 1.0, 2.0])
    );

    let mut bad = dense_spd();
    bad[6] = 0.5;
    assert_eq!(dpoco(&mut bad, 5, 3, &mut z), (0.0, 2));
}

#[test]
fn triangular_solves_use_only_the_requested_triangle() {
    let upper = padded(3, 5, &[2.0, PAD, PAD, 3.0, 4.0, PAD, -1.0, 2.0, 5.0]);
    let lower = padded(3, 5, &[2.0, 3.0, -1.0, PAD, 4.0, 2.0, PAD, PAD, 5.0]);
    for (job, matrix, rhs) in [
        (0, &lower, [2.0, 11.0, 18.0]),
        (1, &upper, [5.0, 14.0, 15.0]),
        (10, &lower, [5.0, 14.0, 15.0]),
        (11, &upper, [2.0, 11.0, 18.0]),
    ] {
        let mut b = [rhs[0], rhs[1], rhs[2], PAD];
        assert_eq!(dtrsl(matrix, 5, 3, &mut b, job), 0);
        close_slice(&b, &[1.0, 2.0, 3.0, PAD], 1e-14);

        let mut singular = matrix.clone();
        singular[6] = 0.0;
        let mut b = rhs;
        assert_eq!(dtrsl(&singular, 5, 3, &mut b, job), 2);
        assert_eq!(b, rhs);
    }
}

#[test]
fn triangular_condition_estimate_and_null_vector() {
    let mut z = [PAD; 3];
    for job in [0, 1] {
        let (i, j) = if job == 0 { (1, 4) } else { (4, 1) };
        for (diag, off, expected) in [
            (2.0, 0.0, 1.0),
            (2e-15, 0.0, 1e-15),
            (0.0, 1.0, 0.0),
            (2.0, 1e8, 4e-16),
        ] {
            let mut t = [2.0, PAD, PAD, PAD, PAD, diag, PAD, PAD];
            t[i] = off;
            t[j] = f64::NAN;
            let rcond = dtrco(&t, 4, 2, &mut z, job);
            close(rcond, expected, 1e-7 * expected);
            close(z[..2].iter().map(|v| v.abs()).sum(), 1.0, 1e-14);
            assert_eq!(z[2], PAD);
            if expected == 0.0 && job == 0 {
                close_slice(&[2.0 * z[0], z[0]], &[0.0, 0.0], 0.0);
            } else if expected == 0.0 {
                close(2.0 * z[0] + z[1], 0.0, 1e-14);
            }
        }
    }
}

#[test]
fn qr_pivots_initial_free_and_final_columns() {
    let n = 5;
    let p = 6;
    let ld = 7;
    let original = padded(
        n,
        ld,
        &[
            1.0, 2.0, 0.0, 0.0, 0.0, 90.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 30.0,
            3.0, 4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 99.0, 0.0, 0.0, 2.0, 0.0, 0.0,
        ],
    );
    let mut x = original.clone();
    let mut aux = [PAD; 7];
    let mut work = [PAD; 7];
    let mut pivot = [0, -1, 7, 0, -5, 2, 99];
    dqrdc(&mut x, ld, n, p, &mut aux, &mut pivot, &mut work, 1);
    assert_eq!(pivot, [3, 6, 4, 1, 5, 2, 99]);
    padding(&x, n, ld);
    assert_eq!(aux[p], PAD);
    assert_eq!(work[p], PAD);
    for (j, &index) in pivot[..p].iter().enumerate() {
        let mut r = [0.0; 5];
        let count = n.min(j + 1);
        r[..count].copy_from_slice(&x[j * ld..j * ld + count]);
        let mut col = [0.0; 5];
        assert_eq!(
            dqrsl(
                &x,
                ld,
                n,
                n,
                &aux,
                &r,
                Some(&mut col),
                None,
                None,
                None,
                None,
                10000
            ),
            0
        );
        let start = (index as usize - 1) * ld;
        close_slice(&col, &original[start..start + n], 1e-12);
    }
}

#[test]
fn qr_outputs_and_implied_transpose_job() {
    let mut x = padded(3, 5, &[0.0, 2.0, 0.0, 3.0, 1.0, 0.0]);
    let mut aux = [PAD; 2];
    let mut pivot = [7, -3];
    let mut work = [PAD; 2];
    dqrdc(&mut x, 5, 3, 2, &mut aux, &mut pivot, &mut work, 0);
    assert_eq!(pivot, [7, -3]);
    assert_eq!(work, [PAD; 2]);
    padding(&x, 3, 5);

    let y = [6.0, 4.0, 5.0];
    for job in [10000, 1000, 100, 10, 1, 11111] {
        let (mut qy, mut qty, mut b, mut rsd, mut xb) =
            ([PAD; 4], [PAD; 4], [PAD; 3], [PAD; 4], [PAD; 4]);
        assert_eq!(
            dqrsl(
                &x,
                5,
                3,
                2,
                &aux,
                &y,
                Some(&mut qy),
                Some(&mut qty),
                Some(&mut b),
                Some(&mut rsd),
                Some(&mut xb),
                job
            ),
            0
        );
        for (requested, actual, expected) in [
            (job >= 10000, &qy[..], &[4.0, -6.0, 5.0, PAD][..]),
            (job % 10000 != 0, &qty, &[-4.0, 6.0, 5.0, PAD]),
            ((job / 100) % 10 != 0, &b, &[1.0, 2.0, PAD]),
            ((job / 10) % 10 != 0, &rsd, &[0.0, 0.0, 5.0, PAD]),
            (job % 10 != 0, &xb, &[6.0, 4.0, 0.0, PAD]),
        ] {
            if requested {
                close_slice(actual, expected, 1e-14);
            } else {
                assert!(actual.iter().all(|&v| v == PAD));
            }
        }
    }
    let mut rhs = y;
    let snapshot = rhs;
    assert_eq!(
        dqrsl(
            &x,
            5,
            3,
            2,
            &aux,
            &snapshot,
            None,
            None,
            None,
            Some(&mut rhs),
            None,
            10
        ),
        0
    );
    assert_eq!(rhs, [0.0, 0.0, 5.0]);
}

#[test]
fn qr_singular_and_zero_column_cases() {
    let x = [1.0, PAD, PAD, PAD, 0.0, PAD, PAD, PAD, 0.0];
    let mut b = [PAD; 3];
    let y = [1.0, 2.0, 3.0];
    let (mut qty, mut rsd, mut xb) = ([PAD; 3], [PAD; 3], [PAD; 3]);
    assert_eq!(
        dqrsl(
            &x,
            3,
            3,
            3,
            &[0.0; 3],
            &y,
            None,
            Some(&mut qty),
            Some(&mut b),
            None,
            None,
            100
        ),
        2
    );
    assert_eq!(b, [PAD; 3]);
    assert_eq!(qty, y);
    assert_eq!(
        dqrsl(
            &[],
            3,
            3,
            0,
            &[],
            &y,
            None,
            None,
            None,
            Some(&mut rsd),
            Some(&mut xb),
            11
        ),
        0
    );
    assert_eq!(rsd, y);
    assert_eq!(xb, [0.0; 3]);
}

fn orthogonal(matrix: &[f64], rows: usize, ld: usize, cols: usize) {
    for j in 0..cols {
        for i in 0..=j {
            let dot = (0..rows)
                .map(|k| matrix[i * ld + k] * matrix[j * ld + k])
                .sum();
            close(dot, f64::from(i == j), 1e-12);
        }
    }
}

fn svd_jobs(n: usize, p: usize, a: &[f64], scale: f64, expected: Option<&[f64]>) {
    let (ldx, ldu, ldv) = (n + 2, n + 3, p + 2);
    let rank = n.min(p);
    let order = (n + 1).min(p);
    let mut singular_values = vec![0.0; rank];
    for job in [0, 1, 10, 11, 21] {
        let (nu, nv) = (
            if job == 21 {
                n
            } else if job >= 10 {
                rank
            } else {
                0
            },
            if job % 10 != 0 { p } else { 0 },
        );
        let mut x = padded(n, ldx, a);
        let mut s = vec![PAD; order + 1];
        let mut e = vec![PAD; p + 1];
        let mut work = vec![PAD; n + 1];
        let mut u = vec![PAD; ldu * n];
        let mut v = vec![PAD; ldv * p];
        assert_eq!(
            dsvdc(
                &mut x,
                ldx,
                n,
                p,
                &mut s,
                &mut e,
                Some(&mut u),
                ldu,
                Some(&mut v),
                ldv,
                &mut work,
                job
            ),
            0,
            "{n}x{p}, job {job}"
        );
        assert!(s[..order].windows(2).all(|w| w[0] >= w[1] && w[1] >= 0.0));
        assert!(s[0] >= 0.0);
        if job == 0 {
            singular_values.copy_from_slice(&s[..rank]);
        } else {
            for (actual, expected) in s.iter().zip(&singular_values) {
                close(actual / scale, expected / scale, 1e-12);
            }
        }
        if let Some(expected) = expected {
            for (actual, expected) in s.iter().zip(expected) {
                close(actual / scale, *expected, 1e-12);
            }
        }
        assert_eq!(s[order], PAD);
        assert!(s[rank..order].iter().all(|&v| v == 0.0));
        assert!(e[..p].iter().all(|&v| v == 0.0));
        assert_eq!(e[p], PAD);
        assert_eq!(work[n], PAD);
        assert!(u[nu * ldu..].iter().all(|&v| v == PAD));
        assert!(v[nv * ldv..].iter().all(|&v| v == PAD));
        padding(&x, n, ldx);
        padding(&u, n, ldu);
        padding(&v, p, ldv);
        orthogonal(&u, n, ldu, nu);
        orthogonal(&v, p, ldv, nv);
        if nu != 0 && nv != 0 {
            for j in 0..p {
                for i in 0..n {
                    let value = (0..rank)
                        .map(|k| u[k * ldu + i] * (s[k] / scale) * v[k * ldv + j])
                        .sum();
                    close(value, a[j * n + i] / scale, 1e-12);
                }
            }
        }
    }
}

#[test]
fn svd_reconstructs_tall_wide_square_and_rank_deficient_matrices() {
    svd_jobs(
        4,
        3,
        &[
            1.0, 2.0, 4.0, -1.0, 3.0, -1.0, 2.0, 0.0, -2.0, 4.0, 1.0, 2.0,
        ],
        1.0,
        None,
    );
    svd_jobs(2, 4, &[1.0, 2.0, -3.0, 4.0, 0.0, 2.0, 5.0, 1.0], 1.0, None);
    svd_jobs(
        3,
        3,
        &[1.0, 2.0, 4.0, -1.0, 3.0, -1.0, 2.0, 0.0, -2.0],
        1.0,
        None,
    );
    svd_jobs(
        4,
        3,
        &[1.0, 2.0, 0.0, 0.0, 2.0, 4.0, 0.0, 0.0, -3.0, -6.0, 0.0, 0.0],
        1.0,
        Some(&[70.0_f64.sqrt(), 0.0, 0.0]),
    );
    svd_jobs(2, 3, &[0.0; 6], 1.0, Some(&[0.0, 0.0]));
    svd_jobs(1, 3, &[0.0, 3.0, 4.0], 1.0, Some(&[5.0]));
    svd_jobs(3, 1, &[0.0, 3.0, 4.0], 1.0, Some(&[5.0]));
}

#[test]
fn svd_handles_wide_ranges_and_internal_zero_pivots() {
    for scale in [1e-290, 1.0, 1e290] {
        let a = [0.0, 0.0, 0.0, 3.0 * scale, 0.0, 0.0, 0.0, 4.0 * scale, 0.0];
        svd_jobs(3, 3, &a, scale, Some(&[4.0, 3.0, 0.0]));
    }
    let a = [1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 4.0];
    svd_jobs(3, 3, &a, 1.0, Some(&[5.0, 5.0_f64.sqrt(), 0.0]));
}

#[test]
fn svd_preserves_a_small_singular_value() {
    let mut x = [1.0, 0.0, 1.0, 1e-200];
    let (mut s, mut e, mut work) = ([0.0; 2], [0.0; 2], [0.0; 2]);
    let info = dsvdc(
        &mut x, 2, 2, 2, &mut s, &mut e, None, 0, None, 0, &mut work, 0,
    );
    assert_eq!(info, 0);
    close(s[0], 2.0_f64.sqrt(), 1e-14);
    close(s[1] / 1e-200, 1.0 / 2.0_f64.sqrt(), 1e-14);
}

#[test]
fn svd_deterministic_rectangular_sweep() {
    let mut state = 1234_u64;
    for n in 1..=8 {
        for p in 1..=8 {
            let a = (0..n * p)
                .map(|_| {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    f64::from((state >> 57) as u8) / 64.0 - 1.0
                })
                .collect::<Vec<_>>();
            svd_jobs(n, p, &a, 1.0, None);
        }
    }
}

#[test]
fn invalid_dimensions_and_short_buffers_leave_outputs_unchanged() {
    let mut a = [PAD; 4];
    let mut b = [PAD; 2];
    let mut z = [PAD; 2];
    let mut det = [PAD; 2];
    assert_eq!(dpofa(&mut a, 1, 2), -1);
    dposl(&a, 2, 2, &mut b[..1]);
    assert_eq!(dpbfa(&mut a, 2, 2, usize::MAX), -1);
    dpbsl(&a, 2, 2, 2, &mut b);
    assert_eq!(dpoco(&mut a, 2, 2, &mut z[..1]), (0.0, -1));
    dpodi(&mut a, usize::MAX, 2, &mut det, 11);
    assert_eq!(dtrsl(&a, 2, 2, &mut b[..1], 0), -1);
    assert_eq!(dtrco(&a, 1, 2, &mut z, 0), 0.0);

    let mut aux = [PAD; 2];
    let mut pivot = [0, 0];
    let mut work = [PAD; 2];
    dqrdc(&mut a, 2, 2, 2, &mut aux[..1], &mut pivot, &mut work, 1);
    dqrdc(&mut a, usize::MAX, 0, 2, &mut aux, &mut pivot, &mut work, 1);
    let info = dqrsl(
        &a,
        2,
        2,
        3,
        &aux,
        &b,
        None,
        None,
        None,
        None,
        Some(&mut z),
        1,
    );
    assert_eq!(info, -1);

    let mut s = [PAD; 2];
    let mut e = [PAD; 2];
    let info = dsvdc(
        &mut a, 2, 2, 2, &mut s, &mut e, None, 0, None, 0, &mut work, 11,
    );
    assert_eq!(info, -1);
    let short_s = &mut s[..1];
    let info = dsvdc(
        &mut a, 2, 2, 2, short_s, &mut e, None, 0, None, 0, &mut work, 0,
    );
    assert_eq!(info, -1);
    assert_eq!(a, [PAD; 4]);
    for values in [b, z, det, aux, work, s, e] {
        assert_eq!(values, [PAD; 2]);
    }
    assert_eq!(pivot, [0, 0]);
}

//! Check spectral values, orthogonality, and reconstruction.

#![forbid(unsafe_code)]

use roxide_math::{
    MatRef,
    linalg::spectral::{
        MatrixNorm, general_eigen, matrix_norm, svd, symmetric_eigen, triangular_condition,
    },
};

fn close(x: f64, y: f64, tol: f64) {
    assert!((x - y).abs() <= tol, "{x} differs from {y}");
}

fn orthogonal(a: &[f64], rows: usize) {
    for (j, x) in a.chunks_exact(rows).enumerate() {
        for (i, y) in a.chunks_exact(rows).enumerate() {
            close(
                x.iter().zip(y).map(|(a, b)| a * b).sum(),
                f64::from(i == j),
                1e-13,
            );
        }
    }
}

fn check_svd(a: &[f64], m: usize, n: usize, magnitude: f64) -> Vec<f64> {
    let a = MatRef::new(a, m, n).expect("matrix shape");
    let e = svd(a, m, n).expect("SVD");
    assert_eq!(e.d, svd(a, 0, 0).expect("values only").d);
    assert_eq!(e.u.len(), m * m);
    assert_eq!(e.v.len(), n * n);
    assert!(e.d.windows(2).all(|x| x[0] >= x[1]));
    assert!(e.d.iter().all(|&x| x >= 0.0));
    orthogonal(&e.u, m);
    orthogonal(&e.v, n);
    for i in 0..m {
        for j in 0..n {
            let value =
                e.d.iter()
                    .enumerate()
                    .map(|(k, d)| e.u[k * m + i] * (d / magnitude) * e.v[k * n + j])
                    .sum();
            close(value, a.get(i, j) / magnitude, 1e-13);
        }
    }
    e.d
}

#[test]
fn singular_values_match_oracle_fixtures_and_reconstruct() {
    let b = [1., 4., 7., 2., 5., 8., 3., 6., 10.];
    let m = [1., 4., 7., 2., 2., 5., 8., 0., 3., 6., 10., 1.];
    let mt = [1., 2., 3., 4., 5., 6., 7., 8., 10., 2., 0., 1.];
    for scale in [1e-200, 1.0, 1e200] {
        for (a, rows, cols, d) in [
            (
                &b[..],
                3,
                3,
                [17.41250516680859, 0.8751613501104368, 0.1968665211174301],
            ),
            (
                &m[..],
                4,
                3,
                [17.48831889344151, 1.681288294994658, 0.5761700707347804],
            ),
            (
                &mt[..],
                3,
                4,
                [17.48831889344151, 1.681288294994658, 0.5761700707347804],
            ),
        ] {
            let scaled: Vec<_> = a.iter().map(|x| x * scale).collect();
            for (value, expected) in check_svd(&scaled, rows, cols, 10.0 * scale).iter().zip(d) {
                close(value / scale, expected, 5e-14);
            }
        }
    }
}

#[test]
fn singular_rank_and_rectangular_edges() {
    assert_eq!(check_svd(&[-5.], 1, 1, 5.0), [5.0]);
    assert_eq!(check_svd(&[0.; 6], 2, 3, 1.0), [0.0; 2]);
    assert_eq!(check_svd(&[1., 0., 1., 0., 0., 0.], 2, 3, 1.0)[1], 0.0);
    check_svd(&[0., 0., 0., 0., 2., 0., 0., 3., 4.], 3, 3, 4.0);
    check_svd(&[0., 0., 0., 1., 0., 0., 2., 0., 3.], 3, 3, 3.0);
    let a = MatRef::new(&[1., 2., 3., 4., 5., 6.], 2, 3).expect("wide matrix");
    let e = svd(a, 0, 1).expect("one right vector");
    assert!(e.u.is_empty());
    assert_eq!(e.v.len(), 3);
    let e = svd(a, 1, 0).expect("one left vector");
    assert!(e.v.is_empty());
    assert_eq!(e.u.len(), 2);
    assert!(svd(a, 3, 1).is_err());
}

#[test]
fn symmetric_eigen_uses_lower_triangle() {
    for scale in [1e-200, 1.0, 1e200] {
        let a = [
            2. * scale,
            scale,
            0.,
            f64::NAN,
            2. * scale,
            0.,
            10.,
            20.,
            -4. * scale,
        ];
        let a = MatRef::new(&a, 3, 3).expect("matrix");
        let e = symmetric_eigen(a, false).expect("symmetric eigen");
        assert_eq!(
            e.values,
            symmetric_eigen(a, true).expect("values only").values
        );
        orthogonal(&e.vectors, 3);
        for (j, value) in e.values.iter().enumerate() {
            close(value / scale, [3., 1., -4.][j], 1e-14);
            for i in 0..3 {
                let av = (0..3)
                    .map(|k| (a.get(i.max(k), i.min(k)) / scale) * e.vectors[3 * j + k])
                    .sum();
                close(av, (value / scale) * e.vectors[3 * j + i], 1e-13);
            }
        }
    }
    for value in [0., 2., -4.] {
        let a = [value, 0., 0., value];
        let a = MatRef::new(&a, 2, 2).expect("matrix");
        let e = symmetric_eigen(a, false).expect("repeated eigenvalues");
        assert_eq!(e.values, [value; 2]);
        orthogonal(&e.vectors, 2);
    }
    let a = MatRef::new(&[-5.], 1, 1).expect("scalar");
    assert_eq!(
        symmetric_eigen(a, false).expect("scalar eigenvalue").values,
        [-5.]
    );
}

fn check_general(a: MatRef<'_>, scale: f64) -> roxide_math::linalg::spectral::GeneralEigen {
    let n = a.nrow;
    let e = general_eigen(a, false).expect("general eigen");
    assert_eq!(
        e.values,
        general_eigen(a, true).expect("values only").values
    );
    assert!(
        e.values
            .windows(2)
            .all(|p| p[0].re.hypot(p[0].im) >= p[1].re.hypot(p[1].im))
    );
    for (lambda, v) in e.values.iter().zip(e.vectors.chunks_exact(n)) {
        close(v.iter().map(|z| z.re * z.re + z.im * z.im).sum(), 1., 1e-13);
        for i in 0..n {
            let (mut re, mut im) = (0., 0.);
            for (j, z) in v.iter().enumerate() {
                re += a.get(i, j) / scale * z.re;
                im += a.get(i, j) / scale * z.im;
            }
            close(
                re,
                (lambda.re / scale) * v[i].re - (lambda.im / scale) * v[i].im,
                1e-13,
            );
            close(
                im,
                (lambda.re / scale) * v[i].im + (lambda.im / scale) * v[i].re,
                1e-13,
            );
        }
    }
    e
}

#[test]
fn general_eigenvalues_pairs_sorting_and_balancing() {
    for scale in [1e-200, 1., 1e200] {
        let rotation = [0., scale, -scale, 0.];
        let e = check_general(MatRef::new(&rotation, 2, 2).expect("rotation"), scale);
        close(e.values[0].im / scale, 1., 1e-14);
        close(e.values[1].im / scale, -1., 1e-14);
        close(e.vectors[0].re, std::f64::consts::FRAC_1_SQRT_2, 1e-14);
        close(e.vectors[1].im, -std::f64::consts::FRAC_1_SQRT_2, 1e-14);
    }
    let e = check_general(
        MatRef::new(&[2., 0., 0., 4., -8., 0., 6., 10., 1.], 3, 3)
            .expect("upper triangular matrix"),
        10.,
    );
    assert_eq!(
        e.values.iter().map(|z| z.re).collect::<Vec<_>>(),
        [-8., 2., 1.]
    );
    let e = check_general(
        MatRef::new(&[1., 1e-200, 1e200, 1.], 2, 2).expect("unbalanced matrix"),
        1.,
    );
    close(e.values[0].re, 2., 1e-14);
    close(e.values[1].re, 0., 1e-14);
    for a in [
        [2., 0., 0., 1., 2., 0., 0., 1., 2.],
        [2., 1e-13, 0., 1., 2., 1e-13, 0., 1., 2.],
        [0., 1., 0., 0., 0., 1., 1., 0., 0.],
        [1., 2., 4., 7., -4., -1., 3., 0., 2.],
    ] {
        check_general(MatRef::new(&a, 3, 3).expect("general matrix"), 10.);
    }
}

#[test]
fn deterministic_dense_samples() {
    let mut state = 19_u64;
    for n in 2..=20 {
        let a: Vec<_> = (0..n * n)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                ((state >> 32) as f64) / f64::from(u32::MAX) - 0.5
            })
            .collect();
        let a = MatRef::new(&a, n, n).expect("square matrix");
        check_general(a, n as f64);
        check_svd(a.data, n, n, 1.);
        let s = symmetric_eigen(a, false).expect("symmetric eigen");
        orthogonal(&s.vectors, n);
        for (j, &value) in s.values.iter().enumerate() {
            for i in 0..n {
                let av = (0..n)
                    .map(|k| a.get(i.max(k), i.min(k)) * s.vectors[n * j + k])
                    .sum();
                close(av, value * s.vectors[n * j + i], 1e-13);
            }
        }
    }
}

#[test]
fn norms_and_limits() {
    let a = MatRef::new(&[1., 2., 3., 4.], 2, 2).expect("matrix");
    for (kind, value) in [
        (MatrixNorm::One, 7.),
        (MatrixNorm::Infinity, 6.),
        (MatrixNorm::Maximum, 4.),
        (MatrixNorm::Frobenius, 30.0_f64.sqrt()),
        (MatrixNorm::Spectral, 5.464985704219043),
    ] {
        close(matrix_norm(a, kind).expect("norm"), value, 1e-14);
    }
    let nan = f64::from_bits(0x7ff8_0000_0000_07a2);
    let data = [nan];
    let a = MatRef::new(&data, 1, 1).expect("matrix");
    assert_eq!(
        matrix_norm(a, MatrixNorm::Maximum)
            .expect("missing norm")
            .to_bits(),
        nan.to_bits()
    );
    assert!(general_eigen(a, false).is_err());
    assert!(svd(a, 1, 1).is_err());
    let a = MatRef {
        data: &[],
        nrow: usize::MAX,
        ncol: 2,
    };
    assert!(svd(a, 0, 0).is_err());
}

#[test]
fn triangular_condition_matches_oracle_growth_estimate() {
    let data = [
        -2.034_136_359_416_822,
        0.0,
        0.0,
        -0.6375201278330286,
        1.6983178603804934,
        0.0,
        -1.2088031045827352,
        0.6158646796111059,
        -1.806357855913784,
    ];
    let a = MatRef::new(&data, 3, 3).expect("triangular matrix");
    close(
        triangular_condition(a, true, false).expect("condition number"),
        2.7405928786589984,
        1e-14,
    );
}

#[test]
fn spectral_scaling_retains_mixed_magnitude_diagonals() {
    let data = [1e200, 0.0, 0.0, 1e-200];
    let a = MatRef::new(&data, 2, 2).expect("diagonal matrix");
    assert_eq!(svd(a, 0, 0).expect("SVD").d, [1e200, 1e-200]);
    assert_eq!(
        symmetric_eigen(a, true).expect("symmetric eigen").values,
        [1e200, 1e-200]
    );
    let e = general_eigen(a, true).expect("general eigen");
    assert_eq!(
        e.values.iter().map(|x| x.re).collect::<Vec<_>>(),
        [1e200, 1e-200]
    );
}

#[test]
fn qr_scales_the_active_block_and_resolves_close_roots() {
    let mut data = [0.0; 16];
    data[0] = 1e200;
    data[9] = 1e-200;
    data[14] = 1e-200;
    data[7] = 1e-200;
    let e = general_eigen(MatRef::new(&data, 4, 4).expect("matrix"), false).expect("eigen");
    assert_eq!(e.values[0].re, 1e200);
    for z in &e.values[1..] {
        close(z.re.hypot(z.im) / 1e-200, 1.0, 1e-12);
    }
    assert!(
        e.values[1..]
            .iter()
            .any(|z| (z.re / 1e-200 - 1.0).abs() < 1e-12)
    );
    assert!(
        e.values[1..]
            .iter()
            .any(|z| (z.im / 1e-200 - 3.0_f64.sqrt() / 2.0).abs() < 1e-12)
    );

    let data = [1.0, -(1.0 - 2.0_f64.powi(-53)), 1.0, -1.0];
    let e = general_eigen(MatRef::new(&data, 2, 2).expect("matrix"), false).expect("eigen");
    for z in e.values {
        assert_eq!(z.im, 0.0);
        assert!((z.re.abs() - 1.05367121277235e-8).abs() < 1e-16);
    }
}

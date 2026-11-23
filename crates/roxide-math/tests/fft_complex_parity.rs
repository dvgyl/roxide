//! Check Fourier conventions and complex spectral residuals.

#![forbid(unsafe_code)]

use std::f64::consts::TAU;

use roxide_math::{
    Cplx,
    fft::{NextLengthError, fft, fft_nd, nextn},
    linalg::complex::{complex_general_eigen, complex_svd, hermitian_eigen},
};

fn add(a: Cplx, b: Cplx) -> Cplx {
    Cplx::new(a.re + b.re, a.im + b.im)
}

fn mul(a: Cplx, b: Cplx) -> Cplx {
    Cplx::new(a.re * b.re - a.im * b.im, a.re * b.im + a.im * b.re)
}

fn conj(a: Cplx) -> Cplx {
    Cplx::new(a.re, -a.im)
}

fn close(a: Cplx, b: Cplx, tolerance: f64) {
    let error = (a.re - b.re).hypot(a.im - b.im);
    assert!(error <= tolerance, "{a:?} differs from {b:?} by {error}");
}

fn input(n: usize) -> Vec<Cplx> {
    (0..n)
        .map(|k| Cplx::new((k as f64 * 0.41).sin(), (k as f64 * 0.27).cos()))
        .collect()
}

#[test]
fn mixed_radix_and_chirp_agree_with_the_fourier_definition() {
    for n in [1, 2, 4, 32, 60, 97, 1009] {
        let x = input(n);
        let mut y = x.clone();
        fft(&mut y, false, 4096).expect("Forward Fourier transform");
        for (k, &actual) in y.iter().enumerate() {
            let expected = x.iter().enumerate().fold(Cplx::default(), |sum, (j, &x)| {
                let (im, re) = (-TAU * ((j * k) % n) as f64 / n as f64).sin_cos();
                add(sum, mul(x, Cplx::new(re, im)))
            });
            close(actual, expected, 3e-12);
        }
        fft(&mut y, true, 4096).expect("Inverse Fourier transform");
        for (&a, &b) in y.iter().zip(&x) {
            close(Cplx::new(a.re / n as f64, a.im / n as f64), b, 5e-15);
        }
    }
}

#[test]
fn column_major_axes_match_a_direct_transform() {
    for shape in [vec![3, 4], vec![2, 3, 7]] {
        let n: usize = shape.iter().product();
        let x = input(n);
        let mut y = x.clone();
        fft_nd(&mut y, &shape, false, 4096).expect("Array Fourier transform");
        for (k, &actual) in y.iter().enumerate() {
            let expected = x
                .iter()
                .enumerate()
                .fold(Cplx::default(), |sum, (mut j, &x)| {
                    let mut k = k;
                    let mut phase = 0.0;
                    for &d in &shape {
                        phase += ((j % d) * (k % d)) as f64 / d as f64;
                        j /= d;
                        k /= d;
                    }
                    let (im, re) = (-TAU * phase).sin_cos();
                    add(sum, mul(x, Cplx::new(re, im)))
                });
            close(actual, expected, 4e-14);
        }
        fft_nd(&mut y, &shape, true, 4096).expect("Inverse array transform");
        for (&a, &b) in y.iter().zip(&x) {
            close(Cplx::new(a.re / n as f64, a.im / n as f64), b, 4e-15);
        }
    }
}

#[test]
fn length_search_preserves_greedy_factor_order() {
    assert_eq!(nextn(36, &[4, 6]), Ok(Some(64)));
    assert_eq!(nextn(36, &[6, 4]), Ok(Some(36)));
    assert_eq!(nextn(97, &[2, 3, 5]), Ok(Some(100)));
    assert_eq!(nextn(2_147_483_647, &[2, 3, 5]), Ok(Some(2_147_483_648)));
    assert_eq!(nextn(12, &[]), Err(NextLengthError::NoFactors));
    assert_eq!(nextn(12, &[2, 1]), Err(NextLengthError::InvalidFactors));
}

fn unitary(a: &[Cplx], n: usize) {
    for (i, x) in a.chunks_exact(n).enumerate() {
        for (j, y) in a.chunks_exact(n).enumerate() {
            let dot = x
                .iter()
                .zip(y)
                .fold(Cplx::default(), |sum, (&x, &y)| add(sum, mul(conj(x), y)));
            close(dot, Cplx::new(f64::from(i == j), 0.0), 1e-13);
        }
    }
}

#[test]
fn singular_vectors_reconstruct_tall_and_wide_matrices() {
    for (m, n) in [(5, 3), (3, 5), (3, 3), (1, 4)] {
        let mut a = input(m * n);
        a[m * n - 1] = Cplx::new(1.3, 2.1);
        let s = complex_svd(&a, m, n, m, n).expect("Complex SVD");
        unitary(&s.u, m);
        unitary(&s.vt, n);
        assert!(s.d.windows(2).all(|d| d[0] >= d[1]));
        for j in 0..n {
            for i in 0..m {
                let value =
                    s.d.iter()
                        .enumerate()
                        .fold(Cplx::default(), |sum, (k, &d)| {
                            add(
                                sum,
                                mul(mul(s.u[k * m + i], Cplx::new(d, 0.0)), s.vt[j * n + k]),
                            )
                        });
                close(value, a[j * m + i], 1e-13);
            }
        }
    }
}

fn eigen_residual(a: &[Cplx], n: usize, values: &[Cplx], vectors: &[Cplx]) {
    for (&value, x) in values.iter().zip(vectors.chunks_exact(n)) {
        for i in 0..n {
            let ax = (0..n).fold(Cplx::default(), |sum, j| add(sum, mul(a[j * n + i], x[j])));
            close(ax, mul(value, x[i]), 2e-12);
        }
    }
}

#[test]
fn hermitian_eigenvectors_form_an_orthonormal_basis() {
    let n = 5;
    let mut a = input(n * n);
    for j in 0..n {
        a[j * n + j].im = 0.0;
        for i in j + 1..n {
            a[i * n + j] = conj(a[j * n + i]);
        }
    }
    let e = hermitian_eigen(&a, n, false).expect("Hermitian eigensystem");
    assert!(e.values.windows(2).all(|d| d[0] >= d[1]));
    unitary(&e.vectors, n);
    let values: Vec<_> = e.values.iter().map(|&v| Cplx::new(v, 0.0)).collect();
    eigen_residual(&a, n, &values, &e.vectors);
}

#[test]
fn general_eigenvectors_have_unit_norm_and_a_real_pivot() {
    for n in [2, 3, 5, 12] {
        let mut a = input(n * n);
        for (i, z) in a.iter_mut().enumerate() {
            z.re += (i as f64 * 13.97).cos();
        }
        let e = complex_general_eigen(&a, n, false).expect("General eigensystem");
        eigen_residual(&a, n, &e.values, &e.vectors);
        for x in e.vectors.chunks_exact(n) {
            unitary(x, n);
            let pivot = x
                .iter()
                .max_by(|a, b| a.re.hypot(a.im).total_cmp(&b.re.hypot(b.im)))
                .expect("Nonempty eigenvector");
            assert_eq!(pivot.im, 0.0);
            assert!(pivot.re > 0.0);
        }
        assert!(
            e.values
                .windows(2)
                .all(|v| v[0].re.hypot(v[0].im) >= v[1].re.hypot(v[1].im))
        );
        let only = complex_general_eigen(&a, n, true).expect("Eigenvalues only");
        assert_eq!(only.values, e.values);
        assert!(only.vectors.is_empty());
    }
}

#[test]
fn extreme_fft_and_complex_decompositions() {
    let mut data = [Cplx::default(); 7];
    data[0].re = 1e308;
    fft(&mut data, false, 64).expect("FFT");
    assert_eq!(data, [Cplx::new(1e308, 0.0); 7]);
    let mut data = [Cplx::default(); 7];
    data[1].re = 1e308;
    fft(&mut data, false, 64).expect("FFT");
    assert!(
        data.iter()
            .all(|z| (z.re.hypot(z.im) / 1e308 - 1.0).abs() < 1e-14)
    );

    let mut a = [
        Cplx::new(1e200, 0.0),
        Cplx::default(),
        Cplx::default(),
        Cplx::new(1e-200, 0.0),
    ];
    assert_eq!(complex_svd(&a, 2, 2, 2, 2).expect("SVD").d, [1e200, 1e-200]);
    assert_eq!(
        hermitian_eigen(&a, 2, false)
            .expect("Hermitian eigen")
            .values,
        [1e200, 1e-200]
    );
    assert_eq!(
        complex_general_eigen(&a, 2, false)
            .expect("general eigen")
            .values[1],
        a[3]
    );

    a = [
        Cplx::default(),
        Cplx::new(1e-200, 1e-200),
        Cplx::new(1e308, 0.0),
        Cplx::default(),
    ];
    let e = hermitian_eigen(&a, 2, false).expect("lower triangle");
    assert!((e.values[0] / 1e-200 - 2.0_f64.sqrt()).abs() < 1e-14);
    assert!((e.values[1] / 1e-200 + 2.0_f64.sqrt()).abs() < 1e-14);
}

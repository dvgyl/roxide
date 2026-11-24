//! Check the LINPACK C ABI with column padding, optional outputs, and aliases.

use std::ffi::c_int;
use std::ptr::{null, null_mut};

use roxide_capi as _;

unsafe extern "C" {
    fn dpbfa_(a: *mut f64, ld: *const c_int, n: *const c_int, m: *const c_int, info: *mut c_int);
    fn dpbsl_(a: *const f64, ld: *const c_int, n: *const c_int, m: *const c_int, b: *mut f64);
    fn dpoco_(
        a: *mut f64,
        ld: *const c_int,
        n: *const c_int,
        rcond: *mut f64,
        z: *mut f64,
        info: *mut c_int,
    );
    fn dpodi_(a: *mut f64, ld: *const c_int, n: *const c_int, det: *mut f64, job: *const c_int);
    fn dpofa_(a: *mut f64, ld: *const c_int, n: *const c_int, info: *mut c_int);
    fn dposl_(a: *const f64, ld: *const c_int, n: *const c_int, b: *mut f64);
    fn dqrdc_(
        x: *mut f64,
        ld: *const c_int,
        n: *const c_int,
        p: *const c_int,
        qraux: *mut f64,
        jpvt: *mut c_int,
        work: *mut f64,
        job: *const c_int,
    );
    fn dqrsl_(
        x: *const f64,
        ld: *const c_int,
        n: *const c_int,
        k: *const c_int,
        qraux: *const f64,
        y: *const f64,
        qy: *mut f64,
        qty: *mut f64,
        b: *mut f64,
        rsd: *mut f64,
        xb: *mut f64,
        job: *const c_int,
        info: *mut c_int,
    );
    fn dsvdc_(
        x: *mut f64,
        ldx: *const c_int,
        n: *const c_int,
        p: *const c_int,
        s: *mut f64,
        e: *mut f64,
        u: *mut f64,
        ldu: *const c_int,
        v: *mut f64,
        ldv: *const c_int,
        work: *mut f64,
        job: *const c_int,
        info: *mut c_int,
    );
    fn dtrco_(
        t: *const f64,
        ld: *const c_int,
        n: *const c_int,
        rcond: *mut f64,
        z: *mut f64,
        job: *const c_int,
    );
    fn dtrsl_(
        t: *const f64,
        ld: *const c_int,
        n: *const c_int,
        b: *mut f64,
        job: *const c_int,
        info: *mut c_int,
    );
}

const PAD: f64 = 99.0;

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-12, "{actual} vs {expected}");
}

fn vector(actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (&value, &target) in actual.iter().zip(expected) {
        close(value, target);
    }
}

#[test]
fn band_cholesky_preserves_padding_and_unused_band_entries() {
    let mut a = [PAD, 4., PAD, 1., 4., PAD, 1., 4.];
    let mut rhs = [6., 12., 14., PAD];
    let mut info = -9;
    // SAFETY: Each column has two band entries. Only the last column omits padding.
    unsafe {
        dpbfa_(a.as_mut_ptr(), &3, &3, &1, &mut info);
        assert_eq!(info, 0);
        dpbsl_(a.as_ptr(), &3, &3, &1, rhs.as_mut_ptr());
    }
    vector(&rhs, &[1., 2., 3., PAD]);
    assert_eq!([a[0], a[2], a[5]], [PAD; 3]);
}

#[test]
fn dense_cholesky_solve_and_condition_estimates() {
    let mut a = [4., PAD, PAD, 2., 10., PAD];
    let mut rhs = [8., 22., PAD];
    let mut info = -9;
    // SAFETY: The matrices have two active rows per column. The vectors have two entries.
    unsafe {
        dpofa_(a.as_mut_ptr(), &3, &2, &mut info);
        assert_eq!(info, 0);
        dposl_(a.as_ptr(), &3, &2, rhs.as_mut_ptr());
    }
    vector(&rhs, &[1., 2., PAD]);
    vector(&a, &[2., PAD, PAD, 1., 3., PAD]);

    let mut rcond = PAD;
    let mut z = [PAD; 3];
    let mut a = [1., PAD, PAD, 0., 16., PAD];
    // SAFETY: All active matrix and vector entries are in bounds.
    unsafe {
        dpoco_(
            a.as_mut_ptr(),
            &3,
            &2,
            &mut rcond,
            z.as_mut_ptr(),
            &mut info,
        )
    };
    assert_eq!(info, 0);
    close(rcond, 17. / 257.);
    close(z[0].abs() + z[1].abs(), 1.);
    assert_eq!(z[2], PAD);
    vector(&a, &[1., PAD, PAD, 0., 4., PAD]);

    let mut indefinite = [4., PAD, PAD, 2., 0., PAD];
    rcond = PAD;
    // SAFETY: All active matrix and vector entries are in bounds.
    unsafe {
        dpoco_(
            indefinite.as_mut_ptr(),
            &3,
            &2,
            &mut rcond,
            z.as_mut_ptr(),
            &mut info,
        )
    };
    assert_eq!(info, 2);
    assert_eq!(rcond, PAD);
    vector(&z, &[6., 2., PAD]);

    let mut indefinite = [1e300, PAD, PAD, 0., -1e-300, PAD];
    // SAFETY: All active matrix and vector entries are in bounds.
    unsafe {
        dpoco_(
            indefinite.as_mut_ptr(),
            &3,
            &2,
            &mut rcond,
            z.as_mut_ptr(),
            &mut info,
        )
    };
    assert_eq!(info, 2);
    assert_eq!(rcond, PAD);
    assert_eq!(z, [1e300, 1e-300, PAD]);
}

#[test]
fn determinant_and_inverse_follow_independent_job_digits() {
    let factor = [2., PAD, PAD, 1., 3., PAD];
    let inverse = [5. / 18., PAD, PAD, -1. / 18., 1. / 9., PAD];
    for job in [0, 1, 10, 11] {
        let mut a = factor;
        let mut det = [PAD; 3];
        // SAFETY: The upper triangle is initialized. The determinant has room for two entries.
        unsafe { dpodi_(a.as_mut_ptr(), &3, &2, det.as_mut_ptr(), &job) };
        vector(&a, if job % 10 == 0 { &factor } else { &inverse });
        vector(&det, if job < 10 { &[PAD; 3] } else { &[3.6, 1., PAD] });
    }
    let mut a = factor;
    // SAFETY: Job 01 uses only the matrix. Job 00 uses neither output.
    unsafe {
        dpodi_(a.as_mut_ptr(), &3, &2, null_mut(), &1);
        dpodi_(null_mut(), null(), null(), null_mut(), &0);
    }
    vector(&a, &inverse);
}

#[test]
fn qr_no_pivot_accepts_null_scratch_and_applies_all_outputs() {
    let mut x = [1., 1., 0., PAD, 0., 0., 1., PAD];
    let mut aux = [PAD; 2];
    let (mut qy, mut qty, mut b, mut rsd, mut xb) =
        ([PAD; 3], [PAD; 3], [PAD; 2], [PAD; 3], [PAD; 3]);
    let y = [1., 3., 2.];
    let mut info = -9;
    // SAFETY: All requested arrays have the corresponding matrix or vector length.
    unsafe {
        dqrdc_(
            x.as_mut_ptr(),
            &4,
            &3,
            &2,
            aux.as_mut_ptr(),
            null_mut(),
            null_mut(),
            &0,
        );
        dqrsl_(
            x.as_ptr(),
            &4,
            &3,
            &2,
            aux.as_ptr(),
            y.as_ptr(),
            qy.as_mut_ptr(),
            qty.as_mut_ptr(),
            b.as_mut_ptr(),
            rsd.as_mut_ptr(),
            xb.as_mut_ptr(),
            &11111,
            &mut info,
        );
    }
    assert_eq!(info, 0);
    vector(&b, &[2., 2.]);
    vector(&rsd, &[-1., 1., 0.]);
    vector(&xb, &[2., 2., 2.]);
    close(qy.iter().map(|v| v * v).sum(), 14.);
    close(qty.iter().map(|v| v * v).sum(), 14.);
    assert_eq!([x[3], x[7]], [PAD; 2]);
}

#[test]
fn qr_pivot_reports_one_based_column_order() {
    let mut x = [1., 0., 0., PAD, 0., 5., 0., PAD, 0., 0., 2., PAD];
    let mut aux = [PAD; 3];
    let mut work = [PAD; 3];
    let mut pivots = [0; 3];
    // SAFETY: Three columns and each pivot and work entry are in bounds.
    unsafe {
        dqrdc_(
            x.as_mut_ptr(),
            &4,
            &3,
            &3,
            aux.as_mut_ptr(),
            pivots.as_mut_ptr(),
            work.as_mut_ptr(),
            &1,
        );
    }
    assert_eq!(pivots, [2, 3, 1]);
    vector(&[x[0].abs(), x[5].abs(), x[10].abs()], &[5., 2., 1.]);
    assert_eq!([x[3], x[7], x[11]], [PAD; 3]);
}

#[test]
fn qr_allows_input_and_output_aliases() {
    let mut x = [1., 1., 0., PAD, 0., 0., 1.];
    let mut aux = [0.; 2];
    // SAFETY: The matrix has three active rows per column.
    unsafe {
        dqrdc_(
            x.as_mut_ptr(),
            &4,
            &3,
            &2,
            aux.as_mut_ptr(),
            null_mut(),
            null_mut(),
            &0,
        )
    };
    for (job, expected) in [
        (10000, None),
        (1000, None),
        (100, Some([2., 2., 2.])),
        (10, Some([-1., 1., 0.])),
        (1, Some([2., 2., 2.])),
    ] {
        let original = [1., 3., 2.];
        let mut y = original;
        let mut expected_output = [PAD; 3];
        let mut info = -9;
        let shared = y.as_mut_ptr();
        let reference = expected_output.as_mut_ptr();
        let outputs = |out| {
            let mut buffers = [null_mut(); 5];
            let index = [10000, 1000, 100, 10, 1]
                .iter()
                .position(|&digit| digit == job)
                .unwrap();
            buffers[index] = out;
            buffers
        };
        let [qy, qty, b, rsd, xb] = outputs(shared);
        // SAFETY: Shared storage holds three entries. Unselected pointers are null.
        unsafe {
            dqrsl_(
                x.as_ptr(),
                &4,
                &3,
                &2,
                aux.as_ptr(),
                shared,
                qy,
                qty,
                b,
                rsd,
                xb,
                &job,
                &mut info,
            );
        }
        assert_eq!(info, 0);
        if let Some(expected) = expected {
            vector(&y, &expected);
        } else {
            let [qy, qty, b, rsd, xb] = outputs(reference);
            // SAFETY: The selected output and the input each hold three entries.
            unsafe {
                dqrsl_(
                    x.as_ptr(),
                    &4,
                    &3,
                    &2,
                    aux.as_ptr(),
                    original.as_ptr(),
                    qy,
                    qty,
                    b,
                    rsd,
                    xb,
                    &job,
                    &mut info,
                );
            }
            assert_eq!(info, 0);
            vector(&y, &expected_output);
        }
    }

    let mut y = [1., 3., 2.];
    let shared = y.as_mut_ptr();
    let mut info = -9;
    // SAFETY: The y, qty, and rsd arguments may share the same three active entries.
    unsafe {
        dqrsl_(
            x.as_ptr(),
            &4,
            &3,
            &2,
            aux.as_ptr(),
            shared,
            null_mut(),
            shared,
            null_mut(),
            shared,
            null_mut(),
            &1010,
            &mut info,
        );
    }
    assert_eq!(info, 0);
    vector(&y, &[-1., 1., 0.]);
}

#[test]
fn qr_singular_solve_keeps_partial_coefficients() {
    let x = [0., 0., PAD, 2., 3.];
    let mut info = -9;
    let mut b = [PAD; 2];
    let mut qty = [PAD; 2];
    // SAFETY: The work vector and the coefficient output each hold two entries.
    unsafe {
        dqrsl_(
            x.as_ptr(),
            &3,
            &2,
            &2,
            [0.].as_ptr(),
            [5., 7.].as_ptr(),
            null_mut(),
            qty.as_mut_ptr(),
            b.as_mut_ptr(),
            null_mut(),
            null_mut(),
            &100,
            &mut info,
        );
    }
    assert_eq!(info, 1);
    vector(&qty, &[5., 7.]);
    vector(&b, &[1. / 3., 7. / 3.]);

    let mut scalar = PAD;
    // SAFETY: A one-row factor has no Householder reflectors and needs no qraux.
    unsafe {
        dqrsl_(
            [0.].as_ptr(),
            &1,
            &1,
            &1,
            null(),
            [7.].as_ptr(),
            null_mut(),
            null_mut(),
            &mut scalar,
            null_mut(),
            null_mut(),
            &100,
            &mut info,
        );
    }
    assert_eq!(info, 1);
    assert_eq!(scalar, PAD);
}

fn orthonormal(matrix: &[f64], ld: usize, rows: usize, cols: usize) {
    for j in 0..cols {
        for k in 0..cols {
            let dot = (0..rows)
                .map(|i| matrix[j * ld + i] * matrix[k * ld + i])
                .sum();
            close(dot, f64::from(j == k));
        }
        assert_eq!(matrix[j * ld + rows], PAD);
    }
}

#[test]
fn svd_job_digits_select_outputs_for_tall_and_wide_matrices() {
    for (n, p) in [(3, 2), (2, 4)] {
        let (rows, cols) = (n as usize, p as usize);
        let (ldx, ldu, ldv) = (n + 1, n + 1, p + 1);
        let (lx, lu, lv) = (ldx as usize, ldu as usize, ldv as usize);
        let mut original = vec![PAD; lx * cols];
        for j in 0..cols {
            original[j * lx..j * lx + rows].fill(0.);
        }
        original[0] = 3.;
        original[lx + 1] = 4.;
        for job in [0, 1, 10, 11, 21] {
            let mut x = original.clone();
            let mut u = vec![PAD; lu * rows];
            let mut v = vec![PAD; lv * cols];
            let (mut values, mut e, mut work) = ([PAD; 4], vec![PAD; cols + 1], vec![PAD; rows]);
            let mut info = -9;
            let (u_ptr, u_ld) = if job >= 10 {
                (u.as_mut_ptr(), &ldu as *const _)
            } else {
                (null_mut(), null())
            };
            let (v_ptr, v_ld) = if job % 10 != 0 {
                (v.as_mut_ptr(), &ldv as *const _)
            } else {
                (null_mut(), null())
            };
            // SAFETY: Active columns and vectors fit in their respective buffers.
            unsafe {
                dsvdc_(
                    x.as_mut_ptr(),
                    &ldx,
                    &n,
                    &p,
                    values.as_mut_ptr(),
                    e.as_mut_ptr(),
                    u_ptr,
                    u_ld,
                    v_ptr,
                    v_ld,
                    work.as_mut_ptr(),
                    &job,
                    &mut info,
                );
            }
            assert_eq!(info, 0, "job {job:02}, {n} x {p}");
            vector(&values[..2], &[4., 3.]);
            let order = (rows + 1).min(cols);
            vector(&values[2..order], &vec![0.; order - 2]);
            assert_eq!(values[order], PAD);
            assert_eq!(e[cols], PAD);
            assert!(x.chunks_exact(lx).all(|column| column[rows] == PAD));
            let left_cols = match job / 10 {
                0 => 0,
                1 => rows,
                _ => rows.min(cols),
            };
            orthonormal(&u, lu, rows, left_cols);
            assert!(u[left_cols * lu..].iter().all(|&value| value == PAD));
            if job % 10 != 0 {
                orthonormal(&v, lv, cols, cols);
                if left_cols != 0 {
                    for j in 0..cols {
                        for i in 0..rows {
                            let reconstructed = (0..rows.min(cols))
                                .map(|k| u[k * lu + i] * values[k] * v[k * lv + j])
                                .sum();
                            close(reconstructed, original[j * lx + i]);
                        }
                    }
                }
            } else {
                assert!(v.iter().all(|&value| value == PAD));
            }
        }
    }
}

#[test]
fn svd_can_store_reduced_left_vectors_in_input_matrix() {
    let mut x = [3., 0., 0., PAD, 0., 4., 0., PAD];
    let shared = x.as_mut_ptr();
    let (mut values, mut e, mut work) = ([PAD; 2], [PAD; 2], [PAD; 3]);
    let mut info = -9;
    // SAFETY: The shared input and reduced left output both need two padded columns.
    unsafe {
        dsvdc_(
            shared,
            &4,
            &3,
            &2,
            values.as_mut_ptr(),
            e.as_mut_ptr(),
            shared,
            &4,
            null_mut(),
            null(),
            work.as_mut_ptr(),
            &20,
            &mut info,
        );
    }
    assert_eq!(info, 0);
    vector(&values, &[4., 3.]);
    orthonormal(&x, 4, 3, 2);
    close(x[1].abs(), 1.);
    close(x[4].abs(), 1.);
}

#[test]
fn triangular_solves_cover_four_jobs_and_report_singular_diagonal() {
    let mut a = [2., 3., PAD, 4., 5., PAD];
    for (job, mut rhs) in [
        (0, [2., 13., PAD]),
        (1, [10., 10., PAD]),
        (10, [8., 10., PAD]),
        (11, [2., 14., PAD]),
    ] {
        let mut info = -9;
        // SAFETY: The padded matrix and the right hand side hold all active entries.
        unsafe { dtrsl_(a.as_ptr(), &3, &2, rhs.as_mut_ptr(), &job, &mut info) };
        assert_eq!(info, 0);
        vector(&rhs, &[1., 2., PAD]);
    }
    a[4] = 0.;
    let mut rhs = [7., 8.];
    let mut info = -9;
    // SAFETY: Both arrays have their declared lengths.
    unsafe { dtrsl_(a.as_ptr(), &3, &2, rhs.as_mut_ptr(), &1, &mut info) };
    assert_eq!(info, 2);
    assert_eq!(rhs, [7., 8.]);
}

#[test]
fn triangular_condition_estimates_use_selected_triangle() {
    for (job, matrix) in [(0, [1., 0., PAD, PAD, 16.]), (1, [1., PAD, PAD, 0., 16.])] {
        let mut rcond = PAD;
        let mut z = [PAD; 3];
        // SAFETY: The selected triangle and active condition vector entries are in bounds.
        unsafe { dtrco_(matrix.as_ptr(), &3, &2, &mut rcond, z.as_mut_ptr(), &job) };
        close(rcond, 17. / 257.);
        close(z[0].abs() + z[1].abs(), 1.);
        assert_eq!(z[2], PAD);
    }
}

#[test]
fn invalid_arguments_leave_arrays_unchanged() {
    let mut a = [PAD; 6];
    for (ld, n) in [(1, 2), (3, 0), (3, -1)] {
        let mut info = -9;
        // SAFETY: The matrix and status are valid. These dimensions must be rejected.
        unsafe { dpofa_(a.as_mut_ptr(), &ld, &n, &mut info) };
        assert_eq!(info, -1);
    }
    let mut info = -9;
    let mut rhs = [PAD; 2];
    // SAFETY: The null arguments exercise boundary validation. The other pointers are valid.
    unsafe {
        dpbfa_(a.as_mut_ptr(), &3, &2, &-1, &mut info);
        assert_eq!(info, -1);
        dpbfa_(null_mut(), &3, &2, &1, &mut info);
        assert_eq!(info, -1);
        dpbfa_(a.as_mut_ptr(), &3, &0, &1, &mut info);
        assert_eq!(info, -1);
        dposl_(null(), &3, &2, rhs.as_mut_ptr());
        dpofa_(a.as_mut_ptr(), &3, &2, null_mut());
        dtrsl_(a.as_ptr(), &3, &2, rhs.as_mut_ptr(), null(), &mut info);
        assert_eq!(info, -1);
        dqrdc_(
            a.as_mut_ptr(),
            &3,
            &2,
            &0,
            null_mut(),
            null_mut(),
            null_mut(),
            &0,
        );
        dqrsl_(
            a.as_ptr(),
            &3,
            &2,
            &3,
            null(),
            null(),
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &0,
            &mut info,
        );
        assert_eq!(info, -1);
        dsvdc_(
            a.as_mut_ptr(),
            &3,
            &2,
            &0,
            null_mut(),
            null_mut(),
            null_mut(),
            null(),
            null_mut(),
            null(),
            null_mut(),
            &0,
            &mut info,
        );
        assert_eq!(info, -1);
    }
    assert_eq!(a, [PAD; 6]);
    assert_eq!(rhs, [PAD; 2]);
}

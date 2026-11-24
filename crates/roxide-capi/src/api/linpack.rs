//! LINPACK Fortran C API wrappers (`R_ext/Linpack.h`).

use std::ffi::c_int;
use std::ops::Range;
use std::ptr;

use roxide_math::linalg::linpack_capi as kernels;

fn valid_span<T>(data: *const T, len: usize) -> bool {
    len == 0
        || (!data.is_null()
            && data.is_aligned()
            && len.checked_mul(size_of::<T>()).is_some_and(|bytes| {
                bytes <= isize::MAX as usize && data.addr().checked_add(bytes).is_some()
            }))
}

unsafe fn integer(data: *const c_int) -> Option<c_int> {
    if !valid_span(data, 1) {
        return None;
    }
    // SAFETY: The caller supplies a readable scalar at each valid address.
    Some(unsafe { data.read() })
}

unsafe fn dimension(data: *const c_int) -> Option<usize> {
    // SAFETY: The caller supplies a readable dimension at a valid address.
    usize::try_from(unsafe { integer(data)? }).ok()
}

fn zeroed<T: Copy + Default>(len: usize) -> Option<Vec<T>> {
    let mut data = Vec::new();
    data.try_reserve_exact(len).ok()?;
    data.resize(len, T::default());
    Some(data)
}

unsafe fn load<T: Copy + Default>(data: *const T, len: usize) -> Option<Vec<T>> {
    if !valid_span(data, len) {
        return None;
    }
    let mut values = zeroed(len)?;
    if len != 0 {
        // SAFETY: The caller supplies initialized entries. The new vector is disjoint.
        unsafe { ptr::copy_nonoverlapping(data, values.as_mut_ptr(), len) };
    }
    Some(values)
}

unsafe fn store<T: Copy>(out: *mut T, values: &[T]) {
    if !values.is_empty() {
        // SAFETY: The caller checked the output span. The Rust buffer is disjoint.
        unsafe { ptr::copy_nonoverlapping(values.as_ptr(), out, values.len()) };
    }
}

#[derive(Clone, Copy)]
enum Entries {
    All,
    Upper,
    Lower,
    Diagonal,
    Band,
}

impl Entries {
    fn rows(self, rows: usize, col: usize) -> Range<usize> {
        match self {
            Self::All => 0..rows,
            Self::Upper => 0..col + 1,
            Self::Lower => col..rows,
            Self::Diagonal => col..col + 1,
            Self::Band => (rows - 1).saturating_sub(col)..rows,
        }
    }
}

struct Matrix {
    ld: usize,
    rows: usize,
    cols: usize,
}

impl Matrix {
    fn new(data: *const f64, ld: usize, rows: usize, cols: usize) -> Option<Self> {
        let len = if cols == 0 {
            0
        } else {
            (cols - 1).checked_mul(ld)?.checked_add(rows)?
        };
        (rows > 0 && ld >= rows && valid_span(data, len)).then_some(Self { ld, rows, cols })
    }

    unsafe fn load(&self, data: *const f64, entries: Entries) -> Option<Vec<f64>> {
        let mut values = zeroed::<f64>(self.rows.checked_mul(self.cols)?)?;
        for col in 0..self.cols {
            let rows = entries.rows(self.rows, col);
            let start = col * self.rows + rows.start;
            // SAFETY: Matrix::new checked the span. The caller supplies initialized
            // entries in this range. The new vector is disjoint.
            unsafe {
                ptr::copy_nonoverlapping(
                    data.add(col * self.ld + rows.start),
                    values.as_mut_ptr().add(start),
                    rows.len(),
                );
            }
        }
        Some(values)
    }

    unsafe fn store(&self, out: *mut f64, entries: Entries, values: &[f64]) {
        for col in 0..self.cols {
            let rows = entries.rows(self.rows, col);
            let start = col * self.rows + rows.start;
            // SAFETY: Matrix::new checked the span. The caller supplies writable
            // entries in this range. The Rust buffer is disjoint.
            unsafe {
                store(
                    out.add(col * self.ld + rows.start),
                    &values[start..start + rows.len()],
                );
            }
        }
    }
}

unsafe fn report(info: *mut c_int, run: impl FnOnce() -> Option<c_int>) {
    if valid_span(info, 1) {
        let status = run().unwrap_or(-1);
        // SAFETY: Each caller supplies writable storage at a valid info address.
        unsafe { info.write(status) };
    }
}

/// Factor a symmetric positive definite band matrix.
///
/// # Safety
///
/// Non-null arguments that the routine uses must point to valid storage of the
/// size declared by the LINPACK interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dpbfa_(
    abd: *mut f64,
    lda: *const c_int,
    n: *const c_int,
    m: *const c_int,
    info: *mut c_int,
) {
    // SAFETY: The caller supplies writable info storage at a valid address.
    unsafe {
        report(info, || {
            // SAFETY: The C caller supplies the scalars and the active band entries.
            let (ld, n, m) = (dimension(lda)?, dimension(n)?, dimension(m)?);
            if n == 0 {
                return None;
            }
            let matrix = Matrix::new(abd, ld, m + 1, n)?;
            let mut a = matrix.load(abd, Entries::Band)?;
            let status = kernels::dpbfa(&mut a, m + 1, n, m);
            matrix.store(abd, Entries::Band, &a);
            Some(status)
        })
    };
}

/// Solve a system with a band Cholesky factor.
///
/// # Safety
///
/// Non-null arguments that the routine uses must point to valid storage of the
/// size declared by the LINPACK interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dpbsl_(
    abd: *const f64,
    lda: *const c_int,
    n: *const c_int,
    m: *const c_int,
    b: *mut f64,
) {
    let run = || {
        // SAFETY: The C caller supplies the scalars, active band entries, and RHS.
        unsafe {
            let (ld, n, m) = (dimension(lda)?, dimension(n)?, dimension(m)?);
            if n == 0 {
                return None;
            }
            let matrix = Matrix::new(abd, ld, m + 1, n)?;
            let mut rhs = load(b, n)?;
            let a = matrix.load(abd, Entries::Band)?;
            kernels::dpbsl(&a, m + 1, n, m, &mut rhs);
            store(b, &rhs);
            Some(())
        }
    };
    run();
}

/// Factor a symmetric positive definite matrix and estimate its reciprocal condition number.
///
/// # Safety
///
/// Non-null arguments that the routine uses must point to valid storage of the
/// size declared by the LINPACK interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dpoco_(
    a: *mut f64,
    lda: *const c_int,
    n: *const c_int,
    rcond: *mut f64,
    z: *mut f64,
    info: *mut c_int,
) {
    // SAFETY: The caller supplies writable info storage at a valid address.
    unsafe {
        report(info, || {
            // SAFETY: The C caller supplies the scalars, upper triangle, and outputs.
            let n = dimension(n)?;
            let matrix = Matrix::new(a, dimension(lda)?, n, n)?;
            if !valid_span(rcond, 1) || !valid_span(z, n) {
                return None;
            }
            let mut factor = matrix.load(a, Entries::Upper)?;
            let mut direction = zeroed(n)?;
            let (estimate, status) = kernels::dpoco(&mut factor, n, n, &mut direction);
            if status != 0 {
                // GNU R returns the original column sums if factorization fails.
                direction.fill(0.0);
                for col in 0..n {
                    let mut sum = 0.0;
                    for (row, previous) in direction[..col].iter_mut().enumerate() {
                        let value = a.add(col * matrix.ld + row).read().abs();
                        *previous += value;
                        sum += value;
                    }
                    direction[col] = sum + a.add(col * matrix.ld + col).read().abs();
                }
            }
            matrix.store(a, Entries::Upper, &factor);
            store(z, &direction);
            if status == 0 {
                rcond.write(estimate);
            }
            Some(status)
        })
    };
}

/// Compute a determinant, inverse, or both from a Cholesky factor.
///
/// # Safety
///
/// Non-null arguments that the routine uses must point to valid storage of the
/// size declared by the LINPACK interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dpodi_(
    a: *mut f64,
    lda: *const c_int,
    n: *const c_int,
    det: *mut f64,
    job: *const c_int,
) {
    let run = || {
        // SAFETY: The C caller supplies the scalars, upper triangle, and requested output.
        unsafe {
            let job = integer(job)?;
            if job == 0 {
                return Some(());
            }
            let determinant = job / 10 != 0;
            if determinant && !valid_span(det, 2) {
                return None;
            }
            let n = dimension(n)?;
            let matrix = Matrix::new(a, dimension(lda)?, n, n)?;
            let inverse = job % 10 != 0;
            let entries = if inverse {
                Entries::Upper
            } else {
                Entries::Diagonal
            };
            let mut factor = matrix.load(a, entries)?;
            let mut result = [0.0; 2];
            kernels::dpodi(&mut factor, n, n, &mut result, job);
            if inverse {
                matrix.store(a, entries, &factor);
            }
            if determinant {
                store(det, &result);
            }
            Some(())
        }
    };
    run();
}

/// Factor the upper triangle of a symmetric positive definite matrix.
///
/// # Safety
///
/// Non-null arguments that the routine uses must point to valid storage of the
/// size declared by the LINPACK interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dpofa_(a: *mut f64, lda: *const c_int, n: *const c_int, info: *mut c_int) {
    // SAFETY: The caller supplies writable info storage at a valid address.
    unsafe {
        report(info, || {
            // SAFETY: The C caller supplies the scalars and upper triangle.
            let n = dimension(n)?;
            let matrix = Matrix::new(a, dimension(lda)?, n, n)?;
            let mut factor = matrix.load(a, Entries::Upper)?;
            let status = kernels::dpofa(&mut factor, n, n);
            matrix.store(a, Entries::Upper, &factor);
            Some(status)
        })
    };
}

/// Solve a system with a dense Cholesky factor.
///
/// # Safety
///
/// Non-null arguments that the routine uses must point to valid storage of the
/// size declared by the LINPACK interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dposl_(a: *const f64, lda: *const c_int, n: *const c_int, b: *mut f64) {
    let run = || {
        // SAFETY: The C caller supplies the scalars, upper triangle, and RHS.
        unsafe {
            let n = dimension(n)?;
            let matrix = Matrix::new(a, dimension(lda)?, n, n)?;
            let mut rhs = load(b, n)?;
            let factor = matrix.load(a, Entries::Upper)?;
            kernels::dposl(&factor, n, n, &mut rhs);
            store(b, &rhs);
            Some(())
        }
    };
    run();
}

/// Compute a packed Householder QR factorization with optional pivoting.
///
/// # Safety
///
/// Non-null arguments that the routine uses must point to valid storage of the
/// size declared by the LINPACK interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dqrdc_(
    x: *mut f64,
    ldx: *const c_int,
    n: *const c_int,
    p: *const c_int,
    qraux: *mut f64,
    jpvt: *mut c_int,
    work: *mut f64,
    job: *const c_int,
) {
    let run = || {
        // SAFETY: The C caller supplies the scalars, matrix, and selected arrays.
        unsafe {
            let (n, p, pivot) = (dimension(n)?, dimension(p)?, integer(job)? != 0);
            let matrix = Matrix::new(x, dimension(ldx)?, n, p)?;
            if p == 0 || !valid_span(qraux, p) || (pivot && !valid_span(work, p)) {
                return None;
            }
            let (mut pivots, mut scratch) = if pivot {
                (load(jpvt, p)?, zeroed(p)?)
            } else {
                (Vec::new(), Vec::new())
            };
            let free_start = pivots.iter().filter(|&&entry| entry > 0).count();
            let free_end = p - pivots.iter().filter(|&&entry| entry < 0).count();
            let mut aux = zeroed(p)?;
            let mut factor = matrix.load(x, Entries::All)?;
            kernels::dqrdc(
                &mut factor,
                n,
                n,
                p,
                &mut aux,
                &mut pivots,
                &mut scratch,
                i32::from(pivot),
            );
            matrix.store(x, Entries::All, &factor);
            let count = n.min(p);
            store(qraux, &aux[..count]);
            if pivot {
                if free_end > count.max(free_start) {
                    let start = count.max(free_start);
                    store(qraux.add(start), &aux[start..free_end]);
                }
                store(jpvt, &pivots);
            }
            Some(())
        }
    };
    run();
}

/// Apply a packed QR factorization and write the outputs selected by `job`.
///
/// # Safety
///
/// Non-null arguments that the routine uses must point to valid storage of the
/// size declared by the LINPACK interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dqrsl_(
    x: *const f64,
    ldx: *const c_int,
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
) {
    // SAFETY: The caller supplies writable info storage at a valid address.
    unsafe {
        report(info, || {
            // SAFETY: The C caller supplies the scalars and selected arrays.
            // The kernels receive separate Rust buffers even when C arrays share storage.
            let (n, k, job) = (dimension(n)?, dimension(k)?, integer(job)?);
            let matrix = Matrix::new(x, dimension(ldx)?, n, k)?;
            if k > n {
                return None;
            }
            if job == 0 {
                return Some(0);
            }
            let need_qty = job % 10000 != 0;
            let destinations = [
                (qy, n, job / 10000 != 0),
                (qty, n, job / 1000 % 10 != 0 || (need_qty && !qty.is_null())),
                (b, k, job / 100 % 10 != 0),
                (rsd, n, job / 10 % 10 != 0),
                (xb, n, job % 10 != 0),
            ];
            for (out, len, enabled) in destinations {
                if enabled && !valid_span(out, len) {
                    return None;
                }
            }
            let [qy, _, b, rsd, xb] =
                destinations.map(|(_, len, enabled)| if enabled { len } else { 0 });
            let (mut qy_buf, mut qty_buf, mut b_buf, mut rsd_buf, mut xb_buf) = (
                zeroed(qy)?,
                zeroed(if need_qty { n } else { 0 })?,
                zeroed(b)?,
                zeroed(rsd)?,
                zeroed(xb)?,
            );
            let factor = matrix.load(x, Entries::All)?;
            let aux = load(qraux, k.min(n - 1))?;
            let input = load(y, n)?;
            let mut status = kernels::dqrsl(
                &factor,
                n,
                n,
                k,
                &aux,
                &input,
                (qy != 0).then_some(qy_buf.as_mut_slice()),
                need_qty.then_some(qty_buf.as_mut_slice()),
                (b != 0).then_some(b_buf.as_mut_slice()),
                (rsd != 0).then_some(rsd_buf.as_mut_slice()),
                (xb != 0).then_some(xb_buf.as_mut_slice()),
                job,
            );
            if status > 0 {
                if n == 1 {
                    b_buf.clear();
                } else {
                    status = partial_coefficients(&factor, n, &qty_buf, &mut b_buf)?;
                }
            }
            for ((out, _, enabled), values) in destinations.into_iter().zip([
                qy_buf.as_slice(),
                &qty_buf,
                &b_buf,
                &rsd_buf,
                &xb_buf,
            ]) {
                if enabled {
                    store(out, values);
                }
            }
            Some(status)
        })
    };
}

fn partial_coefficients(factor: &[f64], n: usize, qty: &[f64], b: &mut [f64]) -> Option<c_int> {
    // LINPACK updates b in descending column order until it finds a zero diagonal.
    let rank = b.len();
    let zero = (0..rank).rfind(|&j| factor[j * n + j] == 0.0)?;
    b.copy_from_slice(&qty[..rank]);
    let first = zero + 1;
    if first < rank {
        kernels::dtrsl(
            &factor[first * n + first..],
            n,
            rank - first,
            &mut b[first..],
            1,
        );
        for j in (first..rank).rev() {
            for i in 0..first {
                b[i] -= factor[j * n + i] * b[j];
            }
        }
    }
    c_int::try_from(first).ok()
}

unsafe fn singular_vectors(
    out: *mut f64,
    ld: *const c_int,
    rows: usize,
    cols: usize,
) -> Option<(Option<Matrix>, Vec<f64>)> {
    if cols == 0 {
        return Some((None, Vec::new()));
    }
    // SAFETY: The caller supplies the leading dimension of the requested output.
    let matrix = Matrix::new(out, unsafe { dimension(ld)? }, rows, cols)?;
    Some((Some(matrix), zeroed(rows.checked_mul(cols)?)?))
}

/// Compute the singular values and the singular vectors selected by `job`.
///
/// # Safety
///
/// Non-null arguments that the routine uses must point to valid storage of the
/// size declared by the LINPACK interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dsvdc_(
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
) {
    // SAFETY: The caller supplies writable info storage at a valid address.
    unsafe {
        report(info, || {
            // SAFETY: The C caller supplies the scalars, matrix, and selected arrays.
            let (n, p, job) = (dimension(n)?, dimension(p)?, integer(job)?);
            let matrix = Matrix::new(x, dimension(ldx)?, n, p)?;
            let order = (n + 1).min(p);
            if p == 0 || !valid_span(s, order) || !valid_span(e, p) || !valid_span(work, n) {
                return None;
            }
            // Fortran uses 1 for all left vectors and 2 or more for the reduced set.
            // The Rust kernel uses the reverse mapping.
            let (nu, left_job) = match job / 10 % 10 {
                0 => (0, 0),
                1 => (n, 2),
                _ => (n.min(p), 1),
            };
            let right_job = i32::from(job % 10 != 0);
            let (u_matrix, mut left) = singular_vectors(u, ldu, n, nu)?;
            let (v_matrix, mut right) =
                singular_vectors(v, ldv, p, if right_job == 0 { 0 } else { p })?;
            let mut a = matrix.load(x, Entries::All)?;
            let (mut values, mut superdiag, mut scratch) = (zeroed(order)?, zeroed(p)?, zeroed(n)?);
            let status = kernels::dsvdc(
                &mut a,
                n,
                n,
                p,
                &mut values,
                &mut superdiag,
                u_matrix.as_ref().map(|_| left.as_mut_slice()),
                n,
                v_matrix.as_ref().map(|_| right.as_mut_slice()),
                p,
                &mut scratch,
                10 * left_job + right_job,
            );
            if status >= 0 {
                matrix.store(x, Entries::All, &a);
                store(s, &values);
                store(e, &superdiag);
                if let Some(matrix) = u_matrix {
                    matrix.store(u, Entries::All, &left);
                }
                if let Some(matrix) = v_matrix {
                    matrix.store(v, Entries::All, &right);
                }
            }
            Some(status)
        })
    };
}

/// Estimate the reciprocal condition number of a triangular matrix.
///
/// # Safety
///
/// Non-null arguments that the routine uses must point to valid storage of the
/// size declared by the LINPACK interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtrco_(
    t: *const f64,
    ldt: *const c_int,
    n: *const c_int,
    rcond: *mut f64,
    z: *mut f64,
    job: *const c_int,
) {
    let run = || {
        // SAFETY: The C caller supplies the scalars, selected triangle, and outputs.
        unsafe {
            let (n, job) = (dimension(n)?, integer(job)?);
            let matrix = Matrix::new(t, dimension(ldt)?, n, n)?;
            if !valid_span(rcond, 1) || !valid_span(z, n) {
                return None;
            }
            let entries = if job == 0 {
                Entries::Lower
            } else {
                Entries::Upper
            };
            let triangle = matrix.load(t, entries)?;
            let mut direction = zeroed(n)?;
            let estimate = kernels::dtrco(&triangle, n, n, &mut direction, job);
            store(z, &direction);
            rcond.write(estimate);
            Some(())
        }
    };
    run();
}

/// Solve a triangular system and report a zero diagonal by its one-based index.
///
/// # Safety
///
/// Non-null arguments that the routine uses must point to valid storage of the
/// size declared by the LINPACK interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtrsl_(
    t: *const f64,
    ldt: *const c_int,
    n: *const c_int,
    b: *mut f64,
    job: *const c_int,
    info: *mut c_int,
) {
    // SAFETY: The caller supplies writable info storage at a valid address.
    unsafe {
        report(info, || {
            // SAFETY: The C caller supplies the scalars, selected triangle, and RHS.
            let (n, job) = (dimension(n)?, integer(job)?);
            let matrix = Matrix::new(t, dimension(ldt)?, n, n)?;
            let entries = if job % 10 == 0 {
                Entries::Lower
            } else {
                Entries::Upper
            };
            let triangle = matrix.load(t, entries)?;
            let mut rhs = load(b, n)?;
            let status = kernels::dtrsl(&triangle, n, n, &mut rhs, job);
            if status == 0 {
                store(b, &rhs);
            }
            Some(status)
        })
    };
}

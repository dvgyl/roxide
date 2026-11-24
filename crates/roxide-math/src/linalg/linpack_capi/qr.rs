use super::tri::{Triangle, solve};
use super::{fits, index_info};
use crate::linalg::linpack::norm;

fn swap_columns(x: &mut [f64], ld: usize, n: usize, a: usize, b: usize) {
    if a != b {
        for i in 0..n {
            x.swap(a * ld + i, b * ld + i);
        }
    }
}

/// Compute a packed Householder QR factorization in column-major storage.
/// With nonzero `job`, positive pivot entries mark initial columns, zero
/// entries mark free columns, and negative entries mark final columns.
/// Return the original one-based column indices in `jpvt` when pivoting.
#[allow(clippy::too_many_arguments)]
pub fn dqrdc(
    x: &mut [f64],
    ldx: usize,
    n: usize,
    p: usize,
    qraux: &mut [f64],
    jpvt: &mut [i32],
    work: &mut [f64],
    job: i32,
) {
    let pivot = job != 0;
    if n == 0
        || p == 0
        || !fits(x.len(), ldx, n, p)
        || qraux.len() < p
        || (pivot && (p > i32::MAX as usize || jpvt.len() < p || work.len() < p))
    {
        return;
    }

    let mut first = 0;
    let mut end = 0;
    if pivot {
        for j in 0..p {
            let class = jpvt[j];
            jpvt[j] = if class < 0 {
                -index_info(j)
            } else {
                index_info(j)
            };
            if class > 0 {
                swap_columns(x, ldx, n, j, first);
                jpvt.swap(j, first);
                first += 1;
            }
        }
        end = p;
        for j in (first..p).rev() {
            if jpvt[j] < 0 {
                jpvt[j] = -jpvt[j];
                end -= 1;
                swap_columns(x, ldx, n, j, end);
                jpvt.swap(j, end);
            }
        }
        for j in first..end {
            qraux[j] = norm(&x[j * ldx..j * ldx + n]);
            work[j] = qraux[j];
        }
    }

    for j in 0..n.min(p) {
        if (first..end).contains(&j) {
            let mut best = j;
            for k in j + 1..end {
                if qraux[k] > qraux[best] {
                    best = k;
                }
            }
            swap_columns(x, ldx, n, j, best);
            jpvt.swap(j, best);
            qraux.swap(j, best);
            work.swap(j, best);
        }
        qraux[j] = 0.0;
        if j + 1 == n {
            break;
        }
        let start = j * ldx;
        let length = norm(&x[start + j..start + n]);
        if length == 0.0 {
            continue;
        }
        let signed = if x[start + j] < 0.0 { -length } else { length };
        for entry in &mut x[start + j..start + n] {
            *entry /= signed;
        }
        x[start + j] += 1.0;
        qraux[j] = x[start + j];
        x[start + j] = -signed;

        for k in j + 1..p {
            let (earlier, target) = x.split_at_mut(k * ldx);
            let col = &mut target[j..n];
            reflect(qraux[j], &earlier[start + j + 1..start + n], col);
            if (first..end).contains(&k) && qraux[k] != 0.0 {
                let ratio = col[0].abs() / qraux[k];
                let remaining = (1.0 - ratio * ratio).max(0.0);
                let reference = qraux[k] / work[k];
                if 1.0 + 0.05 * remaining * reference * reference == 1.0 {
                    qraux[k] = norm(&col[1..]);
                    work[k] = qraux[k];
                } else {
                    qraux[k] *= remaining.sqrt();
                }
            }
        }
    }
}

fn reflect(lead: f64, tail: &[f64], y: &mut [f64]) {
    if lead != 0.0 {
        let dot = tail.iter().zip(&y[1..]).map(|(a, b)| a * b).sum::<f64>() + lead * y[0];
        let scale = -dot / lead;
        y[0] += scale * lead;
        for (entry, &v) in y[1..].iter_mut().zip(tail) {
            *entry += scale * v;
        }
    }
}

fn apply(x: &[f64], ldx: usize, qraux: &[f64], y: &mut [f64], transpose: bool) {
    let n = y.len();
    for step in 0..qraux.len() {
        let j = if transpose {
            step
        } else {
            qraux.len() - 1 - step
        };
        let tail = &x[j * ldx + j + 1..j * ldx + n];
        reflect(qraux[j], tail, &mut y[j..]);
    }
}

/// Apply the first `k` packed QR columns to an input vector.
/// The five decimal places in `job` request `qy`, `qty`, `b`, `rsd`, and `xb`
/// in that order. A request for `b`, `rsd`, or `xb` also computes `qty`.
/// Pass `None` to discard an output. The caller can copy the input first to
/// reuse its buffer for an output, as the Rust slices cannot alias.
/// A singular coefficient solve returns the first one-based zero diagonal
/// index and preserves `b`. Other requested outputs are still computed.
#[allow(clippy::too_many_arguments)]
pub fn dqrsl(
    x: &[f64],
    ldx: usize,
    n: usize,
    k: usize,
    qraux: &[f64],
    y: &[f64],
    qy: Option<&mut [f64]>,
    qty: Option<&mut [f64]>,
    b: Option<&mut [f64]>,
    rsd: Option<&mut [f64]>,
    xb: Option<&mut [f64]>,
    job: i32,
) -> i32 {
    let qy = qy.filter(|_| job / 10000 != 0);
    let qty = qty.filter(|_| job % 10000 != 0);
    let b = b.filter(|_| (job / 100) % 10 != 0);
    let rsd = rsd.filter(|_| (job / 10) % 10 != 0);
    let xb = xb.filter(|_| job % 10 != 0);
    let count = k.min(n.saturating_sub(1));
    if k > n
        || !fits(x.len(), ldx, n, k)
        || qraux.len() < count
        || y.len() < n
        || b.as_ref().is_some_and(|out| out.len() < k)
        || [&qy, &qty, &rsd, &xb]
            .into_iter()
            .any(|out| out.as_ref().is_some_and(|out| out.len() < n))
    {
        return -1;
    }

    let y = &y[..n];
    let reflectors = &qraux[..count];
    if let Some(qy) = qy {
        qy[..n].copy_from_slice(y);
        apply(x, ldx, reflectors, &mut qy[..n], false);
    }
    if job % 10000 == 0 {
        return 0;
    }
    let mut scratch = Vec::new();
    let qty = if let Some(out) = qty {
        out[..n].copy_from_slice(y);
        &mut out[..n]
    } else {
        scratch.extend_from_slice(y);
        &mut scratch[..]
    };
    apply(x, ldx, reflectors, qty, true);

    let mut info = 0;
    if (job / 100) % 10 != 0 {
        if let Some(j) = (0..k).find(|&j| x[j * ldx + j] == 0.0) {
            info = index_info(j);
        } else if let Some(b) = b {
            b[..k].copy_from_slice(&qty[..k]);
            solve(Triangle::new(x, ldx, k, true, 1.0), &mut b[..k], false);
        }
    }
    if let Some(rsd) = rsd {
        rsd[..k].fill(0.0);
        rsd[k..n].copy_from_slice(&qty[k..]);
        apply(x, ldx, reflectors, &mut rsd[..n], false);
    }
    if let Some(xb) = xb {
        xb[..k].copy_from_slice(&qty[..k]);
        xb[k..n].fill(0.0);
        apply(x, ldx, reflectors, &mut xb[..n], false);
    }
    info
}

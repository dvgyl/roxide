use super::{fits, index_info};
use crate::linalg::factor::dlarfg;

struct Vectors<'a> {
    data: &'a mut [f64],
    ld: usize,
    rows: usize,
    cols: usize,
}

impl<'a> Vectors<'a> {
    fn new(data: Option<&'a mut [f64]>, ld: usize, rows: usize, cols: usize) -> Option<Self> {
        if cols == 0 || rows == 0 {
            return Some(Self {
                data: &mut [],
                ld,
                rows,
                cols: 0,
            });
        }
        let data = data?;
        fits(data.len(), ld, rows, cols).then_some(Self {
            data,
            ld,
            rows,
            cols,
        })
    }

    fn column(&mut self, j: usize) -> &mut [f64] {
        &mut self.data[j * self.ld..j * self.ld + self.rows]
    }

    fn identity(&mut self) {
        for j in 0..self.cols {
            let col = self.column(j);
            col.fill(0.0);
            col[j] = 1.0;
        }
    }

    fn modify_pair(&mut self, i: usize, j: usize, operation: impl Fn(f64, f64) -> (f64, f64)) {
        if i < self.cols && j < self.cols {
            for row in 0..self.rows {
                let (x, y) = (i * self.ld + row, j * self.ld + row);
                (self.data[x], self.data[y]) = operation(self.data[x], self.data[y]);
            }
        }
    }

    fn rotate(&mut self, i: usize, j: usize, g: Givens) {
        self.modify_pair(i, j, |x, y| (g.c * x + g.s * y, g.c * y - g.s * x));
    }

    fn swap(&mut self, i: usize, j: usize) {
        self.modify_pair(i, j, |x, y| (y, x));
    }
}

#[derive(Clone, Copy)]
struct Givens {
    c: f64,
    s: f64,
    r: f64,
}

impl Givens {
    fn new(x: f64, y: f64) -> Self {
        let r = x.hypot(y);
        if r == 0.0 {
            Self { c: 1.0, s: 0.0, r }
        } else {
            Self {
                c: x / r,
                s: y / r,
                r,
            }
        }
    }
}

/// Reduce a column-major matrix to bidiagonal form and compute its SVD.
/// The input matrix may change. `s` has length at least `min(n + 1, p)`,
/// `e` has length at least `p`, and `work` has length at least `n`.
///
/// The units digit of `job` requests all right vectors when nonzero.
/// For this Rust API the tens digit requests no left vectors (0),
/// `min(n, p)` left vectors (1), or all `n` left vectors (2 and above).
/// The Fortran entry point uses 1 for all left vectors and 2 and above
/// for the reduced set.
///
/// On success, return zero, store singular values in decreasing order, and
/// zero the superdiagonal. A positive convergence status counts the unresolved
/// prefix of the bidiagonal matrix. The corresponding entries of `s` and `e`
/// hold its diagonal and superdiagonal.
#[allow(clippy::too_many_arguments)]
pub fn dsvdc(
    x: &mut [f64],
    ldx: usize,
    n: usize,
    p: usize,
    s: &mut [f64],
    e: &mut [f64],
    u: Option<&mut [f64]>,
    ldu: usize,
    v: Option<&mut [f64]>,
    ldv: usize,
    work: &mut [f64],
    job: i32,
) -> i32 {
    let rank = n.min(p);
    let order = n.saturating_add(1).min(p);
    let nu = match (job / 10) % 10 {
        0 => 0,
        1 => rank,
        _ => n,
    };
    let nv = if job % 10 == 0 { 0 } else { p };
    if !fits(x.len(), ldx, n, p) || s.len() < order || e.len() < p || work.len() < n {
        return -1;
    }
    let (Some(mut u), Some(mut v)) = (Vectors::new(u, ldu, n, nu), Vectors::new(v, ldv, p, nv))
    else {
        return -1;
    };
    let s = &mut s[..order];
    let e = &mut e[..p];
    if rank == 0 {
        s.fill(0.0);
        e.fill(0.0);
        u.identity();
        v.identity();
        return 0;
    }

    let mut max: f64 = 0.0;
    for j in 0..p {
        for &value in &x[j * ldx..j * ldx + n] {
            if !value.is_finite() {
                return -1;
            }
            max = max.max(value.abs());
        }
    }
    let exponent = ((max.to_bits() >> 52) & 0x7ff) as i32 - 1023;
    let shift = match exponent {
        _ if max == 0.0 => 0,
        i if i > 450 => i - 450,
        i if i < -450 => i.max(-1022),
        _ => 0,
    };
    let scale = 2.0_f64.powi(shift);
    if scale != 1.0 {
        for j in 0..p {
            for value in &mut x[j * ldx..j * ldx + n] {
                *value /= scale;
            }
        }
    }

    s.fill(0.0);
    e.fill(0.0);
    let (mut left, mut right) = (vec![0.0; rank], vec![0.0; rank]);
    for k in 0..rank {
        let start = k * ldx;
        let (alpha, tail) = x[start + k..start + n].split_at_mut(1);
        left[k] = dlarfg(&mut alpha[0], tail);
        s[k] = alpha[0];
        for j in k + 1..p {
            let (earlier, target) = x.split_at_mut(j * ldx);
            reflect(
                left[k],
                &earlier[start + k + 1..start + n],
                &mut target[k..n],
            );
        }
        if k + 1 == p {
            break;
        }
        let first = (k + 1) * ldx + k;
        e[k] = x[first];
        for j in k + 2..p {
            e[j] = x[j * ldx + k];
        }
        let (alpha, tail) = e[k..].split_at_mut(1);
        right[k] = dlarfg(&mut alpha[0], &mut tail[1..]);
        if right[k] != 0.0 {
            let work = &mut work[k + 1..n];
            work.copy_from_slice(&x[first + 1..(k + 1) * ldx + n]);
            for j in k + 2..p {
                for (i, dot) in work.iter_mut().enumerate() {
                    *dot += e[j] * x[j * ldx + k + 1 + i];
                }
            }
            for j in k + 1..p {
                let weight = right[k] * if j == k + 1 { 1.0 } else { e[j] };
                for (i, &dot) in work.iter().enumerate() {
                    x[j * ldx + k + 1 + i] -= weight * dot;
                }
            }
        }
        for j in k + 2..p {
            x[j * ldx + k] = e[j];
        }
    }
    e[order - 1..].fill(0.0);

    u.identity();
    v.identity();
    let mut row = vec![0.0; if nv == 0 { 0 } else { p }];
    for k in (0..rank).rev() {
        let tail = &x[k * ldx + k + 1..k * ldx + n];
        for j in k..u.cols {
            reflect(left[k], tail, &mut u.column(j)[k..]);
        }
        if right[k] != 0.0 && nv != 0 {
            for j in k + 2..p {
                row[j] = x[j * ldx + k];
            }
            for j in k + 1..v.cols {
                reflect(right[k], &row[k + 2..], &mut v.column(j)[k + 1..]);
            }
        }
    }

    let info = diagonalize(s, &mut e[..order], &mut u, &mut v);
    for value in s.iter_mut().chain(e) {
        *value *= scale;
    }
    info
}

fn reflect(tau: f64, tail: &[f64], x: &mut [f64]) {
    if tau != 0.0 {
        let dot = x[0] + tail.iter().zip(&x[1..]).map(|(a, b)| a * b).sum::<f64>();
        let weight = tau * dot;
        x[0] -= weight;
        for (entry, &v) in x[1..].iter_mut().zip(tail) {
            *entry -= weight * v;
        }
    }
}

fn diagonalize(d: &mut [f64], e: &mut [f64], u: &mut Vectors<'_>, v: &mut Vectors<'_>) -> i32 {
    let mut end = d.len();
    let mut iterations = 0;
    while end > 0 {
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
            if d[last].is_sign_negative() {
                d[last] = -d[last];
                if last < v.cols {
                    for value in v.column(last) {
                        *value = -*value;
                    }
                }
            }
            let mut j = last;
            while j + 1 < d.len() && d[j] < d[j + 1] {
                d.swap(j, j + 1);
                u.swap(j, j + 1);
                v.swap(j, j + 1);
                j += 1;
            }
            end -= 1;
            iterations = 0;
            continue;
        }
        if iterations == 128 {
            return index_info(last);
        }
        iterations += 1;

        if d[last] == 0.0 {
            let mut bulge = e[last - 1];
            e[last - 1] = 0.0;
            for j in (start..last).rev() {
                let g = Givens::new(d[j], bulge);
                d[j] = g.r;
                v.rotate(j, last, g);
                if j > start {
                    bulge = -g.s * e[j - 1];
                    e[j - 1] *= g.c;
                }
            }
        } else if let Some(zero) = (start..last).find(|&j| d[j] == 0.0) {
            let mut bulge = e[zero];
            e[zero] = 0.0;
            for j in zero + 1..end {
                let g = Givens::new(d[j], bulge);
                d[j] = g.r;
                u.rotate(j, zero, g);
                bulge = -g.s * e[j];
                e[j] *= g.c;
            }
        } else {
            qr_step(d, e, u, v, start, last);
        }
    }
    0
}

fn qr_step(
    d: &mut [f64],
    e: &mut [f64],
    u: &mut Vectors<'_>,
    v: &mut Vectors<'_>,
    start: usize,
    last: usize,
) {
    // Compute the shift from the trailing two rows of B Bᵀ. Use a zero
    // shift when the active block has a small diagonal to retain precision.
    let p = last - 1;
    let scale = [d[start], e[start], d[p], e[p], d[last]]
        .into_iter()
        .map(f64::abs)
        .fold(0.0, f64::max);
    let (a, b, c) = (d[p] / scale, e[p] / scale, d[last] / scale);
    let delta = ((a - c) * (a + c) + b * b) / 2.0;
    let off = b * c;
    let shift = if off == 0.0 {
        c * c
    } else {
        c * c - off * (off / (delta + delta.hypot(off).copysign(delta)))
    };
    let min = d[start..=last]
        .iter()
        .map(|x| x.abs())
        .fold(scale, f64::min);
    let x = d[start] / scale;
    let (mut f, mut bulge) = if min / scale <= f64::EPSILON.sqrt() {
        (d[start], e[start])
    } else {
        (x * x - shift, x * (e[start] / scale))
    };

    for j in start..last {
        let right = Givens::new(f, bulge);
        if j > start {
            e[j - 1] = right.r;
        }
        f = right.c * d[j] + right.s * e[j];
        e[j] = right.c * e[j] - right.s * d[j];
        bulge = right.s * d[j + 1];
        d[j + 1] *= right.c;
        v.rotate(j, j + 1, right);

        let left = Givens::new(f, bulge);
        d[j] = left.r;
        f = left.c * e[j] + left.s * d[j + 1];
        d[j + 1] = left.c * d[j + 1] - left.s * e[j];
        bulge = left.s * e[j + 1];
        e[j + 1] *= left.c;
        u.rotate(j, j + 1, left);
    }
    e[last - 1] = f;
}

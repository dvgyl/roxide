//! Balanced Hessenberg reduction, Francis QR and Schur back substitution.

use crate::{Cplx, MatRef, MathError, linalg::factor::dlarfg};

use super::{area, eye, length, reflect, reflect_right, rotate, scale};

/// Right eigenvectors and eigenvalues of a general real matrix.
#[derive(Debug)]
pub struct GeneralEigen {
    /// Eigenvalues in decreasing modulus, with the positive imaginary value first in each pair.
    pub values: Vec<Cplx>,
    /// Column-major right eigenvectors, or an empty vector for a values-only call.
    pub vectors: Vec<Cplx>,
}

/// Compute the eigendecomposition of a nonempty, square real matrix.
///
/// Complex pairs use conjugate columns.
pub fn general_eigen(a: MatRef<'_>, only_values: bool) -> Result<GeneralEigen, MathError> {
    let n = a.nrow;
    MatRef::new(a.data, n, a.ncol)?;
    if n == 0 || a.ncol != n {
        return Err(MathError::Dim);
    }
    area::<f64>(n, n)?;
    if !only_values {
        area::<Cplx>(n, n)?;
    }
    if a.data.iter().any(|x| !x.is_finite()) {
        return Err(MathError::NoConvergence { info: 1 });
    }
    let mut h = a.data.to_vec();
    let balance = Balance::new(&mut h, n)?;
    let mut factor = scale(h.iter().copied())?;
    if h.iter().any(|&x| x != 0.0 && x / factor == 0.0) {
        factor = 1.0;
    }
    for x in &mut h {
        *x /= factor;
    }
    let mut q = eye(n, if only_values { 0 } else { n })?;
    hessenberg(&mut h, &mut q, n, balance.start, balance.end);
    schur(&mut h, &mut q, n, balance.start, balance.end)?;
    let mut blocks = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        let (value, width) = if i + 1 < n && h[i * n + i + 1] != 0.0 {
            (pair(&h, n, i).values()[0], 2)
        } else {
            (Cplx::new(h[i * (n + 1)], 0.0), 1)
        };
        blocks.push((i, value, width));
        i += width;
    }
    blocks.sort_by(|a, b| abs(b.1).total_cmp(&abs(a.1)));
    let mut values = Vec::with_capacity(n);
    let mut vectors = Vec::new();
    if !only_values {
        vectors.reserve_exact(area::<Cplx>(n, n)?);
    }
    let mut x = vec![Cplx::default(); if only_values { 0 } else { n }];
    let mut y = x.clone();
    for (i, value, width) in blocks {
        let reported = Cplx::new(value.re * factor, value.im * factor);
        values.push(reported);
        if width == 2 {
            values.push(conj(reported));
        }
        if !only_values {
            schur_vector(&h, n, i, width, value, &mut x);
            balance.transform(&q, &x, &mut y);
            vectors.extend_from_slice(&y);
            if width == 2 {
                vectors.extend(y.iter().copied().map(conj));
            }
        }
    }
    Ok(GeneralEigen { values, vectors })
}

struct Balance {
    start: usize,
    end: usize,
    order: Vec<usize>,
    exponents: Vec<i32>,
}

impl Balance {
    fn new(h: &mut [f64], n: usize) -> Result<Self, MathError> {
        let (mut start, mut end) = (0, n);
        let mut order: Vec<_> = (0..n).collect();
        let swap = |h: &mut [f64], order: &mut [usize], a, b| {
            order.swap(a, b);
            for j in 0..n {
                h.swap(a * n + j, b * n + j);
                h.swap(j * n + a, j * n + b);
            }
        };
        while start < end {
            if let Some(i) = (start..end)
                .rev()
                .find(|&i| (start..end).all(|j| i == j || h[j * n + i] == 0.0))
            {
                end -= 1;
                swap(h, &mut order, i, end);
            } else if let Some(j) =
                (start..end).find(|&j| (start..end).all(|i| i == j || h[j * n + i] == 0.0))
            {
                swap(h, &mut order, j, start);
                start += 1;
            } else {
                break;
            }
        }
        let mut exponents = vec![0_i32; n];
        for _ in 0..n.saturating_mul(32) {
            let mut changed = false;
            for i in start..end {
                let indices = (start..end).filter(|&j| i != j);
                let r = length(indices.clone().map(|j| h[j * n + i]));
                let c = length(indices.map(|j| h[i * n + j]));
                if r == 0.0 || c == 0.0 || !r.is_finite() || !c.is_finite() {
                    continue;
                }
                let exp = ((r.log2() - c.log2()) / 2.0).round().clamp(-512.0, 512.0) as i32;
                let f = 2.0_f64.powi(exp);
                let norm = c.max(r);
                if exp == 0 || (c / norm) * f + (r / norm) / f >= 0.95 * (c / norm + r / norm) {
                    continue;
                }
                let safe = |x: f64, y: f64| x == 0.0 || (y.is_finite() && y != 0.0);
                if (0..n).filter(|&j| i != j).any(|j| {
                    !safe(h[j * n + i], h[j * n + i] / f) || !safe(h[i * n + j], h[i * n + j] * f)
                }) {
                    continue;
                }
                for j in (0..n).filter(|&j| i != j) {
                    h[j * n + i] /= f;
                    h[i * n + j] *= f;
                }
                exponents[i] = exponents[i].checked_add(exp).ok_or(MathError::TooLarge)?;
                changed = true;
            }
            if !changed {
                return Ok(Self {
                    start,
                    end,
                    order,
                    exponents,
                });
            }
        }
        Err(MathError::NoConvergence { info: 1 })
    }

    fn transform(&self, q: &[f64], x: &[Cplx], y: &mut [Cplx]) {
        let n = x.len();
        let mut max = f64::NEG_INFINITY;
        for (i, &original) in self.order.iter().enumerate() {
            let value = x.iter().enumerate().fold(Cplx::default(), |sum, (j, &x)| {
                add(sum, times(x, q[j * n + i]))
            });
            y[original] = value;
            max = max.max(abs(value).log2() + f64::from(self.exponents[i]));
        }
        for (i, &original) in self.order.iter().enumerate() {
            let norm = abs(y[original]);
            if norm != 0.0 {
                let weight = (norm.log2() + f64::from(self.exponents[i]) - max).exp2();
                let x = y[original];
                y[original] = Cplx::new(x.re / norm * weight, x.im / norm * weight);
            }
        }
        let mut pivot = 0;
        for (i, &v) in y.iter().enumerate() {
            if abs(v) > abs(y[pivot]) * (1.0 + 2.0 * f64::EPSILON) {
                pivot = i;
            }
        }
        let norm = length(y.iter().map(|&v| abs(v)));
        if norm > 0.0 {
            let phase = times(conj(y[pivot]), 1.0 / abs(y[pivot]));
            for v in &mut *y {
                *v = times(mul(*v, phase), 1.0 / norm);
            }
            y[pivot].im = 0.0;
        }
    }
}

fn hessenberg(h: &mut [f64], q: &mut [f64], n: usize, start: usize, end: usize) {
    let mut v = vec![0.0; n];
    for j in start..end.saturating_sub(2) {
        let p = j + 1;
        let reflector = &mut v[..end - p];
        reflector.copy_from_slice(&h[j * n + p..j * n + end]);
        let (alpha, tail) = reflector.split_at_mut(1);
        let tau = dlarfg(&mut alpha[0], tail);
        for col in h[p * n..].chunks_exact_mut(n) {
            reflect(tau, tail, &mut col[p..end]);
        }
        reflect_right(h, n, n, p, tau, tail);
        reflect_right(q, n, n, p, tau, tail);
        h[j * n + p] = alpha[0];
        h[j * n + p + 1..j * n + end].fill(0.0);
    }
}

fn schur(
    h: &mut [f64],
    q: &mut [f64],
    n: usize,
    start: usize,
    mut end: usize,
) -> Result<(), MathError> {
    let mut iteration = 0;
    while end > start {
        let last = end - 1;
        let mut lo = last;
        while lo > start {
            if negligible(h, n, lo - 1) {
                h[(lo - 1) * n + lo] = 0.0;
                break;
            }
            lo -= 1;
        }
        if lo == last {
            end -= 1;
            iteration = 0;
        } else if lo + 1 == last {
            let block = pair(h, n, lo);
            let value = block.values()[0];
            if value.im == 0.0 {
                let (x, y) = block.null_vector(value);
                let norm = x.re.hypot(y.re);
                if norm > 0.0 {
                    let (c, s) = (x.re / norm, y.re / norm);
                    rotate(h, n, lo, last, c, s);
                    rotate(q, n, lo, last, c, s);
                    for col in h.chunks_exact_mut(n) {
                        let (u, v) = (col[lo], col[last]);
                        col[lo] = c * u + s * v;
                        col[last] = c * v - s * u;
                    }
                }
                h[lo * (n + 1)] = value.re;
                h[last * (n + 1)] = block.values()[1].re;
                h[lo * n + last] = 0.0;
            }
            end -= 2;
            iteration = 0;
        } else {
            iteration += 1;
            if iteration > 128 {
                return Err(MathError::NoConvergence {
                    info: i32::try_from(end).unwrap_or(i32::MAX),
                });
            }
            let magnitude = (lo..=last)
                .flat_map(|j| &h[j * n + lo..j * n + end])
                .map(|x| x.abs())
                .fold(0.0, f64::max);
            let entry = |r, c| h[c * n + r] / magnitude;
            let trailing = pair(h, n, last - 1).scaled_by(magnitude);
            let (top, bottom, product) = if iteration % 16 == 0 {
                let delta = entry(last, last - 1).abs() + entry(last - 1, last - 2).abs();
                let shift = trailing.d + 0.75 * delta;
                (shift, shift, -0.4375 * delta * delta)
            } else {
                (trailing.a, trailing.d, trailing.b * trailing.c)
            };
            francis(h, q, n, lo, last, [top, bottom, product], magnitude);
        }
    }
    Ok(())
}

fn negligible(h: &[f64], n: usize, i: usize) -> bool {
    let Block { a, b, c, d } = pair(h, n, i);
    if c == 0.0 {
        return true;
    }
    // Check the effect of dropping c, including when b is much larger than c.
    if c.abs() > f64::EPSILON * (a.abs() + d.abs()) {
        return false;
    }
    let ab = b.abs().max(c.abs());
    let ba = b.abs().min(c.abs());
    let aa = d.abs().max((a - d).abs());
    let bb = d.abs().min((a - d).abs());
    let scale = aa.max(ab);
    scale > 0.0 && (ba / scale) * (ab / scale) <= f64::EPSILON * (bb / scale) * (aa / scale)
}

fn francis(
    h: &mut [f64],
    q: &mut [f64],
    n: usize,
    lo: usize,
    last: usize,
    shifts: [f64; 3],
    scale: f64,
) {
    let [a, d, product] = shifts;
    let h00 = h[lo * (n + 1)] / scale;
    let h10 = h[lo * n + lo + 1] / scale;
    let mut v = [
        (h00 - a) * (h00 - d) + (h[(lo + 1) * n + lo] / scale) * h10 - product,
        h10 * ((h00 - a) + (h[(lo + 1) * (n + 1)] / scale - d)),
        h10 * (h[(lo + 1) * n + lo + 2] / scale),
    ];
    for k in lo..last {
        let width = 3.min(last - k + 1);
        if k > lo {
            v[..width].copy_from_slice(&h[(k - 1) * n + k..(k - 1) * n + k + width]);
        }
        let (alpha, tail) = v[..width].split_at_mut(1);
        let tau = dlarfg(&mut alpha[0], tail);
        if k > lo {
            h[(k - 1) * n + k] = alpha[0];
            h[(k - 1) * n + k + 1..(k - 1) * n + k + width].fill(0.0);
        }
        for col in h[k * n..].chunks_exact_mut(n) {
            reflect(tau, tail, &mut col[k..k + width]);
        }
        reflect_right(h, n, n.min(k + width + 1), k, tau, tail);
        reflect_right(q, n, n, k, tau, tail);
    }
}

#[derive(Clone, Copy)]
struct Block {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

fn pair(h: &[f64], n: usize, i: usize) -> Block {
    Block {
        a: h[i * (n + 1)],
        b: h[(i + 1) * n + i],
        c: h[i * n + i + 1],
        d: h[(i + 1) * (n + 1)],
    }
}

impl Block {
    fn scaled_by(self, scale: f64) -> Self {
        Self {
            a: self.a / scale,
            b: self.b / scale,
            c: self.c / scale,
            d: self.d / scale,
        }
    }

    fn values(self) -> [Cplx; 2] {
        let Self { a, b, c, d } = self;
        let delta = (a - d) / 2.0;
        let cross = b.abs().sqrt() * c.abs().sqrt();
        let largest = delta.abs().max(cross);
        if largest == 0.0 {
            return [Cplx::new(a, 0.0), Cplx::new(d, 0.0)];
        }
        // Use a power of two so scaling preserves the bits that distinguish
        // nearly repeated roots. Multiply b and c before rounding the sum.
        let exponent = ((largest.to_bits() >> 52) & 0x7ff) as i32 - 1023;
        let scale = 2.0_f64.powi(exponent.max(-1022));
        let x = delta / scale;
        let (b_scaled, c_scaled) = (b / scale, c / scale);
        let product = b_scaled * c_scaled;
        let discriminant = if product.is_finite() && (product != 0.0 || cross == 0.0) {
            x.mul_add(x, product) + b_scaled.mul_add(c_scaled, -product)
        } else {
            let y = (cross / scale).copysign(if b.is_sign_negative() == c.is_sign_negative() {
                1.0
            } else {
                -1.0
            });
            x.mul_add(x, y * y.abs())
        };
        let root = scale * discriminant.abs().sqrt();
        if discriminant < 0.0 {
            let mid = (a + d) / 2.0;
            [Cplx::new(mid, root), Cplx::new(mid, -root)]
        } else {
            let gap = delta + root.copysign(delta);
            let correction = if gap == 0.0 { 0.0 } else { b * (c / gap) };
            [
                Cplx::new(a + correction, 0.0),
                Cplx::new(d - correction, 0.0),
            ]
        }
    }

    fn null_vector(self, value: Cplx) -> (Cplx, Cplx) {
        let x = (Cplx::new(self.b, 0.0), sub(value, Cplx::new(self.a, 0.0)));
        let y = (sub(value, Cplx::new(self.d, 0.0)), Cplx::new(self.c, 0.0));
        if abs(x.0).max(abs(x.1)) >= abs(y.0).max(abs(y.1)) {
            x
        } else {
            y
        }
    }
}

fn schur_vector(h: &[f64], n: usize, i: usize, width: usize, value: Cplx, x: &mut [Cplx]) {
    x.fill(Cplx::default());
    if width == 1 {
        x[i].re = 1.0;
    } else {
        (x[i], x[i + 1]) = pair(h, n, i).null_vector(value);
        normalize(&mut x[i..i + width]);
    }
    let end = i + width;
    let mut row = i;
    while row > 0 {
        row -= 1;
        let sum =
            |r, first| (first..end).fold(Cplx::default(), |s, j| sub(s, times(x[j], h[j * n + r])));
        if row > 0 && h[(row - 1) * n + row] != 0.0 {
            let p = row - 1;
            let Block { a, b, c, d } = pair(h, n, p);
            let rhs1 = sum(p, row + 1);
            let rhs2 = sum(row, row + 1);
            let a = sub(Cplx::new(a, 0.0), value);
            let d = sub(Cplx::new(d, 0.0), value);
            let scale = abs(a)
                .max(abs(d))
                .max(b.abs())
                .max(c.abs())
                .max(f64::MIN_POSITIVE);
            let scaled = |x: Cplx| Cplx::new(x.re / scale, x.im / scale);
            let (a, d, b, c) = (scaled(a), scaled(d), b / scale, c / scale);
            let det = divisor(
                sub(mul(a, d), Cplx::new(b * c, 0.0)),
                f64::EPSILON * f64::EPSILON,
            );
            // The common scale of this solve also applies to the known entries.
            x[p] = div(sub(mul(d, rhs1), times(rhs2, b)), det);
            x[row] = div(sub(mul(a, rhs2), times(rhs1, c)), det);
            for v in &mut x[row + 1..end] {
                *v = times(*v, scale);
            }
            row = p;
        } else {
            let diag = h[row * (n + 1)];
            let floor = (f64::EPSILON * diag.abs().max(abs(value))).max(f64::MIN_POSITIVE);
            let den = divisor(sub(Cplx::new(diag, 0.0), value), floor);
            let scale = abs(den).min(1.0);
            x[row] = div(sum(row, row + 1), Cplx::new(den.re / scale, den.im / scale));
            for v in &mut x[row + 1..end] {
                *v = times(*v, scale);
            }
        }
        normalize(&mut x[row..end]);
    }
}

fn normalize(x: &mut [Cplx]) {
    let max = x.iter().map(|&x| abs(x)).fold(0.0, f64::max);
    if max != 0.0 {
        for x in x {
            *x = Cplx::new(x.re / max, x.im / max);
        }
    }
}

fn divisor(x: Cplx, floor: f64) -> Cplx {
    if abs(x) < floor {
        Cplx::new(floor, 0.0)
    } else {
        x
    }
}

fn abs(x: Cplx) -> f64 {
    x.re.hypot(x.im)
}
fn add(x: Cplx, y: Cplx) -> Cplx {
    Cplx::new(x.re + y.re, x.im + y.im)
}
fn sub(x: Cplx, y: Cplx) -> Cplx {
    Cplx::new(x.re - y.re, x.im - y.im)
}
fn conj(x: Cplx) -> Cplx {
    Cplx::new(x.re, -x.im)
}
fn times(x: Cplx, k: f64) -> Cplx {
    Cplx::new(x.re * k, x.im * k)
}
fn mul(x: Cplx, y: Cplx) -> Cplx {
    Cplx::new(x.re * y.re - x.im * y.im, x.re * y.im + x.im * y.re)
}
fn div(x: Cplx, y: Cplx) -> Cplx {
    let scale = y.re.abs().max(y.im.abs());
    let (a, b) = (y.re / scale, y.im / scale);
    let den = a * a + b * b;
    let (c, d) = (x.re / den, x.im / den);
    Cplx::new((c * a + d * b) / scale, (d * a - c * b) / scale)
}

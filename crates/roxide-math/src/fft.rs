//! Mixed-radix Fourier transforms and transform-length selection.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod lengths;

use std::f64::consts::TAU;

use crate::{Cplx, MathError};

pub use lengths::{NextLengthError, nextn};

const RADICES: [usize; 4] = [4, 2, 3, 5];

fn product(a: Cplx, b: Cplx) -> Cplx {
    Cplx::new(a.re * b.re - a.im * b.im, a.re * b.im + a.im * b.re)
}

fn times_root(z: Cplx, w: Cplx) -> Cplx {
    if w.im == 0.0 {
        Cplx::new(z.re * w.re, z.im * w.re)
    } else if w.re == 0.0 {
        Cplx::new(-z.im * w.im, z.re * w.im)
    } else {
        product(z, w)
    }
}

fn sum(a: Cplx, b: Cplx) -> Cplx {
    Cplx::new(a.re + b.re, a.im + b.im)
}

fn root(k: usize, n: usize, sign: f64) -> Cplx {
    if k == 0 {
        return Cplx::new(1.0, 0.0);
    }
    if n.is_multiple_of(2) && k == n / 2 {
        return Cplx::new(-1.0, 0.0);
    }
    if n.is_multiple_of(4) {
        if k == n / 4 {
            return Cplx::new(0.0, sign);
        }
        if k == n / 4 * 3 {
            return Cplx::new(0.0, -sign);
        }
    }
    let angle = if k <= n / 2 {
        sign * TAU * (k as f64 / n as f64)
    } else {
        -sign * TAU * ((n - k) as f64 / n as f64)
    };
    let (im, re) = angle.sin_cos();
    Cplx::new(re, im)
}

/// Transform a complex vector in place. The inverse changes the phase sign.
///
/// Both transforms are unnormalized. The limit is in complex elements per buffer.
pub fn fft(data: &mut [Cplx], inverse: bool, limit: usize) -> Result<(), MathError> {
    if data.is_empty() {
        return Ok(());
    }
    Plan::new(data.len(), limit)?.apply(data, inverse);
    Ok(())
}

/// Transform each axis of a column-major array in increasing axis order.
pub fn fft_nd(
    data: &mut [Cplx],
    dimensions: &[usize],
    inverse: bool,
    limit: usize,
) -> Result<(), MathError> {
    let len = dimensions
        .iter()
        .try_fold(1usize, |n, &d| n.checked_mul(d))
        .ok_or(MathError::TooLarge)?;
    if len != data.len() {
        return Err(MathError::Dim);
    }
    if len == 0 {
        return Ok(());
    }
    if len > limit {
        return Err(MathError::TooLarge);
    }
    let mut stride = 1;
    for &axis in dimensions {
        fft_axis(data, axis, stride, inverse, limit)?;
        stride *= axis;
    }
    Ok(())
}

/// Transform lines of the given length and element stride.
pub fn fft_axis(
    data: &mut [Cplx],
    length: usize,
    stride: usize,
    inverse: bool,
    limit: usize,
) -> Result<(), MathError> {
    if data.is_empty() {
        return Ok(());
    }
    let block = length.checked_mul(stride).ok_or(MathError::TooLarge)?;
    if block == 0 || !data.len().is_multiple_of(block) {
        return Err(MathError::Dim);
    }
    let mut plan = Plan::new(length, limit)?;
    if stride == 1 {
        for line in data.chunks_exact_mut(length) {
            plan.apply(line, inverse);
        }
    } else {
        let mut line = vec![Cplx::default(); length];
        for block in data.chunks_exact_mut(block) {
            for i in 0..stride {
                let entries = block[i..].iter_mut().step_by(stride);
                for (v, z) in line.iter_mut().zip(entries) {
                    *v = *z;
                }
                plan.apply(&mut line, inverse);
                for (v, z) in line.iter().zip(block[i..].iter_mut().step_by(stride)) {
                    *z = *v;
                }
            }
        }
    }
    Ok(())
}

struct Plan {
    scratch: Vec<Cplx>,
    step: Step,
}

enum Step {
    One,
    Split {
        radix: usize,
        roots: Vec<Cplx>,
        child: Box<Step>,
    },
    Chirp {
        phase: Vec<Cplx>,
        spectrum: Vec<Cplx>,
        work: Vec<Cplx>,
    },
}

impl Plan {
    fn new(n: usize, limit: usize) -> Result<Self, MathError> {
        let limit = limit.min(isize::MAX as usize / size_of::<Cplx>());
        if n > limit {
            return Err(MathError::TooLarge);
        }
        Ok(Self {
            step: Step::new(n, limit)?,
            scratch: vec![Cplx::default(); n],
        })
    }

    fn apply(&mut self, data: &mut [Cplx], inverse: bool) {
        if inverse {
            for z in data.iter_mut() {
                z.im = -z.im;
            }
        }
        self.step.forward(data, &mut self.scratch, 1, 0);
        for (z, v) in data.iter_mut().zip(&self.scratch) {
            *z = *v;
            if inverse {
                z.im = -z.im;
            }
        }
    }
}

impl Step {
    fn new(n: usize, limit: usize) -> Result<Self, MathError> {
        if n == 1 {
            return Ok(Self::One);
        }
        if let Some(radix) = RADICES.into_iter().find(|&r| n.is_multiple_of(r)) {
            return Ok(Self::Split {
                radix,
                roots: (0..n).map(|k| root(k, n, -1.0)).collect(),
                child: Box::new(Self::new(n / radix, limit)?),
            });
        }
        let twice = n.checked_mul(2).ok_or(MathError::TooLarge)?;
        let len = (twice - 1)
            .checked_next_power_of_two()
            .filter(|&len| len <= limit)
            .ok_or(MathError::TooLarge)?;
        let phase: Vec<_> = (0..n)
            .map(|k| {
                root(
                    ((k as u128 * k as u128) % twice as u128) as usize,
                    twice,
                    -1.0,
                )
            })
            .collect();
        let mut spectrum = vec![Cplx::default(); len];
        spectrum[0] = phase[0];
        for (i, p) in phase.iter().enumerate().skip(1) {
            let z = Cplx::new(p.re, -p.im);
            spectrum[i] = z;
            spectrum[len - i] = z;
        }
        radix_two(&mut spectrum, false);
        Ok(Self::Chirp {
            phase,
            spectrum,
            work: vec![Cplx::default(); len],
        })
    }

    fn forward(&mut self, input: &[Cplx], output: &mut [Cplx], stride: usize, offset: usize) {
        match self {
            Self::One => output[0] = input[offset],
            Self::Split {
                radix,
                roots,
                child,
            } => {
                let n = output.len();
                let m = n / *radix;
                for (j, chunk) in output.chunks_exact_mut(m).enumerate() {
                    child.forward(input, chunk, stride * *radix, offset + j * stride);
                }
                for k in 0..m {
                    let mut lane = [Cplx::default(); 5];
                    for (j, value) in lane[..*radix].iter_mut().enumerate() {
                        *value = output[k + j * m];
                    }
                    for i in (k..n).step_by(m) {
                        let mut value = lane[0];
                        for (j, &z) in lane[..*radix].iter().enumerate().skip(1) {
                            let w = roots[(i * j) % n];
                            value = sum(value, times_root(z, w));
                        }
                        output[i] = value;
                    }
                }
            }
            Self::Chirp {
                phase,
                spectrum,
                work,
            } => {
                let mut lane = input[offset..].iter().step_by(stride).take(output.len());
                let first = input[offset];
                if lane.clone().skip(1).all(|&z| z == Cplx::default()) {
                    output.fill(first);
                    return;
                }
                let (scale, nonfinite) =
                    lane.clone()
                        .fold((0.0_f64, false), |(scale, nonfinite), z| {
                            (
                                scale.max(z.re.abs()).max(z.im.abs()),
                                nonfinite || !z.re.is_finite() || !z.im.is_finite(),
                            )
                        });
                let scale = if scale.is_finite() && scale > 0.0 {
                    scale
                } else {
                    1.0
                };
                let dc = nonfinite.then(|| lane.clone().copied().fold(Cplx::default(), sum));
                work.fill(Cplx::default());
                for ((&p, z), v) in phase.iter().zip(work.iter_mut()).zip(&mut lane) {
                    *z = times_root(Cplx::new(v.re / scale, v.im / scale), p);
                }
                radix_two(work, false);
                for (a, &b) in work.iter_mut().zip(spectrum.iter()) {
                    *a = product(*a, b);
                }
                radix_two(work, true);
                let length = work.len() as f64;
                for ((z, &p), v) in output.iter_mut().zip(phase.iter()).zip(work) {
                    let w = times_root(Cplx::new(v.re / length, v.im / length), p);
                    *z = Cplx::new(w.re * scale, w.im * scale);
                }
                if let Some(dc) = dc {
                    output[0] = dc;
                }
            }
        }
    }
}

fn radix_two(data: &mut [Cplx], inverse: bool) {
    let n = data.len();
    let bits = n.trailing_zeros();
    for i in 0..n {
        let j = i.reverse_bits() >> (usize::BITS - bits);
        if j > i {
            data.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    for power in 1..=bits {
        let width = 1 << power;
        for k in 0..width / 2 {
            let w = root(k, width, sign);
            for block in data.chunks_exact_mut(width) {
                let a = block[k];
                let b = times_root(block[k + width / 2], w);
                block[k] = sum(a, b);
                block[k + width / 2] = Cplx::new(a.re - b.re, a.im - b.im);
            }
        }
    }
}

//! Brent's bracketed scalar root finder.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Result of a bracketed root search.
#[derive(Debug, Clone, Copy)]
pub struct ZeroinOut {
    /// Best root estimate.
    pub root: f64,
    /// Function value at the root estimate.
    pub f_root: f64,
    /// Number of search iterations.
    pub iter: u32,
    /// Distance to the other bracket endpoint.
    pub estim_prec: f64,
    /// Whether the search found a zero or met the tolerance.
    pub converged: bool,
}

/// Find a zero from a bracket and its precomputed function values.
///
/// The endpoint values must have opposite signs unless one endpoint is a zero.
pub fn zeroin<E>(
    mut f: impl FnMut(f64) -> Result<f64, E>,
    mut a: f64,
    mut b: f64,
    mut fa: f64,
    mut fb: f64,
    tol: f64,
    maxit: u32,
) -> Result<ZeroinOut, E> {
    for (root, f_root) in [(a, fa), (b, fb)] {
        if f_root == 0.0 {
            return Ok(ZeroinOut {
                root,
                f_root,
                iter: 0,
                estim_prec: 0.0,
                converged: true,
            });
        }
    }
    let mut c = a;
    let mut fc = fa;
    for iter in 0..=maxit.saturating_add(1) {
        let prev_step = b - a;
        if fc.abs() < fb.abs() {
            (a, fa, b, fb, c, fc) = (b, fb, c, fc, b, fb);
        }
        let delta = 2.0 * f64::EPSILON * b.abs() + tol * 0.5;
        let bisect = c * 0.5 - b * 0.5;
        if bisect.abs() <= delta || fb == 0.0 || iter > maxit {
            return Ok(ZeroinOut {
                root: b,
                f_root: fb,
                iter: iter.min(maxit),
                estim_prec: (c - b).abs(),
                converged: bisect.abs() <= delta || fb == 0.0,
            });
        }
        let mut step = bisect;
        if prev_step.abs() >= delta && fa.abs() > fb.abs() {
            let s = fb / fa;
            let (mut p, mut q) = if a == c {
                (2.0 * bisect * s, 1.0 - s)
            } else {
                let q = fa / fc;
                let r = fb / fc;
                (
                    s * (2.0 * bisect * q * (q - r) - (b - a) * (r - 1.0)),
                    (q - 1.0) * (r - 1.0) * (s - 1.0),
                )
            };
            if p > 0.0 {
                q = -q;
            } else {
                p = -p;
            }
            if p < (0.75 * (c - b) * q - (delta * q).abs()) && p < (prev_step * q * 0.5).abs() {
                step = p / q;
            }
        }
        (a, fa) = (b, fb);
        b += step.abs().max(delta).copysign(step);
        fb = f(b)?;
        if fb.is_sign_positive() == fc.is_sign_positive() {
            (c, fc) = (a, fa);
        }
    }
    Ok(ZeroinOut {
        root: b,
        f_root: fb,
        iter: maxit,
        estim_prec: (c - b).abs(),
        converged: false,
    })
}

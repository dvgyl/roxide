//! Brent's bounded scalar minimizer.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Minimize a scalar objective on `[lower, upper]` with an absolute tolerance.
///
/// The caller validates the bounds and tolerance. Callback errors propagate to the caller.
pub fn fmin<E>(
    mut f: impl FnMut(f64) -> Result<f64, E>,
    mut lower: f64,
    mut upper: f64,
    tol: f64,
) -> Result<(f64, f64), E> {
    let golden = (3.0 - 5.0_f64.sqrt()) / 2.0;
    let mut x = lower + golden * (upper - lower);
    let mut w = x;
    let mut v = x;
    let mut fx = f(x)?;
    let mut fw = fx;
    let mut fv = fx;
    let mut step: f64 = 0.0;
    let mut span: f64 = 0.0;
    for _ in 0..4096 {
        let mid = lower * 0.5 + upper * 0.5;
        let delta = f64::EPSILON.sqrt() * x.abs() + tol / 3.0;
        if (x - mid).abs() <= 2.0 * delta - (upper - lower) * 0.5 {
            break;
        }
        let mut parabolic = false;
        if span.abs() > delta {
            let r = (x - w) * (fx - fv);
            let q = (x - v) * (fx - fw);
            let denominator = 2.0 * (q - r);
            let numerator = ((x - v) * q - (x - w) * r) * -denominator.signum();
            let q = denominator.abs();
            let previous_span = span;
            span = step;
            if numerator.abs() < (0.5 * q * previous_span).abs()
                && numerator > q * (lower - x)
                && numerator < q * (upper - x)
            {
                step = numerator / q;
                if x + step - lower < 2.0 * delta || upper - x - step < 2.0 * delta {
                    step = delta.copysign(mid - x);
                }
                parabolic = true;
            }
        }
        if !parabolic {
            span = if x < mid { upper - x } else { lower - x };
            step = golden * span;
        }
        let u = x + step.abs().max(delta).copysign(step);
        if u == x {
            break;
        }
        let fu = f(u)?;
        if fu <= fx {
            if u < x {
                upper = x;
            } else {
                lower = x;
            }
            (v, fv) = (w, fw);
            (w, fw) = (x, fx);
            (x, fx) = (u, fu);
        } else {
            if u < x {
                lower = u;
            } else {
                upper = u;
            }
            if fu <= fw || w == x {
                (v, fv) = (w, fw);
                (w, fw) = (u, fu);
            } else if fu <= fv || v == x || v == w {
                (v, fv) = (u, fu);
            }
        }
    }
    Ok((x, fx))
}

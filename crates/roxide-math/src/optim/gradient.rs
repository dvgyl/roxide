use super::{Control, OptimOut, Progress, dot, move_to, unchanged};
use crate::Objective;

const REDUCTION: f64 = 0.2;
const ARMIJO: f64 = 0.0001;

/// Minimize with the Broyden–Fletcher–Goldfarb–Shanno inverse Hessian update.
pub fn bfgs<F: Objective>(f: &mut F, par: Vec<f64>, c: Control) -> Result<OptimOut, F::Err> {
    bfgs_with_report(f, par, c, |_, _| {})
}

/// Minimize by BFGS and report progress after each iteration.
pub fn bfgs_with_report<F: Objective>(
    f: &mut F,
    par: Vec<f64>,
    c: Control,
    mut report: impl FnMut(&mut F, Progress<'_>),
) -> Result<OptimOut, F::Err> {
    let mut out = OptimOut::new(par, true);
    let mut x = out.par.clone();
    out.value = out.value(f, &x)?;
    if c.maxit == 0 || !out.value.is_finite() {
        out.fncount = u32::from(c.maxit != 0);
        return Ok(out);
    }
    report(f, Progress::Initial(out.value));
    let n = x.len();
    let mut direction = vec![0.0; n];
    let mut next = direction.clone();
    let mut g = direction.clone();
    let mut old_g = direction.clone();
    let mut inverse = vec![0.0; n.saturating_mul(n)];
    let mut update_at = 1;
    let mut iteration = 1;
    out.gradient(f, &x, &mut g)?;
    loop {
        let count = out.grcount.unwrap_or(0);
        if update_at == count {
            inverse.fill(0.0);
            for i in 0..n {
                inverse[i * n + i] = 1.0;
            }
        }
        for (i, d) in direction.iter_mut().enumerate() {
            *d = -dot(&inverse[i * n..(i + 1) * n], &g);
        }
        let slope = dot(&direction, &g);
        let mut step = 1.0;
        let mut value = out.value;
        if slope < 0.0 {
            loop {
                move_to(&mut next, &x, &direction, step);
                if unchanged(&next, &x) {
                    break;
                }
                value = out.value(f, &next)?;
                if value.is_finite() && value <= (ARMIJO * step).mul_add(slope, out.value) {
                    break;
                }
                step *= REDUCTION;
            }
            let mut progress = !unchanged(&next, &x);
            if value <= c.abstol
                || (value - out.value).abs() <= c.reltol * (out.value.abs() + c.reltol)
            {
                x.copy_from_slice(&next);
                out.value = value;
                progress = false;
            }
            if progress {
                old_g.copy_from_slice(&g);
                out.gradient(f, &next, &mut g)?;
                iteration += 1;
                out.value = value;
                for i in 0..n {
                    direction[i] *= step;
                    old_g[i] = g[i] - old_g[i];
                }
                x.copy_from_slice(&next);
                let curvature = dot(&direction, &old_g);
                if curvature > 0.0 {
                    for (i, v) in next.iter_mut().enumerate() {
                        *v = dot(&inverse[i * n..(i + 1) * n], &old_g);
                    }
                    let scale = 1.0 + dot(&old_g, &next) / curvature;
                    for i in 0..n {
                        for j in 0..=i {
                            let change = (scale * direction[i] * direction[j]
                                - direction[i] * next[j]
                                - next[i] * direction[j])
                                / curvature;
                            inverse[i * n + j] += change;
                            inverse[j * n + i] = inverse[i * n + j];
                        }
                    }
                } else {
                    update_at = out.grcount.unwrap_or(0);
                }
            } else if update_at == count {
                report(f, Progress::Iteration(iteration, out.value));
                break;
            } else {
                update_at = count;
            }
        } else if update_at == count {
            break;
        } else {
            update_at = count;
        }
        report(f, Progress::Iteration(iteration, out.value));
        if iteration >= c.maxit {
            break;
        }
        let count = out.grcount.unwrap_or(0);
        if count - update_at > (2 * n) as u32 {
            update_at = count;
        }
    }
    report(f, Progress::Finish(iteration));
    out.convergence = i32::from(iteration >= c.maxit);
    out.par = x;
    Ok(out)
}

/// Formula for the conjugate direction update.
#[derive(Debug, Clone, Copy)]
pub enum CgFormula {
    /// Fletcher–Reeves.
    FletcherReeves,
    /// Polak–Ribière.
    PolakRibiere,
    /// Beale–Sorenson.
    BealeSorenson,
}

/// Minimize by restarted nonlinear conjugate gradients.
pub fn cg<F: Objective>(
    f: &mut F,
    par: Vec<f64>,
    c: Control,
    formula: CgFormula,
) -> Result<OptimOut, F::Err> {
    cg_with_report(f, par, c, formula, |_, _| {})
}

/// Minimize by conjugate gradients and report progress after each search.
pub fn cg_with_report<F: Objective>(
    f: &mut F,
    par: Vec<f64>,
    c: Control,
    formula: CgFormula,
    mut report: impl FnMut(&mut F, Progress<'_>),
) -> Result<OptimOut, F::Err> {
    let mut out = OptimOut::new(par, true);
    let mut x = out.par.clone();
    out.value = out.value(f, &x)?;
    if c.maxit == 0 || !out.value.is_finite() {
        out.fncount = u32::from(c.maxit != 0);
        return Ok(out);
    }
    let n = x.len();
    let threshold = c.reltol * (n as f64) * c.reltol.sqrt();
    let mut direction = vec![0.0; n];
    let mut next = direction.clone();
    let mut g = direction.clone();
    let mut old_g = direction.clone();
    let mut difference = direction.clone();
    report(f, Progress::CgTolerance(threshold));
    report(f, Progress::CgStep(false, 0, out.fncount, out.value, &x));
    loop {
        direction.fill(0.0);
        let mut step = 1.0;
        for k in 0..n.max(1) {
            out.grcount = out.grcount.map(|count| count.saturating_add(1));
            if out.grcount.unwrap_or(0) > c.maxit {
                out.convergence = 1;
                return Ok(out);
            }
            f.gradient(&x, &mut g)?;
            let norm = dot(&g, &g);
            for i in 0..n {
                difference[i] = g[i] - old_g[i];
            }
            let (numerator, denominator) = match formula {
                CgFormula::FletcherReeves => (norm, dot(&old_g, &old_g)),
                CgFormula::PolakRibiere => (dot(&g, &difference), dot(&old_g, &old_g)),
                CgFormula::BealeSorenson => (dot(&g, &difference), dot(&direction, &difference)),
            };
            if numerator <= threshold || out.value <= c.abstol {
                out.par = x;
                report(f, Progress::Finish(out.fncount));
                return Ok(out);
            }
            let beta = if k == 0 || denominator == 0.0 {
                0.0
            } else {
                numerator / denominator
            };
            for i in 0..n {
                direction[i] = beta.mul_add(direction[i], -g[i]);
            }
            old_g.copy_from_slice(&g);
            out.par.copy_from_slice(&x);
            let slope = dot(&g, &direction);
            if slope >= 0.0 {
                break;
            }
            let accepted = loop {
                move_to(&mut next, &x, &direction, step);
                if unchanged(&next, &x) {
                    break false;
                }
                let value = out.value(f, &next)?;
                if value.is_finite() && value <= (ARMIJO * step).mul_add(slope, out.value) {
                    out.value = value;
                    break true;
                }
                step *= REDUCTION;
            };
            if !accepted {
                if k == 0 {
                    return Ok(out);
                }
                break;
            }
            let trial = step * 0.5;
            let mut trial_x = vec![0.0; n];
            move_to(&mut trial_x, &x, &direction, trial);
            let value = out.value(f, &trial_x)?;
            if value < out.value {
                out.value = value;
                next = trial_x;
            }
            x.copy_from_slice(&next);
            report(
                f,
                Progress::CgStep(k == 0, out.grcount.unwrap_or(0), out.fncount, out.value, &x),
            );
            step = (step * 1.7).min(1.0);
        }
    }
}

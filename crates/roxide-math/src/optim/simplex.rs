use super::{Control, OptimOut, Progress};
use crate::Objective;

/// Controls for the Nelder–Mead simplex transformations.
#[derive(Debug, Clone, Copy)]
pub struct SimplexControl {
    /// Evaluation limit and stopping tolerances.
    pub common: Control,
    /// Reflection multiplier.
    pub alpha: f64,
    /// Contraction and shrink multiplier.
    pub beta: f64,
    /// Expansion multiplier.
    pub gamma: f64,
}

/// Minimize by reflecting, expanding, contracting, and shrinking a simplex.
pub fn nelder_mead<F: Objective>(
    f: &mut F,
    par: Vec<f64>,
    control: SimplexControl,
) -> Result<OptimOut, F::Err> {
    nelder_mead_with_report(f, par, control, |_, _| {})
}

/// Minimize by Nelder–Mead and report progress at simplex transformations.
pub fn nelder_mead_with_report<F: Objective>(
    f: &mut F,
    par: Vec<f64>,
    control: SimplexControl,
    mut report: impl FnMut(&mut F, Progress<'_>),
) -> Result<OptimOut, F::Err> {
    let mut out = OptimOut::new(par, false);
    let mut value = f.value(&out.par)?;
    out.value = value;
    let c = control.common;
    let n = out.par.len();
    if c.maxit == 0 || n == 0 || !value.is_finite() || out.par.iter().any(|x| !x.is_finite()) {
        out.fncount = u32::from(c.maxit != 0);
        return Ok(out);
    }
    let tolerance = c.reltol * (value.abs() + c.reltol);
    out.fncount = 1;
    let initial = 0.1 * out.par.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()));
    let initial = if initial == 0.0 { 0.1 } else { initial };
    let finite = |v: f64| if v.is_finite() { v } else { 1e35 };
    report(f, Progress::Initial(value));
    report(f, Progress::SimplexSetup(tolerance, initial));
    let mut operation = "BUILD";
    let mut simplex = vec![out.par.clone()];
    let mut values = vec![value];
    let mut size = 0.0;
    for i in 0..n {
        let mut x = out.par.clone();
        let mut step = initial;
        for _ in 0..32 {
            if out.par[i] + step != out.par[i] {
                break;
            }
            step *= 10.0;
        }
        x[i] += step;
        size += step * 10.0;
        values.push(finite(out.value(f, &x)?));
        simplex.push(x);
    }
    let mut best = 0;
    let mut center = vec![0.0; n];
    let mut trial = center.clone();
    loop {
        let mut worst = best;
        for i in 0..values.len() {
            if values[i] < values[best] {
                best = i;
            }
            if values[i] > values[worst] {
                worst = i;
            }
        }
        value = values[best];
        out.par.copy_from_slice(&simplex[best]);
        out.value = value;
        if values[worst] <= value + tolerance || value <= c.abstol {
            break;
        }
        report(
            f,
            Progress::SimplexStep(operation, out.fncount, values[worst], value),
        );
        center.fill(0.0);
        for (i, x) in simplex.iter().enumerate() {
            if i != worst {
                for (sum, x) in center.iter_mut().zip(x) {
                    *sum += x;
                }
            }
        }
        center.iter_mut().for_each(|x| *x /= n as f64);
        for ((r, &m), &x) in trial.iter_mut().zip(&center).zip(&simplex[worst]) {
            *r = (1.0 + control.alpha).mul_add(m, -control.alpha * x);
        }
        let reflected = finite(out.value(f, &trial)?);
        if reflected < value {
            let expanded: Vec<_> = center
                .iter()
                .zip(&trial)
                .map(|(&m, &r)| control.gamma.mul_add(r, (1.0 - control.gamma) * m))
                .collect();
            let expanded_value = finite(out.value(f, &expanded)?);
            (simplex[worst], values[worst]) = if expanded_value < reflected {
                operation = "EXTENSION";
                (expanded, expanded_value)
            } else {
                operation = "REFLECTION";
                (trial.clone(), reflected)
            };
        } else {
            let old_worst = values[worst];
            operation = "HI-REDUCTION";
            if reflected < old_worst {
                operation = "LO-REDUCTION";
                simplex[worst].copy_from_slice(&trial);
                values[worst] = reflected;
            }
            for ((r, &m), &x) in trial.iter_mut().zip(&center).zip(&simplex[worst]) {
                *r = control.beta.mul_add(x, (1.0 - control.beta) * m);
            }
            let contracted = finite(out.value(f, &trial)?);
            if contracted < values[worst] {
                simplex[worst].copy_from_slice(&trial);
                values[worst] = contracted;
            } else if reflected >= old_worst {
                operation = "SHRINK";
                let anchor = simplex[best].clone();
                let mut next_size = 0.0;
                for (i, x) in simplex.iter_mut().enumerate() {
                    if i != best {
                        for (x, &m) in x.iter_mut().zip(&anchor) {
                            *x = control.beta.mul_add(*x, (1.0 - control.beta) * m);
                            next_size += (*x - m).abs();
                        }
                    }
                }
                if next_size >= size {
                    report(f, Progress::SimplexDegenerate);
                    out.convergence = 10;
                    break;
                }
                size = next_size;
                for (i, x) in simplex.iter().enumerate() {
                    if i != best {
                        values[i] = finite(out.value(f, x)?);
                    }
                }
            }
        }
        if out.fncount > c.maxit {
            out.convergence = 1;
            break;
        }
    }
    report(f, Progress::Finish(out.fncount));
    Ok(out)
}

//! Multidimensional optimization and finite-difference Hessians.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod bounded;
mod gradient;
mod simplex;

use crate::Objective;

pub use bounded::{BoxControl, lbfgsb, lbfgsb_with_report};
pub use gradient::{CgFormula, bfgs, bfgs_with_report, cg, cg_with_report};
pub use simplex::{SimplexControl, nelder_mead, nelder_mead_with_report};

/// Controls shared by the unconstrained deterministic optimizers.
#[derive(Debug, Clone, Copy)]
pub struct Control {
    /// Iteration or evaluation limit, according to the method.
    pub maxit: u32,
    /// Absolute objective threshold.
    pub abstol: f64,
    /// Relative stopping tolerance.
    pub reltol: f64,
}

/// Multidimensional optimization result.
#[derive(Debug, Clone)]
pub struct OptimOut {
    /// Estimated optimum.
    pub par: Vec<f64>,
    /// Objective at the solution or the last accepted step.
    pub value: f64,
    /// Objective evaluations, excluding calls for numerical derivatives.
    pub fncount: u32,
    /// Gradient evaluations, when applicable.
    pub grcount: Option<u32>,
    /// Method-specific convergence code.
    pub convergence: i32,
    /// Method-specific diagnostic.
    pub message: Option<String>,
}

impl OptimOut {
    fn new(par: Vec<f64>, gradient: bool) -> Self {
        Self {
            par,
            value: f64::NAN,
            fncount: 0,
            grcount: gradient.then_some(0),
            convergence: 0,
            message: None,
        }
    }

    fn gradient<F: Objective>(
        &mut self,
        f: &mut F,
        x: &[f64],
        g: &mut [f64],
    ) -> Result<(), F::Err> {
        self.grcount = self.grcount.map(|count| count.saturating_add(1));
        f.gradient(x, g)
    }

    fn value<F: Objective>(&mut self, f: &mut F, x: &[f64]) -> Result<f64, F::Err> {
        self.fncount = self.fncount.saturating_add(1);
        f.value(x)
    }
}

fn dot(x: &[f64], y: &[f64]) -> f64 {
    x.iter().zip(y).fold(0.0, |sum, (&x, &y)| x.mul_add(y, sum))
}

fn move_to(out: &mut [f64], x: &[f64], direction: &[f64], step: f64) {
    for ((new, &x), &d) in out.iter_mut().zip(x).zip(direction) {
        *new = step.mul_add(d, x);
    }
}

fn unchanged(x: &[f64], y: &[f64]) -> bool {
    x.iter().zip(y).all(|(a, b)| 10.0 + a == 10.0 + b)
}

/// Progress reported by the optimization methods in evaluation order.
#[derive(Debug, Clone, Copy)]
pub enum Progress<'a> {
    /// The initial objective value is available.
    Initial(f64),
    /// Initial simplex tolerance and edge length.
    SimplexSetup(f64, f64),
    /// Simplex operation, evaluation count, worst value, and best value.
    SimplexStep(&'static str, u32, f64, f64),
    /// A simplex shrink did not reduce its size.
    SimplexDegenerate,
    /// The gradient threshold used by conjugate gradients.
    CgTolerance(f64),
    /// Conjugate-gradient restart, iteration, evaluations, objective, and point.
    CgStep(bool, u32, u32, f64, &'a [f64]),
    /// Completed iteration and current objective.
    Iteration(u32, f64),
    /// The method stopped after this number of iterations.
    Finish(u32),
}

/// Random draws and proposals for simulated annealing.
pub trait Anneal: Objective {
    /// Propose a point at the current temperature relative to the initial temperature.
    fn candidate(&mut self, x: &[f64], scale: f64, next: &mut [f64]) -> Result<(), Self::Err>;

    /// Draw a uniform value in `(0, 1)`.
    fn uniform(&mut self) -> Result<f64, Self::Err>;
}

/// Minimize with Bélisle's logarithmic cooling schedule.
pub fn sann<F: Anneal>(
    f: &mut F,
    par: Vec<f64>,
    maxit: u32,
    temp: f64,
    tmax: u32,
) -> Result<OptimOut, F::Err> {
    sann_with_report(f, par, maxit, temp, tmax, |_, _| {})
}

/// Minimize by simulated annealing and report progress at temperature changes.
pub fn sann_with_report<F: Anneal>(
    f: &mut F,
    par: Vec<f64>,
    maxit: u32,
    temp: f64,
    tmax: u32,
    mut report: impl FnMut(&mut F, Progress<'_>),
) -> Result<OptimOut, F::Err> {
    let mut out = OptimOut::new(par, false);
    let mut current = out.par.clone();
    let mut next = current.clone();
    let finite = |v: f64| if v.is_finite() { v } else { 1e35 };
    let mut value = finite(out.value(f, &current)?);
    out.value = value;
    report(f, Progress::Initial(value));
    if maxit == 0 || current.is_empty() {
        report(f, Progress::Finish(0));
        out.fncount = u32::from(maxit != 0);
        return Ok(out);
    }
    let tmax = tmax.max(1);
    // Preserve the decimal offset measured from the first proposal in GNU R.
    const SCHEDULE_OFFSET: f64 = 27_182_818.0 / 10_000_000.0;
    for i in 1..maxit {
        let scale = (f64::from((i - 1) / tmax * tmax) + SCHEDULE_OFFSET)
            .ln()
            .recip();
        f.candidate(&current, scale, &mut next)?;
        let candidate = finite(out.value(f, &next)?);
        let change = candidate - value;
        if change <= 0.0 || f.uniform()? < (-change / (temp * scale)).exp() {
            current.copy_from_slice(&next);
            value = candidate;
        }
        if candidate <= out.value {
            out.par.copy_from_slice(&next);
            out.value = candidate;
        }
        if i.is_multiple_of(tmax) || i + 1 == maxit {
            report(f, Progress::Iteration(i, out.value));
        }
    }
    report(f, Progress::Finish(maxit.saturating_sub(1)));
    Ok(out)
}

/// Estimate a symmetric Hessian in column-major order using central differences.
///
/// `step` gives the perturbation for each coordinate in `par`.
pub fn optim_hess<F: Objective>(f: &mut F, par: &[f64], step: &[f64]) -> Result<Vec<f64>, F::Err> {
    let n = par.len();
    let mut hessian = vec![0.0; n.saturating_mul(n)];
    let mut x = par.to_vec();
    let mut plus = x.clone();
    let mut minus = x.clone();
    for (i, (&value, &h)) in par.iter().zip(step).enumerate() {
        x[i] = value + h;
        f.gradient(&x, &mut plus)?;
        x[i] = value - h;
        f.gradient(&x, &mut minus)?;
        x[i] = value;
        for j in 0..n {
            hessian[i * n + j] = (plus[j] - minus[j]) / (2.0 * h);
        }
    }
    for i in 0..n {
        for j in 0..i {
            let average = (hessian[i * n + j] + hessian[j * n + i]) * 0.5;
            hessian[i * n + j] = average;
            hessian[j * n + i] = average;
        }
    }
    Ok(hessian)
}

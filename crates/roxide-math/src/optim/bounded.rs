use std::collections::VecDeque;

use super::{OptimOut, Progress, dot, move_to};
use crate::Objective;

/// Controls for limited-memory BFGS with box constraints.
#[derive(Debug, Clone, Copy)]
pub struct BoxControl {
    /// Limit on completed iterations.
    pub maxit: u32,
    /// Maximum number of stored curvature pairs.
    pub lmm: usize,
    /// Relative objective reduction tolerance in units of machine epsilon.
    pub factr: f64,
    /// Infinity-norm tolerance for the projected gradient.
    pub pgtol: f64,
}

struct Pair {
    s: Vec<f64>,
    y: Vec<f64>,
    curvature: f64,
}

struct Update {
    bs: Vec<f64>,
    y: Vec<f64>,
    sbs: f64,
    sy: f64,
}

struct Hessian {
    scale: f64,
    updates: Vec<Update>,
}

impl Hessian {
    fn new(history: &VecDeque<Pair>) -> Self {
        let scale = history
            .back()
            .map_or(1.0, |pair| dot(&pair.y, &pair.y) / pair.curvature);
        let mut result = Self {
            scale,
            updates: Vec::new(),
        };
        for pair in history {
            let mut bs = pair.s.clone();
            result.apply(&pair.s, &mut bs);
            result.updates.push(Update {
                sbs: dot(&pair.s, &bs),
                bs,
                y: pair.y.clone(),
                sy: pair.curvature,
            });
        }
        result
    }

    fn apply(&self, x: &[f64], out: &mut [f64]) {
        for (v, &x) in out.iter_mut().zip(x) {
            *v = self.scale * x;
        }
        // Each Hessian update subtracts a term in Bs and adds a term in y.
        for update in &self.updates {
            let subtract = dot(&update.bs, x) / update.sbs;
            let add = dot(&update.y, x) / update.sy;
            for ((value, &s), &y) in out.iter_mut().zip(&update.bs).zip(&update.y) {
                *value = add.mul_add(y, (-subtract).mul_add(s, *value));
            }
        }
    }
}

fn cauchy(x: &[f64], g: &[f64], bounds: &[(f64, f64)], h: &Hessian) -> Vec<f64> {
    let mut direction: Vec<_> = g.iter().map(|&v| -v).collect();
    let mut breaks = Vec::new();
    for (i, ((&x, &d), &(lo, hi))) in x.iter().zip(&direction).zip(bounds).enumerate() {
        let target = if d < 0.0 { lo } else { hi };
        if d != 0.0 && target.is_finite() {
            breaks.push(((target - x) / d, i, target));
        }
    }
    breaks.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut z = x.to_vec();
    let mut displacement = vec![0.0; x.len()];
    let mut work = displacement.clone();
    let mut previous = 0.0;
    for (time, i, target) in breaks.into_iter().chain([(f64::INFINITY, 0, 0.0)]) {
        if time > previous {
            h.apply(&displacement, &mut work);
            let slope = dot(g, &direction) + dot(&work, &direction);
            h.apply(&direction, &mut work);
            let curvature = dot(&direction, &work);
            if slope >= 0.0 || curvature <= 0.0 {
                break;
            }
            let step = (-slope / curvature).min(time - previous);
            for j in 0..x.len() {
                z[j] = step.mul_add(direction[j], z[j]);
                displacement[j] = z[j] - x[j];
            }
            if step < time - previous || !time.is_finite() {
                break;
            }
        }
        z[i] = target;
        displacement[i] = target - x[i];
        direction[i] = 0.0;
        previous = time;
    }
    z
}

fn subspace(x: &[f64], g: &[f64], mut z: Vec<f64>, bounds: &[(f64, f64)], h: &Hessian) -> Vec<f64> {
    let free: Vec<_> = z
        .iter()
        .zip(bounds)
        .map(|(&z, &(lo, hi))| lo < z && z < hi)
        .collect();
    let displacement: Vec<_> = z.iter().zip(x).map(|(z, x)| z - x).collect();
    let mut residual = vec![0.0; x.len()];
    h.apply(&displacement, &mut residual);
    for ((r, &g), &free) in residual.iter_mut().zip(g).zip(&free) {
        *r = if free { -*r - g } else { 0.0 };
    }
    let mut direction = residual.clone();
    let mut work = direction.clone();
    let mut correction = vec![0.0; x.len()];
    let mut norm = dot(&residual, &residual);
    let accuracy = norm * f64::EPSILON;
    // Solve the free-coordinate Newton system. The Hessian is a scaled identity
    // plus at most two rank-one terms per stored curvature pair.
    for _ in 0..x.len().min(2 * h.updates.len() + 1) {
        if norm <= accuracy {
            break;
        }
        h.apply(&direction, &mut work);
        let curvature = dot(&direction, &work);
        if curvature <= 0.0 {
            break;
        }
        let step = norm / curvature;
        for i in 0..x.len() {
            if free[i] {
                correction[i] = step.mul_add(direction[i], correction[i]);
                residual[i] = (-step).mul_add(work[i], residual[i]);
            }
        }
        let next = dot(&residual, &residual);
        for (d, &r) in direction.iter_mut().zip(&residual) {
            *d = (next / norm).mul_add(*d, r);
        }
        norm = next;
    }
    let step = step_bound(&z, &correction, bounds).min(1.0);
    for ((z, &d), &(lo, hi)) in z.iter_mut().zip(&correction).zip(bounds) {
        *z = step.mul_add(d, *z).clamp(lo, hi);
    }
    z
}

fn step_bound(x: &[f64], d: &[f64], bounds: &[(f64, f64)]) -> f64 {
    x.iter()
        .zip(d)
        .zip(bounds)
        .fold(f64::INFINITY, |limit, ((&x, &d), &(lo, hi))| {
            if d == 0.0 {
                limit
            } else {
                limit.min((if d < 0.0 { lo - x } else { hi - x }) / d)
            }
        })
}

#[derive(Clone, Copy)]
struct Trial {
    step: f64,
    value: f64,
    slope: f64,
}

fn cubic(a: Trial, b: Trial) -> f64 {
    let width = b.step - a.step;
    let slope = a.slope + b.slope - 3.0 * (b.value - a.value) / width;
    let scale = slope.abs().max(a.slope.abs()).max(b.slope.abs());
    let root = ((slope / scale).powi(2) - (a.slope / scale) * (b.slope / scale))
        .max(0.0)
        .sqrt()
        .copysign(width);
    let fraction =
        (b.slope / scale + root - slope / scale) / ((b.slope - a.slope) / scale + 2.0 * root);
    b.step - width * fraction
}

fn secant(a: Trial, b: Trial) -> f64 {
    b.step + (a.step - b.step) * (b.slope / (b.slope - a.slope))
}

struct SearchInterval {
    best: Trial,
    other: Trial,
    bracketed: bool,
    minimum: f64,
    maximum: f64,
}

impl SearchInterval {
    fn update(&mut self, trial: Trial, shift: f64) -> f64 {
        let modify = |t: Trial| Trial {
            step: t.step,
            value: (-shift).mul_add(t.step, t.value),
            slope: t.slope - shift,
        };
        let x = modify(self.best);
        let y = modify(self.other);
        let p = modify(trial);
        let opposite = p.slope.is_sign_positive() != x.slope.is_sign_positive();
        let width = p.step - x.step;
        let midpoint = |a: f64, b: f64| a * 0.5 + b * 0.5;
        let next = if p.value > x.value {
            let c = cubic(x, p);
            let q = x.step - 0.5 * x.slope * width / ((p.value - x.value) / width - x.slope);
            self.bracketed = true;
            if (c - x.step).abs() < (q - x.step).abs() {
                c
            } else {
                midpoint(c, q)
            }
        } else if opposite {
            let c = cubic(x, p);
            let q = secant(x, p);
            self.bracketed = true;
            if (c - p.step).abs() > (q - p.step).abs() {
                c
            } else {
                q
            }
        } else if p.slope.abs() < x.slope.abs() {
            let c = cubic(x, p);
            let edge = if width > 0.0 {
                self.maximum
            } else {
                self.minimum
            };
            let c = if c.is_finite() && (c - p.step) / width > 0.0 {
                c
            } else {
                edge
            };
            let q = secant(x, p);
            let c_is_nearer = (c - p.step).abs() < (q - p.step).abs();
            if self.bracketed {
                let next = if c_is_nearer { c } else { q };
                let limit = 0.66_f64.mul_add(y.step - p.step, p.step);
                if width > 0.0 {
                    next.min(limit)
                } else {
                    next.max(limit)
                }
            } else if c_is_nearer {
                q
            } else {
                c
            }
        } else if self.bracketed {
            cubic(p, y)
        } else if width > 0.0 {
            self.maximum
        } else {
            self.minimum
        };
        if p.value > x.value {
            self.other = trial;
        } else {
            if opposite {
                self.other = self.best;
            }
            self.best = trial;
        }
        if next.is_finite() {
            next
        } else if self.bracketed {
            midpoint(self.best.step, self.other.step)
        } else {
            self.maximum
        }
    }
}

struct Line<'a> {
    x: &'a [f64],
    direction: &'a [f64],
    bounds: &'a [(f64, f64)],
    max_step: f64,
    value: f64,
    slope: f64,
}

impl Line<'_> {
    fn search<F: Objective>(
        &self,
        f: &mut F,
        out: &mut OptimOut,
        next: &mut [f64],
        gradient: &mut [f64],
        mut step: f64,
    ) -> Result<Option<f64>, F::Err> {
        let initial = Trial {
            step: 0.0,
            value: self.value,
            slope: self.slope,
        };
        let mut interval = SearchInterval {
            best: initial,
            other: initial,
            bracketed: false,
            minimum: 0.0,
            maximum: 5.0 * step,
        };
        let mut modified = true;
        let mut width = self.max_step;
        let mut previous_width = 2.0 * width;
        let slope_test = 0.001 * self.slope;
        for _ in 0..20 {
            move_to(next, self.x, self.direction, step);
            for (x, &(lo, hi)) in next.iter_mut().zip(self.bounds) {
                *x = x.clamp(lo, hi);
            }
            let value = out.value(f, next)?;
            out.gradient(f, next, gradient)?;
            let slope = dot(gradient, self.direction);
            let trial = Trial { step, value, slope };
            let sufficient = value <= step.mul_add(slope_test, self.value);
            if sufficient
                && (slope.abs() <= -0.9 * self.slope
                    || step == self.max_step && slope <= slope_test)
            {
                return Ok(Some(value));
            }
            if modified && sufficient && slope >= 0.0 {
                modified = false;
            }
            // During the first stage, bracket a minimizer of the objective
            // minus the Armijo slope term when the unmodified objective decreases.
            let shift = if modified && value <= interval.best.value && !sufficient {
                slope_test
            } else {
                0.0
            };
            step = interval.update(trial, shift);
            if interval.bracketed {
                let (a, b) = (interval.best.step, interval.other.step);
                if (a - b).abs() >= 0.66 * previous_width {
                    step = a * 0.5 + b * 0.5;
                }
                (previous_width, width) = (width, (a - b).abs());
                (interval.minimum, interval.maximum) = (a.min(b), a.max(b));
            } else {
                let delta = step - interval.best.step;
                interval.minimum = 1.1_f64.mul_add(delta, step);
                interval.maximum = 4.0_f64.mul_add(delta, step);
            }
            step = step.clamp(0.0, self.max_step);
            if interval.bracketed && (step <= interval.minimum || step >= interval.maximum) {
                step = interval.best.step;
            }
        }
        Ok(None)
    }
}

/// Minimize with limited-memory BFGS, a projected Cauchy point, and a free-coordinate search.
///
/// `bounds` has one entry per parameter. Each entry contains the lower and upper bounds.
pub fn lbfgsb<F: Objective>(
    f: &mut F,
    par: Vec<f64>,
    bounds: &[(f64, f64)],
    c: BoxControl,
) -> Result<OptimOut, F::Err> {
    lbfgsb_with_report(f, par, bounds, c, |_, _| {})
}

/// Minimize by limited-memory BFGS and report progress at accepted steps.
pub fn lbfgsb_with_report<F: Objective>(
    f: &mut F,
    par: Vec<f64>,
    bounds: &[(f64, f64)],
    c: BoxControl,
    mut report: impl FnMut(&mut F, Progress<'_>),
) -> Result<OptimOut, F::Err> {
    let mut out = OptimOut::new(par, true);
    let message = if c.lmm == 0 {
        "ERROR: M .LE. 0"
    } else if c.factr < 0.0 {
        out.value = f64::INFINITY;
        "ERROR: FACTR .LT. 0"
    } else if bounds.len() != out.par.len()
        || bounds
            .iter()
            .any(|&(lo, hi)| lo.is_nan() || hi.is_nan() || lo > hi)
    {
        "ERROR: NO FEASIBLE SOLUTION"
    } else {
        let (code, message) = minimize(f, &mut out, bounds, c, &mut report)?;
        out.convergence = code;
        out.message = Some(message.to_owned());
        return Ok(out);
    };
    out.convergence = 52;
    out.message = Some(message.to_owned());
    Ok(out)
}

fn minimize<F: Objective>(
    f: &mut F,
    out: &mut OptimOut,
    bounds: &[(f64, f64)],
    c: BoxControl,
    report: &mut impl FnMut(&mut F, Progress<'_>),
) -> Result<(i32, &'static str), F::Err> {
    let mut x = out.par.clone();
    for (x, &(lo, hi)) in x.iter_mut().zip(bounds) {
        *x = x.clamp(lo, hi);
    }
    let mut value = out.value(f, &x)?;
    out.value = value;
    let mut g = vec![0.0; x.len()];
    if x.is_empty() {
        return Ok((0, "NOTHING TO DO"));
    }
    out.gradient(f, &x, &mut g)?;
    let mut next = x.clone();
    let mut next_g = g.clone();
    let mut history = VecDeque::new();
    let mut iteration = 0;
    let constrained = bounds
        .iter()
        .any(|(lo, hi)| lo.is_finite() || hi.is_finite());
    let fully_boxed = bounds
        .iter()
        .all(|(lo, hi)| lo.is_finite() && hi.is_finite());
    let convergence = loop {
        let projected = x
            .iter()
            .zip(&g)
            .zip(bounds)
            .fold(0.0_f64, |acc, ((&x, &g), &(lo, hi))| {
                let component = if g.is_nan() {
                    0.0
                } else if g > 0.0 {
                    g.min(x - lo)
                } else {
                    g.max(x - hi)
                };
                acc.max(component.abs())
            });
        if projected <= c.pgtol {
            break (0, "CONVERGENCE: NORM OF PROJECTED GRADIENT <= PGTOL");
        }
        let h = Hessian::new(&history);
        let z = cauchy(&x, &g, bounds, &h);
        let z = if history.is_empty() {
            z
        } else {
            subspace(&x, &g, z, bounds, &h)
        };
        let direction: Vec<_> = z.iter().zip(&x).map(|(z, x)| z - x).collect();
        let max_step = step_bound(&x, &direction, bounds).min(if iteration == 0 && constrained {
            1.0
        } else {
            1e10
        });
        let step = if iteration == 0 && !fully_boxed {
            dot(&direction, &direction).sqrt().recip().min(max_step)
        } else {
            1.0_f64.min(max_step)
        };
        let line = Line {
            x: &x,
            direction: &direction,
            bounds,
            max_step,
            value,
            slope: dot(&direction, &g),
        };
        let trial = if line.slope < 0.0 {
            line.search(f, out, &mut next, &mut next_g, step)?
        } else {
            None
        };
        let Some(next_value) = trial else {
            if history.is_empty() {
                break (52, "ERROR: ABNORMAL_TERMINATION_IN_LNSRCH");
            }
            history.clear();
            continue;
        };
        let s: Vec<_> = next.iter().zip(&x).map(|(a, b)| a - b).collect();
        let y: Vec<_> = next_g.iter().zip(&g).map(|(a, b)| a - b).collect();
        let curvature = dot(&s, &y);
        let change = value - next_value;
        let objective_scale = value.abs().max(next_value.abs()).max(1.0);
        value = next_value;
        let previous_slope = line.slope;
        x.copy_from_slice(&next);
        g.copy_from_slice(&next_g);
        iteration += 1;
        report(f, Progress::Iteration(iteration, value));
        if iteration > c.maxit {
            break (1, "NEW_X");
        }
        if change <= c.factr * f64::EPSILON * objective_scale {
            break (0, "CONVERGENCE: REL_REDUCTION_OF_F <= FACTR*EPSMCH");
        }
        if curvature > f64::EPSILON * (-previous_slope * step) {
            if history.len() == c.lmm {
                history.pop_front();
            }
            history.push_back(Pair { s, y, curvature });
        }
    };
    report(f, Progress::Finish(iteration));
    out.par = x;
    out.value = value;
    Ok(convergence)
}

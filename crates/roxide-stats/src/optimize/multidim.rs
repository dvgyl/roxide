use roxide_base::random::{get_rng, put_rng, rng_revision};
use roxide_eval::{Args, Call, Ctx, EvalResult, Unwind};
use roxide_gc::SexpId;
use roxide_math::fmin::fmin;
use roxide_math::optim::{
    self, Anneal, BoxControl, CgFormula, Control, OptimOut, Progress, SimplexControl,
};
use roxide_math::{Objective, rng::Rng};
use roxide_value::{NA_INTEGER, NA_REAL, SEXP_NIL, ValueApi};

use super::{callback, named_list, scalar::string, strings};
use crate::distn::support::{
    attr, limit, logical, number, numbers, set_attr, set_dim, set_dimnames,
};

const TOLERANCE: f64 = 1.4901161193847656e-8;
const KEYS: &[&str] = &[
    "trace",
    "fnscale",
    "parscale",
    "ndeps",
    "maxit",
    "abstol",
    "reltol",
    "alpha",
    "beta",
    "gamma",
    "REPORT",
    "warn.1d.NelderMead",
    "type",
    "lmm",
    "factr",
    "pgtol",
    "tmax",
    "temp",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Simplex,
    Bfgs,
    Cg,
    Bounded,
    Anneal,
    Brent,
}

struct Settings {
    values: Vec<(String, SexpId)>,
}

impl Settings {
    fn new(ctx: &mut Ctx, call: &Call, control: SexpId, warn: bool) -> EvalResult<Self> {
        let mut values = Vec::new();
        let names = attr(ctx, control, "names")?;
        if names != SEXP_NIL {
            for (index, &id) in ctx.session().str_slice(names)?.iter().enumerate() {
                let key = String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned();
                if !key.is_empty() {
                    values.push((key, ctx.session().list_get(control, index)?));
                }
            }
        }
        if warn {
            let mut unknown = Vec::new();
            for (key, _) in &values {
                if !KEYS.contains(&key.as_str()) && !unknown.contains(&key.as_str()) {
                    unknown.push(key);
                }
            }
            if !unknown.is_empty() {
                ctx.warn_now(
                    call.err_call(),
                    format!("unknown names in control: {}", unknown.join(", ")),
                )?;
            }
        }
        Ok(Self { values })
    }

    fn get(&self, name: &str) -> Option<SexpId> {
        self.values
            .iter()
            .rev()
            .find(|(key, id)| key == name && *id != SEXP_NIL)
            .map(|(_, value)| *value)
    }

    fn number(&self, ctx: &mut Ctx, call: &Call, name: &str, default: f64) -> EvalResult<f64> {
        self.get(name)
            .map_or(Ok(default), |arg| number(ctx, call, arg))
    }

    fn vector(
        &self,
        ctx: &mut Ctx,
        call: &Call,
        name: &str,
        n: usize,
        default: f64,
    ) -> EvalResult<Vec<f64>> {
        let values = self
            .get(name)
            .map_or_else(|| Ok(vec![default; n]), |arg| numbers(ctx, call, arg))?;
        if values.len() != n {
            return Err(ctx.stop(call.err_call(), format!("'{name}' is of the wrong length")));
        }
        Ok(values)
    }
}

struct RObjective<'a> {
    ctx: &'a mut Ctx,
    call: &'a Call,
    fun: SexpId,
    grad: SexpId,
    names: SexpId,
    parscale: Vec<f64>,
    ndeps: Vec<f64>,
    fnscale: f64,
    bounds: Option<Vec<(f64, f64)>>,
    method: Method,
    initial_error: Option<&'static str>,
    anneal_rng: Option<(Rng, u64)>,
}

impl RObjective<'_> {
    fn annealing_rng(&mut self) -> EvalResult<&mut Rng> {
        let revision = rng_revision(self.ctx);
        if self
            .anneal_rng
            .as_ref()
            .is_none_or(|(_, saved)| *saved != revision)
        {
            self.anneal_rng = Some((get_rng(self.ctx, self.call)?, revision));
        }
        self.anneal_rng.as_mut().map(|(rng, _)| rng).ok_or_else(|| {
            self.ctx
                .stop(self.call.err_call(), "cannot load random stream")
        })
    }

    fn evaluate(&mut self, x: &[f64]) -> EvalResult<f64> {
        let point: Vec<_> = x
            .iter()
            .zip(&self.parscale)
            .map(|(x, scale)| x * scale)
            .collect();
        self.ctx.scoped(|ctx| {
            if x.iter().chain(&point).any(|v| !v.is_finite()) {
                return Err(ctx.stop(self.call.err_call(), "non-finite value supplied by optim"));
            }
            let result = callback(ctx, self.call, self.fun, &point, self.names)?;
            let length = ctx.session().xlength(result)?;
            if length != 1 {
                return Err(ctx.stop(
                    self.call.err_call(),
                    format!("objective function in optim evaluates to length {length} not 1"),
                ));
            }
            Ok(number(ctx, self.call, result)? / self.fnscale)
        })
    }

    fn gradient_callback(&mut self, x: &[f64]) -> EvalResult<Vec<f64>> {
        let point: Vec<_> = x
            .iter()
            .zip(&self.parscale)
            .map(|(x, scale)| x * scale)
            .collect();
        self.ctx.scoped(|ctx| {
            if point.iter().any(|v| !v.is_finite()) {
                return Err(ctx.stop(self.call.err_call(), "non-finite value supplied by optim"));
            }
            let result = callback(ctx, self.call, self.grad, &point, self.names)?;
            let n = ctx.session().xlength(result)?;
            if n != x.len() {
                return Err(ctx.stop(
                    self.call.err_call(),
                    format!("gradient in optim evaluated to length {n} not {}", x.len()),
                ));
            }
            numbers(ctx, self.call, result)
        })
    }
}

impl Objective for RObjective<'_> {
    type Err = Unwind;

    fn value(&mut self, x: &[f64]) -> EvalResult<f64> {
        let value = self.evaluate(x)?;
        let initial_error = self.initial_error.take();
        if !value.is_finite()
            && let Some(msg) = if self.method == Method::Bounded {
                Some("L-BFGS-B needs finite values of 'fn'")
            } else {
                initial_error
            }
        {
            return Err(self.ctx.stop(self.call.err_call(), msg));
        }
        Ok(value)
    }

    fn gradient(&mut self, x: &[f64], g: &mut [f64]) -> EvalResult<()> {
        if self.grad != SEXP_NIL {
            let gradient = self.gradient_callback(x)?;
            for ((out, value), &scale) in g.iter_mut().zip(gradient).zip(&self.parscale) {
                *out = value * scale / self.fnscale;
            }
            if matches!(self.method, Method::Bfgs | Method::Cg)
                && g.iter().any(|value| value.is_infinite())
            {
                return Err(self
                    .ctx
                    .stop(self.call.err_call(), "non-finite value supplied by optim"));
            }
            return Ok(());
        }
        let mut point = x.to_vec();
        for (i, out) in g.iter_mut().enumerate() {
            let step = self.ndeps[i];
            let (mut plus, mut minus) = (step, step);
            if let Some(bounds) = &self.bounds {
                let (lo, hi) = bounds[i];
                if x[i] + plus > hi {
                    plus = hi - x[i];
                }
                if x[i] - minus < lo {
                    minus = x[i] - lo;
                }
            }
            point[i] = x[i] + plus;
            let above = self.evaluate(&point)?;
            point[i] = x[i] - minus;
            let below = self.evaluate(&point)?;
            point[i] = x[i];
            *out = (above - below) / (plus + minus);
            if !out.is_finite() {
                return Err(self.ctx.stop(
                    self.call.err_call(),
                    format!("non-finite finite-difference value [{}]", i + 1),
                ));
            }
        }
        Ok(())
    }
}

impl Anneal for RObjective<'_> {
    fn candidate(&mut self, x: &[f64], scale: f64, next: &mut [f64]) -> EvalResult<()> {
        if self.grad != SEXP_NIL {
            let candidate = self.gradient_callback(x)?;
            for ((out, value), &scale) in next.iter_mut().zip(candidate).zip(&self.parscale) {
                *out = value / scale;
            }
        } else {
            let rng = self.annealing_rng()?;
            for (next, &x) in next.iter_mut().zip(x) {
                *next = scale.mul_add(rng.norm_rand(), x);
            }
        }
        Ok(())
    }

    fn uniform(&mut self) -> EvalResult<f64> {
        Ok(self.annealing_rng()?.unif_rand())
    }
}

fn bounds(
    ctx: &mut Ctx,
    call: &Call,
    lower: SexpId,
    upper: SexpId,
    n: usize,
) -> EvalResult<Vec<(f64, f64)>> {
    let lower = numbers(ctx, call, lower)?;
    let upper = numbers(ctx, call, upper)?;
    if lower.is_empty() || upper.is_empty() {
        return Ok(vec![(f64::NEG_INFINITY, f64::INFINITY); n]);
    }
    Ok(lower
        .iter()
        .copied()
        .cycle()
        .zip(upper.iter().copied().cycle())
        .take(n)
        .map(|(lo, hi)| {
            if lo.is_nan() || hi.is_nan() {
                (
                    if lo.is_nan() { f64::NEG_INFINITY } else { lo },
                    if hi.is_nan() { f64::INFINITY } else { hi },
                )
            } else {
                (lo, hi)
            }
        })
        .collect())
}

fn hessian_matrix(f: &mut RObjective<'_>, par: &[f64]) -> EvalResult {
    let n = par.len();
    limit::<f64>(f.ctx, f.call, n.saturating_mul(n))?;
    let step: Vec<_> = f
        .ndeps
        .iter()
        .zip(&f.parscale)
        .map(|(h, scale)| h / scale)
        .collect();
    let mut matrix = optim::optim_hess(f, par, &step)?;
    for (i, col) in matrix.chunks_mut(n.max(1)).enumerate() {
        for (j, value) in col.iter_mut().enumerate() {
            *value *= f.fnscale / (f.parscale[i] * f.parscale[j]);
        }
    }
    let result = f.ctx.session_mut().alloc_real_from(&matrix)?;
    f.ctx.protect(result);
    set_dim(f.ctx, f.call, result, n, n)?;
    set_dimnames(f.ctx, result, f.names, f.names)?;
    Ok(result)
}

fn make_objective<'a>(
    ctx: &'a mut Ctx,
    call: &'a Call,
    args: &Args,
    controls: &Settings,
    n: usize,
    method: Method,
    needs_steps: bool,
) -> EvalResult<RObjective<'a>> {
    let names = attr(ctx, args.get(0), "names")?;
    let parscale = if method == Method::Brent {
        vec![1.0; n]
    } else {
        controls.vector(ctx, call, "parscale", n, 1.0)?
    };
    let ndeps = if needs_steps {
        controls.vector(ctx, call, "ndeps", n, 0.001)?
    } else {
        vec![0.001; n]
    };
    let fnscale = controls.number(ctx, call, "fnscale", 1.0)?;
    Ok(RObjective {
        ctx,
        call,
        fun: args.get(1),
        grad: args.get(2),
        names,
        parscale,
        ndeps,
        fnscale,
        bounds: None,
        method,
        initial_error: None,
        anneal_rng: None,
    })
}

pub(super) fn hessian(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    ctx.scoped(|ctx| {
        for id in [args.get(0), args.get(1), args.get(2), args.get(3), call.env] {
            ctx.protect(id);
        }
        let x = numbers(ctx, call, args.get(0))?;
        let controls = Settings::new(ctx, call, args.get(3), false)?;
        let mut f = make_objective(ctx, call, args, &controls, x.len(), Method::Bfgs, true)?;
        let scaled: Vec<_> = x.iter().zip(&f.parscale).map(|(x, s)| x / s).collect();
        hessian_matrix(&mut f, &scaled)
    })
}

pub(super) fn optim(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    ctx.scoped(|ctx| {
        ctx.protect(call.env);
        for i in 0..8 {
            ctx.protect(args.get(i));
        }
        let x = numbers(ctx, call, args.get(0))?;
        let n = x.len();
        let mut method = match string(ctx, args.get(3))?.as_str() {
            "Nelder-Mead" => Method::Simplex,
            "BFGS" => Method::Bfgs,
            "CG" => Method::Cg,
            "L-BFGS-B" => Method::Bounded,
            "SANN" => Method::Anneal,
            _ => Method::Brent,
        };
        if !matches!(method, Method::Bounded | Method::Brent) {
            for index in [4, 5] {
                if numbers(ctx, call, args.get(index))?.iter().any(|v| v.is_nan()) {
                    return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
                }
            }
        }
        let interval = bounds(ctx, call, args.get(4), args.get(5), n)?;
        if !matches!(method, Method::Bounded | Method::Brent)
            && interval.iter().any(|(lo, hi)| lo.is_finite() || hi.is_finite())
        {
            ctx.warn_now(call.err_call(), "bounds can only be used with method L-BFGS-B (or Brent)")?;
            method = Method::Bounded;
        }
        let controls = Settings::new(ctx, call, args.get(6), true)?;
        if method == Method::Bounded && (controls.get("reltol").is_some() || controls.get("abstol").is_some()) {
            ctx.warn_now(call.err_call(), "method L-BFGS-B uses 'factr' (and 'pgtol') instead of 'reltol' and 'abstol'")?;
        }
        if method == Method::Simplex && n == 1
            && match controls.get("warn.1d.NelderMead") {
                None => true,
                Some(flag) => logical(ctx, call, flag)? == Some(true),
            }
        {
            ctx.warn_now(call.err_call(), "one-dimensional optimization by Nelder-Mead is unreliable:\nuse \"Brent\" or optimize() directly")?;
        }
        if method == Method::Brent && n != 1 {
            return Err(ctx.stop(call.err_call(), "method = \"Brent\" is only available for one-dimensional optimization"));
        }
        if matches!(method, Method::Bfgs | Method::Simplex) {
            limit::<f64>(ctx, call, n.saturating_mul(n))?;
        }
        let needs_steps = args.get(2) == SEXP_NIL && matches!(method, Method::Bfgs | Method::Cg | Method::Bounded);
        let mut f = make_objective(ctx, call, args, &controls, n, method, needs_steps)?;
        let scaled: Vec<_> = x.iter().zip(&f.parscale).map(|(x, s)| x / s).collect();
        let result = run(&mut f, scaled, interval, &controls)?;
        let hessian = logical(f.ctx, call, args.get(7))?
            .ok_or_else(|| f.ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"))?;
        if hessian {
            limit::<f64>(f.ctx, call, n.saturating_mul(n))?;
            if !needs_steps && method != Method::Brent {
                f.ndeps = controls.vector(f.ctx, call, "ndeps", n, 0.001)?;
            }
        }
        finish(&mut f, result, hessian, &controls)
    })
}

fn run(
    f: &mut RObjective<'_>,
    par: Vec<f64>,
    interval: Vec<(f64, f64)>,
    settings: &Settings,
) -> EvalResult<OptimOut> {
    let method = f.method;
    let ctx = &mut *f.ctx;
    let call = f.call;
    let default_iterations = match method {
        Method::Simplex => 500.0,
        Method::Anneal => 10000.0,
        _ => 100.0,
    };
    let maxit = settings.number(ctx, call, "maxit", default_iterations)?;
    if method != Method::Brent && (!maxit.is_finite() || maxit.abs() > f64::from(i32::MAX)) {
        return Err(ctx.stop(call.err_call(), "'maxit' is not an integer"));
    }
    let control = Control {
        maxit: maxit as u32,
        abstol: settings.number(ctx, call, "abstol", f64::NEG_INFINITY)?,
        reltol: settings.number(ctx, call, "reltol", TOLERANCE)?,
    };
    let report = settings.number(
        ctx,
        call,
        "REPORT",
        if method == Method::Anneal {
            100.0
        } else {
            10.0
        },
    )? as i32;
    let trace = settings.number(ctx, call, "trace", 0.0)? as i32;
    f.initial_error = match (method, control.maxit) {
        (_, 0) => None,
        (Method::Simplex | Method::Cg, _) => {
            Some("function cannot be evaluated at initial parameters")
        }
        (Method::Bfgs, _) => Some("initial value in 'vmmin' is not finite"),
        _ => None,
    };
    let mut output = Trace {
        level: trace,
        period: report.max(1) as u32,
        method,
        maxit: control.maxit,
        tmax: 10,
        formula: CgFormula::FletcherReeves,
        iterations: 0,
    };
    let result = match method {
        Method::Simplex => {
            let simplex = SimplexControl {
                common: control,
                alpha: settings.number(ctx, call, "alpha", 1.0)?,
                beta: settings.number(ctx, call, "beta", 0.5)?,
                gamma: settings.number(ctx, call, "gamma", 2.0)?,
            };
            optim::nelder_mead_with_report(f, par, simplex, |f, event| output.write(f, event))
        }
        Method::Bfgs => {
            if report <= 0 {
                return Err(ctx.stop(call.err_call(), "REPORT must be > 0 (method = \"BFGS\")"));
            }
            optim::bfgs_with_report(f, par, control, |f, event| output.write(f, event))
        }
        Method::Cg => {
            let formula = match settings.number(ctx, call, "type", 1.0)? as i32 {
                1 => CgFormula::FletcherReeves,
                2 => CgFormula::PolakRibiere,
                3 => CgFormula::BealeSorenson,
                _ => {
                    return Err(
                        ctx.stop(call.err_call(), "unknown type in \"CG\" method of 'optim'")
                    );
                }
            };
            output.formula = formula;
            optim::cg_with_report(f, par, control, formula, |f, event| output.write(f, event))
        }
        Method::Bounded => {
            if report <= 0 {
                return Err(ctx.stop(
                    call.err_call(),
                    "REPORT must be > 0 (method = \"L-BFGS-B\")",
                ));
            }
            let lmm = settings.number(ctx, call, "lmm", 5.0)? as i32;
            limit::<f64>(
                ctx,
                call,
                par.len()
                    .saturating_mul(lmm.max(0) as usize)
                    .saturating_mul(4),
            )?;
            let box_control = BoxControl {
                maxit: control.maxit,
                lmm: lmm.max(0) as usize,
                factr: settings.number(ctx, call, "factr", 1e7)?,
                pgtol: settings.number(ctx, call, "pgtol", 0.0)?,
            };
            let scaled_bounds: Vec<_> = interval
                .iter()
                .zip(&f.parscale)
                .map(|(&(lo, hi), scale)| (lo / scale, hi / scale))
                .collect();
            f.bounds = Some(scaled_bounds.clone());
            optim::lbfgsb_with_report(f, par, &scaled_bounds, box_control, |f, event| {
                output.write(f, event)
            })
        }
        Method::Anneal => {
            let temp = settings.number(ctx, call, "temp", 10.0)?;
            let tmax = settings.number(ctx, call, "tmax", 10.0)? as u32;
            if tmax == 0 {
                return Err(ctx.stop(call.err_call(), "'tmax' is not a positive integer"));
            }
            output.tmax = tmax;
            f.annealing_rng()?;
            let result = optim::sann_with_report(f, par, control.maxit, temp, tmax, |f, event| {
                output.write(f, event)
            })?;
            f.annealing_rng()?;
            if let Some((rng, _)) = f.anneal_rng.take() {
                put_rng(f.ctx, &rng)?;
            }
            Ok(result)
        }
        Method::Brent => {
            let (lo, hi) = interval[0];
            if !lo.is_finite() || !hi.is_finite() {
                return Err(ctx.stop(call.err_call(), "'lower' and 'upper' must be finite values"));
            }
            if lo >= hi {
                return Err(ctx.stop(call.err_call(), "'xmin' not less than 'xmax'"));
            }
            if control.reltol <= 0.0 || !control.reltol.is_finite() {
                return Err(ctx.stop(call.err_call(), "invalid 'tol' value"));
            }
            let names = f.names;
            f.names = SEXP_NIL;
            let (x, _) = fmin(
                |x| {
                    let value = f.value(&[x])?;
                    super::scalar::finite_for_brent(f.ctx, f.call, value)
                },
                lo,
                hi,
                control.reltol,
            )?;
            let value = f.value(&[x])?;
            f.names = names;
            Ok(OptimOut {
                par: vec![x],
                value,
                fncount: 0,
                grcount: None,
                convergence: 0,
                message: None,
            })
        }
    }?;
    output.finish(f, &result);
    Ok(result)
}

struct Trace {
    level: i32,
    period: u32,
    method: Method,
    maxit: u32,
    tmax: u32,
    formula: CgFormula,
    iterations: u32,
}

impl Trace {
    fn write(&mut self, f: &mut RObjective<'_>, event: Progress<'_>) {
        if self.level <= 0 {
            return;
        }
        let text = match event {
            Progress::Initial(value) => match self.method {
                Method::Simplex => format!(
                    "  Nelder-Mead direct search function minimizer\nfunction value for initial parameters = {value:.6}\n"
                ),
                Method::Anneal => {
                    format!("sann objective function values\ninitial       value {value:.6}\n")
                }
                _ => format!("initial  value {value:.6} \n"),
            },
            Progress::SimplexSetup(tol, step) => {
                let tol = super::scalar::display(f.ctx, tol, 6);
                format!("  Scaled convergence tolerance is {tol}\nStepsize computed as {step:.6}\n")
            }
            Progress::SimplexStep(action, count, worst, best) => {
                format!("{action:<14}{count:>6} {worst:.6} {best:.6}\n")
            }
            Progress::SimplexDegenerate => "Polytope size measure not decreased in shrink\n".into(),
            Progress::CgTolerance(tol) => {
                let method = match self.formula {
                    CgFormula::FletcherReeves => "Fletcher Reeves",
                    CgFormula::PolakRibiere => "Polak Ribiere",
                    CgFormula::BealeSorenson => "Beale Sorenson",
                };
                let tol = super::scalar::display(f.ctx, tol, 6);
                format!(
                    "  Conjugate gradients function minimizer\nMethod: {method}\ntolerance used in gradient test={tol}\n"
                )
            }
            Progress::CgStep(restart, iter, count, value, point) => {
                let prefix = if iter == 0 {
                    ""
                } else if restart {
                    "* i> "
                } else {
                    " i> "
                };
                let parameters = point
                    .iter()
                    .map(|p| format!(" {p:>10.5}"))
                    .collect::<String>();
                format!("{prefix}{iter} {count} {value:.6}\nparameters{parameters} \n")
            }
            Progress::Iteration(iter, value) if self.method == Method::Anneal => {
                if !((iter - 1) / self.tmax + 1).is_multiple_of(self.period) {
                    return;
                }
                format!("iter {iter:>8} value {value:.6}\n")
            }
            Progress::Iteration(iter, value) if iter.is_multiple_of(self.period) => {
                let width = if self.method == Method::Bounded { 4 } else { 3 };
                format!("iter {iter:>width$} value {value:.6}\n")
            }
            Progress::Finish(iter) => {
                self.iterations = iter;
                if self.method != Method::Simplex {
                    return;
                }
                format!(
                    "Exiting from Nelder Mead minimizer\n    {iter} function evaluations used\n"
                )
            }
            _ => return,
        };
        let _ = f.ctx.host().write_out(text.as_bytes());
    }

    fn finish(&self, f: &mut RObjective<'_>, result: &OptimOut) {
        if self.level <= 0 {
            return;
        }
        let iter = self.iterations;
        let value = result.value;
        let text = match self.method {
            Method::Anneal => {
                format!("final         value {value:.6}\nsann stopped after {iter} iterations\n")
            }
            Method::Cg if iter != 0 => format!(
                "Exiting from conjugate gradients minimizer\n    {} function evaluations used\n    {} gradient evaluations used\n",
                result.fncount,
                result.grcount.unwrap_or(0)
            ),
            Method::Bfgs | Method::Bounded if iter != 0 => {
                let status = if iter < self.maxit {
                    "converged".to_owned()
                } else {
                    format!("stopped after {iter} iterations")
                };
                format!("final  value {value:.6} \n{status}\n")
            }
            _ => return,
        };
        let _ = f.ctx.host().write_out(text.as_bytes());
    }
}

fn finish(
    f: &mut RObjective<'_>,
    result: OptimOut,
    hessian: bool,
    settings: &Settings,
) -> EvalResult {
    let method = f.method;
    let par: Vec<_> = result
        .par
        .iter()
        .zip(&f.parscale)
        .map(|(x, scale)| x * scale)
        .collect();
    let ctx = &mut *f.ctx;
    let par_value = ctx.session_mut().alloc_real_from(&par)?;
    ctx.protect(par_value);
    if method != Method::Brent && f.names != SEXP_NIL {
        set_attr(ctx, par_value, "names", f.names)?;
    }
    let value = ctx.session_mut().scalar_real(if result.value.is_nan() {
        NA_REAL
    } else {
        result.value * f.fnscale
    })?;
    ctx.protect(value);
    let counts = ctx.session_mut().alloc_int_from(&[
        if method == Method::Brent {
            NA_INTEGER
        } else {
            result.fncount as i32
        },
        result.grcount.map_or(NA_INTEGER, |n| n as i32),
    ])?;
    ctx.protect(counts);
    let count_names = strings(ctx, &["function", "gradient"])?;
    ctx.protect(count_names);
    set_attr(ctx, counts, "names", count_names)?;
    let convergence = ctx.session_mut().scalar_int(result.convergence)?;
    ctx.protect(convergence);
    let message = match result.message {
        Some(text) => ctx.session_mut().scalar_str(&text)?,
        None => SEXP_NIL,
    };
    ctx.protect(message);
    let mut fields = vec!["par", "value", "counts", "convergence", "message"];
    let mut values = vec![par_value, value, counts, convergence, message];
    if hessian {
        f.method = Method::Bfgs;
        f.bounds = None;
        if method == Method::Brent {
            f.names = SEXP_NIL;
            f.parscale = settings.vector(f.ctx, f.call, "parscale", par.len(), 1.0)?;
        }
        let x: Vec<_> = par
            .iter()
            .zip(&f.parscale)
            .map(|(x, scale)| x / scale)
            .collect();
        fields.push("hessian");
        values.push(hessian_matrix(f, &x)?);
    }
    named_list(f.ctx, &fields, &values)
}

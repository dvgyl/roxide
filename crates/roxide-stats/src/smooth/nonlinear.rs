use super::{list, reals};
use crate::distn::support::{attr, limit, logical, number, numbers, set_attr, set_dim};
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

struct Sample {
    value: f64,
    gradient: Option<Vec<f64>>,
    hessian: Option<Vec<f64>>,
}

struct Objective<'a> {
    ctx: &'a mut Ctx,
    call: &'a Call,
    fun: SexpId,
    gradient: SexpId,
    hessian: SexpId,
    names: SexpId,
    port: bool,
    evals: [i32; 2],
}

impl Objective<'_> {
    fn call<T>(
        &mut self,
        fun: SexpId,
        x: &[f64],
        read: impl FnOnce(&mut Ctx, SexpId) -> EvalResult<T>,
    ) -> EvalResult<T> {
        self.ctx.scoped(|ctx| {
            let p = reals(ctx, x)?;
            if self.names != SEXP_NIL {
                set_attr(ctx, p, "names", self.names)?;
            }
            let out = ctx.apply(fun, &[(None, p)], self.call.err_call(), self.call.env)?;
            ctx.protect(out);
            read(ctx, out)
        })
    }

    fn at(&mut self, x: &[f64], count: bool) -> EvalResult<Sample> {
        if count {
            self.evals[0] += 1;
        }
        let call = self.call;
        let port = self.port;
        let first = self.evals[0] == 1;
        self.call(self.fun, x, |ctx, out| {
            if !matches!(
                ctx.session().type_of(out)?,
                roxide_value::SexpType::Real | roxide_value::SexpType::Int
            ) || ctx.session().xlength(out)? != 1
            {
                return Err(ctx.stop(call.err_call(), "invalid function value in optimizer"));
            }
            let mut value = number(ctx, call, out)?;
            if value.is_nan() || (!port && !value.is_finite()) {
                ctx.warn_now(call.err_call(), "NA/Inf replaced by maximum positive value")?;
                value = f64::MAX;
            }
            let (gradient, hessian) = if port || !count {
                (None, None)
            } else {
                let gradient = derivative(ctx, call, out, "gradient", x.len(), first)?;
                let hessian = if gradient.is_some() {
                    derivative(ctx, call, out, "hessian", x.len() * x.len(), first)?
                } else {
                    None
                };
                (gradient, hessian)
            };
            Ok(Sample {
                value,
                gradient,
                hessian,
            })
        })
    }

    fn slopes(
        &mut self,
        x: &[f64],
        sample: &Sample,
        c: &Control,
        centered: bool,
    ) -> EvalResult<Vec<f64>> {
        self.evals[1] += 1;
        if self.gradient != SEXP_NIL {
            let call = self.call;
            return self.call(self.gradient, x, |ctx, out| {
                let result = numbers(ctx, call, out)?;
                if result.len() != x.len() || result.iter().any(|v| !v.is_finite()) {
                    return Err(ctx.stop(
                        call.err_call(),
                        "gradient has the wrong length or contains non-finite values",
                    ));
                }
                Ok(result)
            });
        }
        if let Some(g) = &sample.gradient {
            return Ok(g.clone());
        }
        self.numeric_slopes(x, sample.value, c, centered)
    }

    fn numeric_slopes(
        &mut self,
        x: &[f64],
        value: f64,
        c: &Control,
        centered: bool,
    ) -> EvalResult<Vec<f64>> {
        let step = if centered {
            c.precision.cbrt()
        } else {
            c.precision.sqrt()
        };
        let mut point = x.to_vec();
        let mut g = Vec::with_capacity(x.len());
        for (i, &p) in x.iter().enumerate() {
            let h = step * p.abs().max(c.scale[i].recip());
            let plus = (p + h).min(c.upper[i]);
            let minus = (p - h).max(c.lower[i]);
            let (a, b) = if plus > p && minus < p && centered {
                point[i] = minus;
                let low = self.at(&point, false)?.value;
                point[i] = plus;
                (low, self.at(&point, false)?.value)
            } else if plus > p {
                point[i] = plus;
                (value, self.at(&point, false)?.value)
            } else if minus < p {
                point[i] = minus;
                (self.at(&point, false)?.value, value)
            } else {
                g.push(0.0);
                continue;
            };
            let width = if centered && plus > p && minus < p {
                plus - minus
            } else {
                (point[i] - p).abs()
            };
            g.push((b - a) / width);
            point[i] = p;
        }
        Ok(g)
    }

    fn analytic_hessian(&mut self, x: &[f64], sample: &Sample) -> EvalResult<Option<Vec<f64>>> {
        if self.hessian == SEXP_NIL || self.gradient == SEXP_NIL {
            return Ok(sample.hessian.clone());
        }
        let call = self.call;
        self.call(self.hessian, x, |ctx, out| {
            let n = x.len();
            let dims = attr(ctx, out, "dim")?;
            let square = ctx
                .session()
                .int(dims)
                .is_ok_and(|dim| dim.len() == 2 && dim.iter().all(|v| v as usize == n));
            if !square || ctx.session().type_of(out)? != SexpType::Real {
                return Err(ctx.stop(
                    call.err_call(),
                    format!("Hessian function must return a square numeric matrix of order {n}"),
                ));
            }
            let mut h = numbers(ctx, call, out)?;
            for i in 0..n {
                for j in i + 1..n {
                    h[j * n + i] = h[i * n + j];
                }
            }
            if h.iter().any(|v| !v.is_finite()) {
                return Err(ctx.stop(call.err_call(), "Hessian contains non-finite values"));
            }
            Ok(Some(h))
        })
    }

    fn numeric_hessian(&mut self, x: &[f64], value: f64, c: &Control) -> EvalResult<Vec<f64>> {
        let n = x.len();
        let mut h = vec![0.0; n * n];
        let mut q = x.to_vec();
        let step: Vec<_> = x
            .iter()
            .zip(&c.scale)
            .map(|(&p, &scale)| (p + c.precision.cbrt() * p.abs().max(scale.recip())) - p)
            .collect();
        let mut neighbor = vec![0.0; n];
        for i in 0..n {
            q[i] = x[i] + step[i];
            neighbor[i] = self.at(&q, false)?.value;
            q[i] = x[i];
        }
        for i in 0..n {
            for j in 0..=i {
                q[i] += step[i];
                q[j] += step[j];
                let cross = self.at(&q, false)?.value;
                let v = ((cross - neighbor[i]) - (neighbor[j] - value)) / (step[i] * step[j]);
                h[i * n + j] = v;
                h[j * n + i] = v;
                q[i] = x[i];
                q[j] = x[j];
            }
        }
        Ok(h)
    }
}

fn derivative(
    ctx: &mut Ctx,
    call: &Call,
    out: SexpId,
    name: &str,
    len: usize,
    warn: bool,
) -> EvalResult<Option<Vec<f64>>> {
    let id = attr(ctx, out, name)?;
    if id == SEXP_NIL {
        return Ok(None);
    }
    if !matches!(ctx.session().type_of(id)?, SexpType::Real | SexpType::Int)
        || ctx.session().xlength(id)? != len
    {
        if warn {
            ctx.warn_now(
                call.err_call(),
                format!("{name} supplied is of the wrong length or mode, so ignored"),
            )?;
        }
        return Ok(None);
    }
    let derivative = numbers(ctx, call, id)?;
    if derivative.iter().any(|v| !v.is_finite()) {
        return Err(ctx.stop(
            call.err_call(),
            format!("non-finite value supplied by '{name}'"),
        ));
    }
    Ok(Some(derivative))
}

struct Control {
    scale: Vec<f64>,
    lower: Vec<f64>,
    upper: Vec<f64>,
    iter: i32,
    eval: i32,
    trace: i32,
    stepmax: f64,
    steptol: f64,
    gradtol: f64,
    abstol: f64,
    reltol: f64,
    fscale: f64,
    precision: f64,
    check: bool,
    result_hessian: bool,
}

impl Control {
    fn read(ctx: &mut Ctx, call: &Call, args: &Args, n: usize, port: bool) -> EvalResult<Self> {
        let mut scale = numbers(ctx, call, args.get(4))?;
        let options = numbers(ctx, call, args.get(5))?;
        if options.len() != if port { 11 } else { 9 } || !options.iter().all(|x| x.is_finite()) {
            return Err(ctx.stop(call.err_call(), "invalid optimization controls"));
        }
        if scale.len() != n || scale.iter().any(|&s| !s.is_finite() || s <= 0.0) {
            return Err(ctx.stop(call.err_call(), "invalid parameter scale"));
        }
        let (lower, upper) = if port {
            (
                numbers(ctx, call, args.get(6))?,
                numbers(ctx, call, args.get(7))?,
            )
        } else {
            for s in &mut scale {
                *s = s.recip();
            }
            (vec![f64::NEG_INFINITY; n], vec![f64::INFINITY; n])
        };
        if lower.len() != n
            || upper.len() != n
            || lower
                .iter()
                .zip(&upper)
                .any(|(&a, &b)| a.is_nan() || b.is_nan() || a > b)
        {
            return Err(ctx.stop(call.err_call(), "lower bound exceeds upper bound"));
        }
        let c = if port {
            Self {
                scale,
                lower,
                upper,
                eval: options[0] as i32,
                iter: options[1] as i32,
                trace: options[2] as i32,
                abstol: options[3],
                reltol: options[4],
                steptol: options[5],
                stepmax: f64::MAX,
                gradtol: options[5],
                fscale: 1.0,
                precision: options[10],
                check: false,
                result_hessian: false,
            }
        } else {
            Self {
                scale,
                lower,
                upper,
                eval: i32::MAX,
                iter: options[6] as i32,
                trace: options[1] as i32,
                abstol: -1.0,
                reltol: 0.0,
                steptol: options[5],
                stepmax: options[4],
                gradtol: options[3],
                fscale: options[0],
                precision: 10.0_f64.powf(-options[2]).max(f64::EPSILON),
                check: options[7] != 0.0,
                result_hessian: options[8] != 0.0,
            }
        };
        if !port && c.iter <= 0 {
            return Err(ctx.stop(call.err_call(), "invalid iteration limit in nlm"));
        }
        if !(0..=100_000).contains(&c.iter)
            || c.eval < 1
            || c.stepmax <= 0.0
            || c.steptol < 0.0
            || c.gradtol < 0.0
            || c.precision <= 0.0
            || c.fscale <= 0.0
        {
            return Err(ctx.stop(call.err_call(), "invalid optimization controls"));
        }
        Ok(c)
    }

    fn norm(&self, x: &[f64]) -> f64 {
        x.iter()
            .zip(&self.scale)
            .fold(0.0_f64, |norm, (x, s)| norm.hypot(x * s))
    }

    fn trial(&self, x: &[f64], direction: &[f64], alpha: f64) -> Vec<f64> {
        x.iter()
            .zip(direction)
            .enumerate()
            .map(|(i, (&x, &d))| (x + alpha * d).clamp(self.lower[i], self.upper[i]))
            .collect()
    }

    fn projected(&self, x: &[f64], g: &[f64]) -> Vec<f64> {
        g.iter()
            .enumerate()
            .map(|(i, &g)| {
                if (x[i] <= self.lower[i] && g > 0.0) || (x[i] >= self.upper[i] && g < 0.0) {
                    0.0
                } else {
                    g
                }
            })
            .collect()
    }
}

fn dot(x: &[f64], y: &[f64]) -> f64 {
    x.iter().zip(y).map(|(a, b)| a * b).sum()
}

fn newton(h: &[f64], g: &[f64], free: &[bool]) -> Option<Vec<f64>> {
    let n = g.len();
    let mut factor = vec![0.0; n * n];
    let mut d = vec![0.0; n];
    for i in 0..n {
        if !free[i] {
            factor[i * n + i] = 1.0;
            continue;
        }
        for j in 0..=i {
            if !free[j] {
                continue;
            }
            let a = h[i * n + j] - dot(&factor[i * n..i * n + j], &factor[j * n..j * n + j]);
            if i == j {
                if a <= 0.0 || !a.is_finite() {
                    return None;
                }
                factor[i * n + i] = a.sqrt();
            } else {
                factor[i * n + j] = a / factor[j * n + j];
            }
        }
        d[i] = (-g[i] - dot(&factor[i * n..i * n + i], &d[..i])) / factor[i * n + i];
    }
    for i in (0..n).rev() {
        for j in i + 1..n {
            d[i] -= factor[j * n + i] * d[j];
        }
        d[i] /= factor[i * n + i];
    }
    Some(d)
}

fn update(h: &mut [f64], step: &[f64], gradient_change: &[f64]) {
    let n = step.len();
    let hp: Vec<_> = h.chunks_exact(n).map(|row| dot(row, step)).collect();
    let predicted = dot(step, &hp);
    let observed = dot(step, gradient_change);
    if !(predicted > 0.0 && observed > 1e-12 * predicted) {
        return;
    }
    for i in 0..n {
        for j in 0..=i {
            let v = h[i * n + j] + gradient_change[i] * gradient_change[j] / observed
                - hp[i] * hp[j] / predicted;
            h[i * n + j] = v;
            h[j * n + i] = v;
        }
    }
}

fn check_derivatives(f: &mut Objective<'_>, c: &Control, x: &[f64], sm: &Sample) -> EvalResult<()> {
    let Some(g) = &sm.gradient else { return Ok(()) };
    let tolerance = c.precision.sqrt().max(0.01);
    let numeric = f.numeric_slopes(x, sm.value, c, false)?;
    for (i, (&analytic, &numeric)) in g.iter().zip(&numeric).enumerate() {
        let scale = analytic
            .abs()
            .max(sm.value.abs().max(c.fscale) / x[i].abs().max(c.scale[i].recip()));
        if (analytic - numeric).abs() > tolerance * scale {
            return Err(f.ctx.stop(
                f.call.err_call(),
                "probable coding error in analytic gradient",
            ));
        }
    }
    if let Some(h) = &sm.hessian {
        let numeric = f.numeric_hessian(x, sm.value, c)?;
        for (analytic, numeric) in h.iter().zip(numeric) {
            if (analytic - numeric).abs() > tolerance * (1.0 + analytic.abs()) {
                return Err(f.ctx.stop(
                    f.call.err_call(),
                    "probable coding error in analytic Hessian",
                ));
            }
        }
    }
    Ok(())
}

struct ResultState {
    x: Vec<f64>,
    sample: Sample,
    g: Vec<f64>,
    iterations: i32,
    reason: StopReason,
}

enum StopReason {
    GradientTolerance,
    BoundedGradientTolerance,
    StepTolerance,
    RelativeFunctionTolerance,
    AbsoluteFunctionTolerance,
    NoProgress,
    IterationLimit,
    EvaluationLimit,
    RepeatedMaximumSteps,
}

impl StopReason {
    fn nlm_code(&self) -> i32 {
        match self {
            Self::GradientTolerance => 1,
            Self::StepTolerance | Self::RelativeFunctionTolerance => 2,
            Self::NoProgress => 3,
            Self::IterationLimit => 4,
            Self::BoundedGradientTolerance | Self::RepeatedMaximumSteps => 5,
            Self::AbsoluteFunctionTolerance => 6,
            Self::EvaluationLimit => 9,
        }
    }

    fn port_status(&self) -> (i32, &'static str) {
        match self {
            Self::GradientTolerance | Self::StepTolerance => (0, "X-convergence (3)"),
            Self::RelativeFunctionTolerance => (0, "relative convergence (4)"),
            Self::NoProgress => (1, "false convergence (8)"),
            Self::BoundedGradientTolerance | Self::RepeatedMaximumSteps => {
                (0, "both X-convergence and relative convergence (5)")
            }
            Self::AbsoluteFunctionTolerance => (0, "absolute function convergence (6)"),
            Self::EvaluationLimit => (
                1,
                "function evaluation limit reached without convergence (9)",
            ),
            Self::IterationLimit => (1, "iteration limit reached without convergence (10)"),
        }
    }
}

fn scaled_gradient(c: &Control, x: &[f64], g: &[f64], value: f64) -> f64 {
    g.iter()
        .enumerate()
        .map(|(i, v)| v.abs() * x[i].abs().max(c.scale[i].recip()) / value.abs().max(c.fscale))
        .fold(0.0, f64::max)
}

fn minimize(f: &mut Objective<'_>, c: &Control, mut x: Vec<f64>) -> EvalResult<ResultState> {
    let n = x.len();
    let mut sm = f.at(&x, true)?;
    let mut g = f.slopes(&x, &sm, c, false)?;
    if c.check {
        check_derivatives(f, c, &x, &sm)?;
    }
    let mut h = vec![0.0; n * n];
    for i in 0..n {
        h[i * n + i] = c.scale[i].powi(2);
    }
    let mut centered = false;
    let mut iterations = 0;
    let mut max_steps = 0;
    let mut radius = if f.port { 1.0 } else { c.stepmax };
    let reason = 'search: loop {
        if iterations >= c.iter {
            break StopReason::IterationLimit;
        }
        if f.port && f.evals[0] >= c.eval {
            iterations = iterations.max(1);
            break StopReason::EvaluationLimit;
        }
        let projected = c.projected(&x, &g);
        let relative_grad = if f.port {
            scaled_gradient(c, &x, &projected, 0.0)
        } else {
            scaled_gradient(c, &x, &projected, sm.value)
        };
        let analytic = f.analytic_hessian(&x, &sm)?;
        if relative_grad <= c.gradtol {
            let bounded = c.lower.iter().chain(&c.upper).any(|v| v.is_finite());
            break if f.port && bounded {
                StopReason::BoundedGradientTolerance
            } else {
                StopReason::GradientTolerance
            };
        }
        let free: Vec<_> = projected
            .iter()
            .zip(&g)
            .enumerate()
            .map(|(i, (&p, &g))| p == g && c.lower[i] != c.upper[i])
            .collect();
        let direction = newton(analytic.as_deref().unwrap_or(&h), &projected, &free)
            .filter(|d| dot(d, &projected) < 0.0)
            .unwrap_or_else(|| {
                projected
                    .iter()
                    .zip(&c.scale)
                    .map(|(&g, &s)| -g / (s * s))
                    .collect()
            });
        let norm = c.norm(&direction);
        let mut alpha = (radius / norm).min(1.0);
        let mut result = None;
        for i in 0..64 {
            let point = c.trial(&x, &direction, alpha);
            if point == x {
                break;
            }
            let step: Vec<_> = point.iter().zip(&x).map(|(a, b)| a - b).collect();
            if !f.port
                && !centered
                && sm.gradient.is_none()
                && step
                    .iter()
                    .enumerate()
                    .all(|(j, v)| v.abs() <= c.steptol * x[j].abs().max(c.scale[j].recip()))
            {
                break;
            }
            let slope = dot(&g, &step);
            let mut trial = f.at(&point, true)?;
            if trial.value <= sm.value + 1e-4 * slope && trial.value < sm.value {
                let mut point = point;
                if f.port && i == 0 && alpha < 1.0 && f.evals[0] < c.eval {
                    let curvature = 2.0 * (trial.value - sm.value - slope);
                    let factor = (-slope / curvature).clamp(1.0, 4.0);
                    if factor.is_finite() && factor > 1.25 {
                        let expanded = c.trial(&x, &step, factor);
                        if expanded != point {
                            let alternative = f.at(&expanded, true)?;
                            if alternative.value < trial.value {
                                point = expanded;
                                trial = alternative;
                            }
                        }
                    }
                }
                result = Some((point, trial));
                break;
            }
            if f.port && f.evals[0] >= c.eval {
                break 'search StopReason::EvaluationLimit;
            }
            let denominator = 2.0 * (trial.value - sm.value - slope);
            let fraction = if denominator > 0.0 {
                let upper = if !f.port && i == 0 && alpha == 1.0 {
                    0.95
                } else {
                    0.5
                };
                (-slope / denominator).clamp(0.1, upper)
            } else {
                0.5
            };
            alpha *= fraction;
        }
        let Some((point, trial)) = result else {
            if sm.gradient.is_none() && f.gradient == SEXP_NIL && !centered {
                centered = true;
                g = f.slopes(&x, &sm, c, true)?;
                continue;
            }
            break StopReason::NoProgress;
        };
        let step: Vec<_> = point.iter().zip(&x).map(|(a, b)| a - b).collect();
        let step_size = c.norm(&step);
        let next_g = f.slopes(&point, &trial, c, centered)?;
        let diff: Vec<_> = next_g.iter().zip(&g).map(|(a, b)| a - b).collect();
        if analytic.is_none() {
            if !f.port && iterations == 0 {
                let curvature = diff
                    .iter()
                    .zip(&c.scale)
                    .map(|(g, scale)| (g / scale).powi(2))
                    .sum::<f64>();
                let gamma = curvature / dot(&step, &diff);
                if gamma.is_finite() && gamma > 0.0 {
                    for i in 0..n {
                        h[i * n + i] = gamma * c.scale[i].powi(2);
                    }
                }
            }
            update(&mut h, &step, &diff);
        }
        let relative_step = step
            .iter()
            .enumerate()
            .map(|(i, v)| v.abs() / point[i].abs().max(c.scale[i].recip()))
            .fold(0.0, f64::max);
        let value_change = (sm.value - trial.value).abs();
        x = point;
        sm = trial;
        g = next_g;
        iterations += 1;
        if c.trace > 0 && iterations % c.trace == 0 {
            let _ = f
                .ctx
                .host()
                .write_out(format!("{iterations:4}: {:14.8}\n", sm.value).as_bytes());
        }
        if f.port {
            radius = (step_size * 2.0).max(c.steptol);
            if sm.value.abs() < c.abstol {
                break StopReason::AbsoluteFunctionTolerance;
            }
            if relative_step < c.steptol {
                break StopReason::StepTolerance;
            }
            if value_change <= c.reltol * sm.value.abs()
                && scaled_gradient(c, &x, &c.projected(&x, &g), 0.0) <= c.gradtol
            {
                break StopReason::RelativeFunctionTolerance;
            }
        } else {
            if scaled_gradient(c, &x, &g, sm.value) <= c.gradtol {
                break StopReason::GradientTolerance;
            }
            if relative_step <= c.steptol {
                break StopReason::StepTolerance;
            }
            max_steps = if step_size >= 0.99 * c.stepmax {
                max_steps + 1
            } else {
                0
            };
            if max_steps >= 5 {
                break StopReason::RepeatedMaximumSteps;
            }
        }
    };
    Ok(ResultState {
        x,
        sample: sm,
        g,
        iterations,
        reason,
    })
}

pub(super) fn fit(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut start = numbers(ctx, call, args.get(0))?;
    if start.is_empty() || start.iter().any(|v| !v.is_finite()) {
        return Err(ctx.stop(call.err_call(), "invalid initial parameters"));
    }
    let n = start.len();
    limit::<f64>(ctx, call, n.saturating_mul(n))?;
    let port = logical(ctx, call, args.get(8))?.unwrap_or(false);
    let control = Control::read(ctx, call, args, n, port)?;
    let names = if port {
        attr(ctx, args.get(0), "names")?
    } else {
        SEXP_NIL
    };
    ctx.protect(names);
    for (i, p) in start.iter_mut().enumerate() {
        *p = p.clamp(control.lower[i], control.upper[i]);
    }
    let mut objective = Objective {
        ctx,
        call,
        fun: args.get(1),
        gradient: args.get(2),
        hessian: args.get(3),
        names,
        port,
        evals: [0, 0],
    };
    let out = minimize(&mut objective, &control, start)?;
    let hessian = if control.result_hessian {
        Some(objective.numeric_hessian(&out.x, out.sample.value, &control)?)
    } else {
        None
    };
    let counts = objective.evals;
    let value = reals(ctx, &[out.sample.value])?;
    let par = reals(ctx, &out.x)?;
    let iter = ctx.session_mut().alloc_int_from(&[out.iterations])?;
    ctx.protect(iter);
    if port {
        if names != SEXP_NIL {
            set_attr(ctx, par, "names", names)?;
        }
        let (code, message) = out.reason.port_status();
        let code = ctx.session_mut().alloc_int_from(&[code])?;
        ctx.protect(code);
        let counts = ctx.session_mut().alloc_int_from(&counts)?;
        ctx.protect(counts);
        let message = ctx.session_mut().scalar_str(message)?;
        ctx.protect(message);
        list(ctx, &[par, value, code, iter, counts, message])
    } else {
        let g = reals(ctx, &out.g)?;
        let code = ctx.session_mut().alloc_int_from(&[out.reason.nlm_code()])?;
        ctx.protect(code);
        let mut values = vec![value, par, g];
        if let Some(h) = hessian {
            let h = reals(ctx, &h)?;
            set_dim(ctx, call, h, n, n)?;
            values.push(h);
        }
        values.extend([code, iter]);
        list(ctx, &values)
    }
}

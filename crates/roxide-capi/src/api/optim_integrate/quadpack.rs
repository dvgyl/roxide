use std::ffi::{c_int, c_void};

use roxide_eval::{Ctx, EvalResult, Unwind};
use roxide_math::{VecFn, quadpack};
use roxide_value::SEXP_NIL;

use super::super::{api, vector::check_capacity};
use super::{Callback, IntegrFn, store};

#[derive(Clone, Copy)]
enum Domain {
    Finite(f64, f64),
    Infinite { bound: f64, direction: c_int },
}

impl Domain {
    fn limits(self) -> (f64, f64) {
        match self {
            Self::Finite(a, b) => (a, b),
            Self::Infinite { .. } => (0.0, 1.0),
        }
    }

    fn point(self, t: f64) -> f64 {
        match self {
            Self::Finite(..) => t,
            Self::Infinite { bound, direction } => {
                bound + f64::from(direction.min(1)) * ((1.0 - t) / t)
            }
        }
    }

    fn two_sided(self) -> bool {
        matches!(self, Self::Infinite { direction: 2, .. })
    }
}

#[derive(Clone, Copy)]
struct Interval(f64, f64);

impl Interval {
    fn midpoint(self) -> f64 {
        self.0 * 0.5 + self.1 * 0.5
    }
}

struct Panel {
    interval: Interval,
    value: f64,
    error: f64,
}

struct Integrand<'a> {
    ctx: &'a mut Ctx,
    fun: IntegrFn,
    data: *mut c_void,
    domain: Domain,
    evaluations: c_int,
    panels: Vec<Panel>,
    left: Option<(usize, Panel)>,
    positive: Option<[f64; 15]>,
}

impl VecFn for Integrand<'_> {
    type Err = Unwind;

    fn eval(&mut self, x: &[f64], out: &mut [f64]) -> EvalResult<()> {
        out.copy_from_slice(x);
        Callback::Integrand(self.fun, out.len() as c_int, out.as_mut_ptr())
            .run(self.ctx, self.data)?;
        self.evaluations = self.evaluations.saturating_add(out.len() as c_int);
        let mut samples = [0.0; 21];
        samples[..out.len()].copy_from_slice(out);
        if self.domain.two_sided() {
            if let Some(positive) = self.positive.take() {
                for (sample, positive) in samples.iter_mut().zip(positive) {
                    *sample += positive;
                }
            } else {
                let mut positive = [0.0; 15];
                positive.copy_from_slice(out);
                self.positive = Some(positive);
                return Ok(());
            }
        }
        self.record(x, &samples)
    }
}

impl Integrand<'_> {
    fn record(&mut self, x: &[f64], samples: &[f64; 21]) -> EvalResult<()> {
        if self.panels.is_empty() {
            let (a, b) = self.domain.limits();
            self.panels.push(self.estimate(Interval(a, b), samples));
        } else if let Some((index, left)) = self.left.take() {
            let parent = self.panels[index].interval;
            let right = self.estimate(Interval(parent.midpoint(), parent.1), samples);
            let (first, extra) = if left.error >= right.error {
                (left, right)
            } else {
                (right, left)
            };
            self.panels[index] = first;
            self.panels.push(extra);
        } else {
            // The kernel evaluates the left child first. Compare a pair of
            // nodes as well as the center to distinguish narrow intervals.
            let (index, interval) = self
                .panels
                .iter()
                .enumerate()
                .find_map(|(index, panel)| {
                    let interval = Interval(panel.interval.0, panel.interval.midpoint());
                    let center = (interval.0 + interval.1) * 0.5;
                    let node = match self.domain {
                        Domain::Finite(..) => 0.9739065285171717,
                        Domain::Infinite { .. } => 0.9491079123427585,
                    };
                    let offset = ((interval.1 - interval.0) * 0.5) * node;
                    let sign = if self.domain.two_sided() { -1.0 } else { 1.0 };
                    let matches = [center, center - offset, center + offset]
                        .iter()
                        .zip(x)
                        .all(|(&t, &actual)| sign * self.domain.point(t) == actual);
                    matches.then_some((index, interval))
                })
                .ok_or_else(|| {
                    self.ctx
                        .stop(SEXP_NIL, "cannot identify the quadrature work interval")
                })?;
            self.left = Some((index, self.estimate(interval, samples)));
        }
        Ok(())
    }

    fn estimate(&self, interval: Interval, samples: &[f64; 21]) -> Panel {
        let Interval(a, b) = interval;
        let out = if matches!(self.domain, Domain::Finite(..)) {
            quadpack::qk21(&mut Replay(|_, i| samples[i]), a, b)
        } else {
            // Replay a single 15-point rule with the observed integrand.
            // Convert its [0, 1] weights to the panel's transformed interval.
            quadpack::dqagi(
                &mut Replay(|x: f64, i: usize| {
                    let u = 1.0 / (1.0 + x);
                    let t = (a + b) * 0.5 + (b - a) * (u - 0.5);
                    ((samples[i] / t) / t) * (u * u) * (b - a)
                }),
                0.0,
                f64::INFINITY,
                0.0,
                0.0,
                1,
            )
        };
        let Ok(out) = out;
        Panel {
            interval,
            value: out.value,
            error: out.abs_error,
        }
    }
}

struct Replay<F>(F);

impl<F: Fn(f64, usize) -> f64> VecFn for Replay<F> {
    type Err = std::convert::Infallible;

    fn eval(&mut self, x: &[f64], out: &mut [f64]) -> Result<(), Self::Err> {
        for (i, (&x, y)) in x.iter().zip(out).enumerate() {
            *y = (self.0)(x, i);
        }
        Ok(())
    }
}

struct QuadOutput {
    result: *mut f64,
    abserr: *mut f64,
    neval: *mut c_int,
    ier: *mut c_int,
    last: *mut c_int,
    index_work: *mut c_int,
    work: *mut f64,
}

impl QuadOutput {
    unsafe fn summary(
        &self,
        value: f64,
        error: f64,
        evaluations: c_int,
        status: c_int,
        last: c_int,
    ) {
        // SAFETY: The C caller guarantees that non-null outputs are writable.
        unsafe {
            store(self.result, value);
            store(self.abserr, error);
            store(self.neval, evaluations);
            store(self.ier, status);
            store(self.last, last);
        }
    }

    unsafe fn workspace(&self, limit: usize, panels: &[Panel]) {
        let mut order: Vec<_> = (0..panels.len()).collect();
        order.sort_by(|&a, &b| panels[b].error.total_cmp(&panels[a].error).then(a.cmp(&b)));
        // SAFETY: The caller supplies the validated work arrays. Each panel
        // uses one element in each of the four limit-sized sections.
        unsafe {
            for (i, panel) in panels.iter().enumerate() {
                self.work.add(i).write(panel.interval.0);
                self.work.add(limit + i).write(panel.interval.1);
                self.work.add(2 * limit + i).write(panel.value);
                self.work.add(3 * limit + i).write(panel.error);
                self.index_work.add(i).write((order[i] + 1) as c_int);
            }
        }
    }
}

unsafe fn integrate(
    fun: Option<IntegrFn>,
    data: *mut c_void,
    domain: Option<Domain>,
    tolerance: (*const f64, *const f64),
    limit: *const c_int,
    lenw: *const c_int,
    output: QuadOutput,
) {
    // SAFETY: The caller supplies the active trampoline, scalar arguments and
    // arrays. Check the scalar pointers and work array dimensions before use.
    unsafe {
        api((), |ctx| {
            output.summary(0.0, 0.0, 0, 6, 0);
            let (Some(domain), Some(fun), Some(&abstol), Some(&reltol), Some(&limit), Some(&lenw)) = (
                domain,
                fun,
                tolerance.0.as_ref(),
                tolerance.1.as_ref(),
                limit.as_ref(),
                lenw.as_ref(),
            ) else {
                return Ok(());
            };
            if limit < 1 || lenw / 4 < limit || output.index_work.is_null() || output.work.is_null()
            {
                return Ok(());
            }
            let limit = limit as usize;
            check_capacity(ctx, limit, 4 * size_of::<f64>())?;
            let (a, b) = domain.limits();
            output.workspace(
                limit,
                &[Panel {
                    interval: Interval(a, b),
                    value: 0.0,
                    error: 0.0,
                }],
            );
            output.index_work.write(0);
            if !abstol.is_finite()
                || !reltol.is_finite()
                || (abstol <= 0.0 && reltol < (50.0 * f64::EPSILON).max(5e-29))
            {
                return Ok(());
            }
            let mut f = Integrand {
                ctx,
                fun,
                data,
                domain,
                evaluations: 0,
                panels: Vec::new(),
                left: None,
                positive: None,
            };
            let out = match domain {
                Domain::Finite(a, b) => quadpack::dqags(&mut f, a, b, abstol, reltol, limit),
                Domain::Infinite { bound, direction } => {
                    let (a, b) = match direction {
                        -1 => (f64::NEG_INFINITY, bound),
                        1 => (bound, f64::INFINITY),
                        _ => (f64::NEG_INFINITY, f64::INFINITY),
                    };
                    quadpack::dqagi(&mut f, a, b, abstol, reltol, limit)
                }
            }?;
            let ier = match out.status {
                quadpack::Status::Ok => 0,
                quadpack::Status::Subdivisions => 1,
                quadpack::Status::Roundoff => 2,
                quadpack::Status::BadBehavior | quadpack::Status::NonFinite => 3,
                quadpack::Status::Divergent => 5,
            };
            output.workspace(limit, &f.panels);
            output.summary(
                out.value,
                out.abs_error,
                f.evaluations,
                ier,
                out.subdivisions as c_int,
            );
            Ok(())
        });
    }
}

/// Integrate a vectorized callback over a finite interval.
///
/// # Safety
/// Use an active native trampoline. Scalar inputs must be initialized.
/// `index_work` must hold `limit` writable integers. `work` must hold
/// `lenw` writable doubles. The callback must replace all `n` samples.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rdqags(
    fun: Option<IntegrFn>,
    data: *mut c_void,
    lower: *const f64,
    upper: *const f64,
    abstol: *const f64,
    reltol: *const f64,
    result: *mut f64,
    abserr: *mut f64,
    neval: *mut c_int,
    ier: *mut c_int,
    limit: *const c_int,
    lenw: *const c_int,
    last: *mut c_int,
    index_work: *mut c_int,
    work: *mut f64,
) {
    // SAFETY: The caller supplies readable scalars and writable work arrays.
    unsafe {
        let domain = lower
            .as_ref()
            .zip(upper.as_ref())
            .and_then(|(&a, &b)| (a.is_finite() && b.is_finite()).then_some(Domain::Finite(a, b)));
        integrate(
            fun,
            data,
            domain,
            (abstol, reltol),
            limit,
            lenw,
            QuadOutput {
                result,
                abserr,
                neval,
                ier,
                last,
                index_work,
                work,
            },
        );
    }
}

/// Integrate a vectorized callback over an interval with infinite endpoints.
///
/// # Safety
/// Use an active native trampoline. Scalar inputs must be initialized.
/// `index_work` must hold `limit` writable integers. `work` must hold
/// `lenw` writable doubles. The callback must replace all `n` samples.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rdqagi(
    fun: Option<IntegrFn>,
    data: *mut c_void,
    bound: *const f64,
    infinite: *const c_int,
    abstol: *const f64,
    reltol: *const f64,
    result: *mut f64,
    abserr: *mut f64,
    neval: *mut c_int,
    ier: *mut c_int,
    limit: *const c_int,
    lenw: *const c_int,
    last: *mut c_int,
    index_work: *mut c_int,
    work: *mut f64,
) {
    // SAFETY: The caller supplies readable scalars and writable work arrays.
    unsafe {
        let domain = bound
            .as_ref()
            .zip(infinite.as_ref())
            .and_then(|(&bound, &direction)| match direction {
                2 => Some(Domain::Infinite {
                    bound: 0.0,
                    direction,
                }),
                -1 | 1 if bound.is_finite() => Some(Domain::Infinite { bound, direction }),
                _ => None,
            });
        integrate(
            fun,
            data,
            domain,
            (abstol, reltol),
            limit,
            lenw,
            QuadOutput {
                result,
                abserr,
                neval,
                ier,
                last,
                index_work,
                work,
            },
        );
    }
}

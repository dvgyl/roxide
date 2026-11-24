//! C callbacks and output buffers for optimization and quadrature.

#![allow(non_snake_case)]

use std::ffi::{c_char, c_int, c_void};
use std::ptr::{copy_nonoverlapping, from_mut};
use std::slice;

use roxide_eval::{Ctx, EvalResult, Unwind};
use roxide_math::optim::{
    self, Anneal, BoxControl, CgFormula, Control, OptimOut, Progress, SimplexControl,
};
use roxide_math::{Objective, VecFn, quadpack};
use roxide_value::SEXP_NIL;

use crate::jump::{FrameKind, VoidFn, enter_trampoline_with};

use super::{api, random, vector::check_capacity};

type OptimFn = unsafe extern "C-unwind" fn(c_int, *mut f64, *mut c_void) -> f64;
type OptimGr = unsafe extern "C-unwind" fn(c_int, *mut f64, *mut f64, *mut c_void);
type IntegrFn = unsafe extern "C-unwind" fn(*mut f64, c_int, *mut c_void);

enum Callback {
    Value(OptimFn, c_int, *mut f64, *mut f64),
    Gradient(OptimGr, c_int, *mut f64, *mut f64),
    Integrand(IntegrFn, c_int, *mut f64),
}

impl Callback {
    fn run(&mut self, ctx: &mut Ctx, data: *mut c_void) -> EvalResult<()> {
        let mut call = (self, data);
        // SAFETY: The C++ trampoline calls this adapter with the C calling
        // convention and catches foreign exceptions. Rust never calls it
        // through the erased type. The descriptor lives through the call.
        unsafe {
            let thunk = std::mem::transmute::<unsafe extern "C-unwind" fn(*mut c_void), VoidFn>(
                call_callback,
            );
            enter_trampoline_with(ctx, FrameKind::Callback, thunk, from_mut(&mut call).cast())
        }
    }
}

unsafe extern "C-unwind" fn call_callback(raw: *mut c_void) {
    // SAFETY: run supplies a live descriptor and the caller's callback.
    // The adapter owns no Rust values that need destruction on a C jump.
    unsafe {
        let (call, data) = &mut *raw.cast::<(&mut Callback, *mut c_void)>();
        match **call {
            Callback::Value(fun, n, x, result) => result.write(fun(n, x, *data)),
            Callback::Gradient(fun, n, x, g) => fun(n, x, g, *data),
            Callback::Integrand(fun, n, x) => fun(x, n, *data),
        }
    }
}

struct ForeignObjective<'a> {
    ctx: &'a mut Ctx,
    fun: OptimFn,
    grad: Option<OptimGr>,
    data: *mut c_void,
    point: Vec<f64>,
    gradient: Vec<f64>,
    active: Vec<usize>,
    initial_error: Option<&'static str>,
    cached_value: Option<f64>,
    gradient_norm: f64,
}

impl<'a> ForeignObjective<'a> {
    fn new(
        ctx: &'a mut Ctx,
        fun: Option<OptimFn>,
        grad: Option<OptimGr>,
        data: *mut c_void,
        point: Vec<f64>,
        active: Vec<usize>,
    ) -> EvalResult<Self> {
        let fun = fun.ok_or_else(|| ctx.stop(SEXP_NIL, "invalid NULL objective"))?;
        Ok(Self {
            gradient: vec![0.0; point.len()],
            ctx,
            fun,
            grad,
            data,
            point,
            active,
            initial_error: None,
            cached_value: None,
            gradient_norm: 0.0,
        })
    }

    fn update_point(&mut self, x: &[f64]) {
        for (&index, &value) in self.active.iter().zip(x) {
            self.point[index] = value;
        }
    }

    fn report(&mut self, trace: c_int, period: u32, event: Progress<'_>) {
        if trace <= 0 {
            return;
        }
        let line = match event {
            Progress::Initial(value) => format!("initial value {value:.6}\n"),
            Progress::SimplexStep(op, count, worst, best) => {
                format!("{op:>10} {count:>5} {worst:.6} {best:.6}\n")
            }
            Progress::CgStep(_, iter, count, value, _) => {
                format!("{iter:>5} {count:>5} {value:.6}\n")
            }
            Progress::Iteration(iter, value) if iter.is_multiple_of(period) => {
                format!("iter {iter:>4} value {value:.6}\n")
            }
            Progress::Finish(count) => format!("finished after {count}\n"),
            _ => return,
        };
        let _ = self.ctx.host().write_out(line.as_bytes());
    }
}

impl Objective for ForeignObjective<'_> {
    type Err = Unwind;

    fn value(&mut self, x: &[f64]) -> EvalResult<f64> {
        self.update_point(x);
        if let Some(value) = self.cached_value.take() {
            return Ok(value);
        }
        let mut result = f64::NAN;
        Callback::Value(
            self.fun,
            self.point.len() as c_int,
            self.point.as_mut_ptr(),
            &mut result,
        )
        .run(self.ctx, self.data)?;
        if let Some(message) = self.initial_error.take()
            && !result.is_finite()
        {
            return Err(self.ctx.stop(SEXP_NIL, message));
        }
        Ok(result)
    }

    fn gradient(&mut self, x: &[f64], gradient: &mut [f64]) -> EvalResult<()> {
        let grad = self
            .grad
            .ok_or_else(|| self.ctx.stop(SEXP_NIL, "invalid NULL gradient"))?;
        self.update_point(x);
        self.gradient.fill(0.0);
        Callback::Gradient(
            grad,
            self.point.len() as c_int,
            self.point.as_mut_ptr(),
            self.gradient.as_mut_ptr(),
        )
        .run(self.ctx, self.data)?;
        for (out, &index) in gradient.iter_mut().zip(&self.active) {
            *out = self.gradient[index];
        }
        self.gradient_norm = gradient.iter().fold(0.0, |norm, &x| x.mul_add(x, norm));
        Ok(())
    }
}

impl Anneal for ForeignObjective<'_> {
    fn candidate(&mut self, x: &[f64], scale: f64, next: &mut [f64]) -> EvalResult<()> {
        random::draw(self.ctx, |rng| {
            for (out, &value) in next.iter_mut().zip(x) {
                *out = scale.mul_add(rng.norm_rand(), value);
            }
        })
    }

    fn uniform(&mut self) -> EvalResult<f64> {
        random::draw(self.ctx, |rng| rng.unif_rand())
    }
}

fn control(maxit: c_int, abstol: f64, reltol: f64) -> Control {
    Control {
        maxit: maxit.max(0) as u32,
        abstol,
        reltol,
    }
}

fn dimension(ctx: &mut Ctx, n: c_int) -> EvalResult<usize> {
    let n = usize::try_from(n).map_err(|_| ctx.stop(SEXP_NIL, "negative optimizer dimension"))?;
    check_capacity(ctx, n, 4 * size_of::<f64>())?;
    Ok(n)
}

unsafe fn input<T>(ctx: &mut Ctx, ptr: *const T, len: usize) -> EvalResult<&[T]> {
    if len == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        return Err(ctx.stop(SEXP_NIL, "invalid NULL numerical input"));
    }
    // SAFETY: The caller guarantees a readable input array of this length.
    Ok(unsafe { slice::from_raw_parts(ptr, len) })
}

struct OptimOutput {
    parameters: *mut f64,
    minimum: *mut f64,
    status: *mut c_int,
    fn_count: *mut c_int,
    gr_count: *mut c_int,
}

impl OptimOutput {
    unsafe fn write(self, out: &OptimOut) {
        // SAFETY: All non-null outputs point to writable C storage.
        unsafe {
            write_coordinates(self.parameters, &out.par);
            store(self.minimum, out.value);
            store(self.status, out.convergence);
            store(self.fn_count, out.fncount.min(c_int::MAX as u32) as c_int);
            if let Some(count) = out.grcount {
                store(self.gr_count, count.min(c_int::MAX as u32) as c_int);
            }
        }
    }
}

unsafe fn write_coordinates(parameters: *mut f64, values: &[f64]) {
    if !parameters.is_null() && !values.is_empty() {
        // SAFETY: The caller provides a writable array with one slot per coordinate.
        unsafe { copy_nonoverlapping(values.as_ptr(), parameters, values.len()) };
    }
}

unsafe fn store<T>(ptr: *mut T, value: T) {
    if !ptr.is_null() {
        // SAFETY: Caller supplies writable storage for a non-null pointer.
        unsafe { ptr.write(value) };
    }
}

/// Minimize a function with the Nelder–Mead simplex method.
///
/// # Safety
/// Use an active native trampoline. Callbacks and buffers must meet `Applic.h`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nmmin(
    n: c_int,
    initial: *mut f64,
    parameters: *mut f64,
    minimum: *mut f64,
    fun: Option<OptimFn>,
    status: *mut c_int,
    abstol: f64,
    reltol: f64,
    data: *mut c_void,
    alpha: f64,
    beta: f64,
    gamma: f64,
    trace: c_int,
    fn_count: *mut c_int,
    maxit: c_int,
) {
    // SAFETY: The caller supplies live callbacks, arrays and an active trampoline.
    unsafe {
        api((), |ctx| {
            let n = dimension(ctx, n)?;
            check_capacity(ctx, n.saturating_mul(n + 1), size_of::<f64>())?;
            let par = input(ctx, initial, n)?.to_vec();
            let mut f = ForeignObjective::new(ctx, fun, None, data, par.clone(), (0..n).collect())?;
            f.initial_error =
                (maxit > 0).then_some("function cannot be evaluated at initial parameters");
            let control = SimplexControl {
                common: control(maxit, abstol, reltol),
                alpha,
                beta,
                gamma,
            };
            let out = optim::nelder_mead_with_report(&mut f, par, control, |f, event| {
                f.report(trace, 1, event)
            })?;
            let parameters = if maxit > 0 {
                write_coordinates(initial, &f.point);
                parameters
            } else {
                std::ptr::null_mut()
            };
            OptimOutput {
                parameters,
                minimum,
                status,
                fn_count,
                gr_count: std::ptr::null_mut(),
            }
            .write(&out);
            Ok(())
        });
    }
}

/// Minimize a function with BFGS on the coordinates selected by `mask`.
///
/// # Safety
/// Use an active native trampoline. Callbacks and buffers must meet `Applic.h`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vmmin(
    n: c_int,
    parameters: *mut f64,
    minimum: *mut f64,
    fun: Option<OptimFn>,
    grad: Option<OptimGr>,
    maxit: c_int,
    trace: c_int,
    mask: *mut c_int,
    abstol: f64,
    reltol: f64,
    report: c_int,
    data: *mut c_void,
    fn_count: *mut c_int,
    gr_count: *mut c_int,
    status: *mut c_int,
) {
    // SAFETY: The caller supplies live callbacks, arrays and an active trampoline.
    unsafe {
        api((), |ctx| {
            if maxit > 0 && report <= 0 {
                return Err(ctx.stop(SEXP_NIL, "REPORT must be > 0 (method = \"BFGS\")"));
            }
            let n = dimension(ctx, n)?;
            let original = input(ctx, parameters, n)?.to_vec();
            let active: Vec<_> = input(ctx, mask, n)?
                .iter()
                .enumerate()
                .filter_map(|(i, &include)| (include != 0).then_some(i))
                .collect();
            check_capacity(
                ctx,
                active.len().saturating_mul(active.len()),
                size_of::<f64>(),
            )?;
            let par = active.iter().map(|&i| original[i]).collect();
            let mut f = ForeignObjective::new(ctx, fun, grad, data, original, active)?;
            f.initial_error = (maxit > 0).then_some("initial value in 'vmmin' is not finite");
            let mut out = optim::bfgs_with_report(
                &mut f,
                par,
                control(maxit, abstol, reltol),
                |f, event| {
                    f.report(trace, report.max(1) as u32, event);
                },
            )?;
            f.update_point(&out.par);
            out.par = f.point;
            OptimOutput {
                parameters,
                minimum,
                status,
                fn_count,
                gr_count,
            }
            .write(&out);
            Ok(())
        });
    }
}

/// Minimize a function with nonlinear conjugate gradients.
///
/// # Safety
/// Use an active native trampoline. Callbacks and buffers must meet `Applic.h`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cgmin(
    n: c_int,
    initial: *mut f64,
    parameters: *mut f64,
    minimum: *mut f64,
    fun: Option<OptimFn>,
    grad: Option<OptimGr>,
    status: *mut c_int,
    abstol: f64,
    reltol: f64,
    data: *mut c_void,
    formula: c_int,
    trace: c_int,
    fn_count: *mut c_int,
    gr_count: *mut c_int,
    maxit: c_int,
) {
    // SAFETY: The caller supplies live callbacks, arrays and an active trampoline.
    unsafe {
        api((), |ctx| {
            let formula = match formula {
                1 => CgFormula::FletcherReeves,
                2 => CgFormula::PolakRibiere,
                3 => CgFormula::BealeSorenson,
                _ => return Err(ctx.stop(SEXP_NIL, "unknown 'type' in CG method")),
            };
            let n = dimension(ctx, n)?;
            let par = input(ctx, initial, n)?.to_vec();
            let mut f = ForeignObjective::new(ctx, fun, grad, data, par.clone(), (0..n).collect())?;
            f.initial_error =
                (maxit > 0).then_some("function cannot be evaluated at initial parameters");
            let out = cg_with_restarts(
                &mut f,
                par,
                control(maxit, abstol, reltol),
                formula,
                |f, event| {
                    if let Progress::CgStep(_, _, _, _, point) = event {
                        write_coordinates(initial, point);
                    }
                    f.report(trace, 1, event);
                },
            )?;
            let parameters = if maxit > 0 {
                parameters
            } else {
                std::ptr::null_mut()
            };
            OptimOutput {
                parameters,
                minimum,
                status,
                fn_count,
                gr_count,
            }
            .write(&out);
            Ok(())
        });
    }
}

fn cg_with_restarts(
    f: &mut ForeignObjective<'_>,
    mut par: Vec<f64>,
    control: Control,
    formula: CgFormula,
    mut progress: impl FnMut(&mut ForeignObjective<'_>, Progress<'_>),
) -> EvalResult<OptimOut> {
    let threshold = control.reltol * par.len() as f64 * control.reltol.sqrt();
    let (mut fn_count, mut gr_count) = (0_u32, 0_u32);
    loop {
        let mut finished = false;
        let mut moved = false;
        let remaining = control.maxit - gr_count;
        let cached = u32::from(f.cached_value.is_some());
        let mut out = optim::cg_with_report(
            f,
            par,
            Control {
                maxit: remaining,
                ..control
            },
            formula,
            |f, event| match event {
                Progress::Finish(_) => finished = true,
                Progress::CgStep(restart, iter, count, value, x) => {
                    moved |= iter > 0;
                    progress(
                        f,
                        Progress::CgStep(
                            restart,
                            gr_count.saturating_add(iter),
                            fn_count.saturating_add(count.saturating_sub(cached)),
                            value,
                            x,
                        ),
                    );
                }
                other => progress(f, other),
            },
        )?;
        fn_count = fn_count.saturating_add(out.fncount.saturating_sub(cached));
        gr_count = gr_count.saturating_add(out.grcount.unwrap_or(0));
        out.fncount = fn_count;
        out.grcount = Some(gr_count);
        // The update formula can end a search even when the gradient still
        // exceeds the tolerance. Start a new search from the accepted point.
        if matches!(formula, CgFormula::FletcherReeves)
            || !finished
            || !moved
            || !f.gradient_norm.is_finite()
            || f.gradient_norm <= threshold
            || out.value <= control.abstol
        {
            if finished {
                progress(f, Progress::Finish(fn_count));
            }
            return Ok(out);
        }
        if gr_count >= control.maxit {
            out.convergence = 1;
            out.grcount = Some(gr_count.saturating_add(1));
            return Ok(out);
        }
        f.cached_value = Some(out.value);
        par = out.par;
    }
}

/// Minimize a function with limited-memory BFGS and box constraints.
///
/// # Safety
/// Use an active native trampoline. Callbacks and buffers must meet `Applic.h`.
/// `message` must hold 60 bytes, including the terminator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lbfgsb(
    n: c_int,
    m: c_int,
    parameters: *mut f64,
    lower: *mut f64,
    upper: *mut f64,
    bounds: *mut c_int,
    minimum: *mut f64,
    fun: Option<OptimFn>,
    grad: Option<OptimGr>,
    status: *mut c_int,
    data: *mut c_void,
    factr: f64,
    pgtol: f64,
    fn_count: *mut c_int,
    gr_count: *mut c_int,
    maxit: c_int,
    message: *mut c_char,
    trace: c_int,
    report: c_int,
) {
    // SAFETY: The caller supplies live callbacks, arrays and an active trampoline.
    unsafe {
        api((), |ctx| {
            let n = dimension(ctx, n)?;
            let lmm = m.max(0) as usize;
            check_capacity(
                ctx,
                n.saturating_mul(lmm.min(maxit.max(0) as usize)),
                4 * size_of::<f64>(),
            )?;
            let codes = input(ctx, bounds, n)?.to_vec();
            let output = OptimOutput {
                parameters,
                minimum,
                status,
                fn_count,
                gr_count,
            };
            if codes.iter().any(|&code| !(0..=3).contains(&code)) {
                store(minimum, f64::INFINITY);
                store(status, 52);
                store(fn_count, 0);
                store(gr_count, 0);
                write_message(message, "ERROR: INVALID NBD");
                return Ok(());
            }
            let mut intervals = Vec::with_capacity(n);
            for (i, &code) in codes.iter().enumerate() {
                let lo = if matches!(code, 1 | 2) {
                    bound_at(ctx, lower, i)?
                } else {
                    f64::NEG_INFINITY
                };
                let hi = if matches!(code, 2 | 3) {
                    bound_at(ctx, upper, i)?
                } else {
                    f64::INFINITY
                };
                intervals.push((lo, hi));
            }
            let par = input(ctx, parameters, n)?.to_vec();
            let mut f = ForeignObjective::new(ctx, fun, grad, data, par.clone(), (0..n).collect())?;
            let control = BoxControl {
                maxit: maxit.max(0) as u32,
                lmm,
                factr,
                pgtol,
            };
            let out = optim::lbfgsb_with_report(&mut f, par, &intervals, control, |f, event| {
                f.report(trace, report.max(1) as u32, event);
            })?;
            write_message(message, out.message.as_deref().unwrap_or(""));
            output.write(&out);
            Ok(())
        });
    }
}

unsafe fn bound_at(ctx: &mut Ctx, bounds: *const f64, index: usize) -> EvalResult<f64> {
    if bounds.is_null() {
        return Err(ctx.stop(SEXP_NIL, "invalid NULL bounds array"));
    }
    // SAFETY: The bound code selects an initialized element in the caller's array.
    Ok(unsafe { bounds.add(index).read() })
}

unsafe fn write_message(message: *mut c_char, text: &str) {
    if !message.is_null() {
        let len = text.len().min(59);
        // SAFETY: The caller supplies a 60-byte output buffer.
        unsafe {
            copy_nonoverlapping(text.as_ptr(), message.cast(), len);
            message.add(len).write(0);
        }
    }
}

/// Minimize a function with simulated annealing and the native random stream.
///
/// # Safety
/// Use an active native trampoline. Callbacks and buffers must meet `Applic.h`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn samin(
    n: c_int,
    parameters: *mut f64,
    minimum: *mut f64,
    fun: Option<OptimFn>,
    maxit: c_int,
    tmax: c_int,
    temperature: f64,
    trace: c_int,
    data: *mut c_void,
) {
    // SAFETY: The caller supplies live callbacks, arrays and an active trampoline.
    unsafe {
        api((), |ctx| {
            let n = dimension(ctx, n)?;
            let par = input(ctx, parameters, n)?.to_vec();
            let optimize = |ctx: &mut Ctx| {
                let mut f =
                    ForeignObjective::new(ctx, fun, None, data, par.clone(), (0..n).collect())?;
                let tmax = tmax.max(1) as u32;
                optim::sann_with_report(
                    &mut f,
                    par,
                    maxit.max(0) as u32,
                    temperature,
                    tmax,
                    |f, event| {
                        if let Progress::Iteration(iter, _) = event
                            && (trace <= 0 || !((iter - 1) / tmax + 1).is_multiple_of(trace as u32))
                        {
                            return;
                        }
                        f.report(trace, 1, event);
                    },
                )
            };
            let out = if n == 0 {
                optimize(ctx)
            } else {
                random::with_synced_state(ctx, optimize)
            }?;
            write_coordinates(parameters, &out.par);
            store(minimum, out.value);
            Ok(())
        });
    }
}

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

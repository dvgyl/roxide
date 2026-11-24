use std::ffi::{c_char, c_int, c_void};
use std::ptr::copy_nonoverlapping;

use roxide_eval::{Ctx, EvalResult};
use roxide_math::optim::{
    self, BoxControl, CgFormula, Control, OptimOut, Progress, SimplexControl,
};
use roxide_value::SEXP_NIL;

use super::super::{api, random, vector::check_capacity};
use super::{ForeignObjective, OptimFn, OptimGr, dimension, input, store, write_coordinates};

fn control(maxit: c_int, abstol: f64, reltol: f64) -> Control {
    Control {
        maxit: maxit.max(0) as u32,
        abstol,
        reltol,
    }
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

//! C callbacks and output buffers for optimization and quadrature.

#![allow(non_snake_case)]

mod optim;
mod quadpack;

pub use optim::*;
pub use quadpack::*;

use std::ffi::{c_int, c_void};
use std::ptr::{copy_nonoverlapping, from_mut};
use std::slice;

use roxide_eval::{Ctx, EvalResult, Unwind};
use roxide_math::Objective;
use roxide_math::optim::{Anneal, Progress};
use roxide_value::SEXP_NIL;

use crate::jump::{FrameKind, VoidFn, enter_trampoline_with};

use super::{random, vector::check_capacity};

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

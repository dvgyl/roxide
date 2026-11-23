use roxide_eval::{Args, Call, Ctx, EvalResult, Unwind};
use roxide_gc::SexpId;
use roxide_math::{VecFn, fmin::fmin, quadpack, roots::zeroin};
use roxide_value::{FormatOpts, NA_INTEGER, SEXP_NIL, SexpType, ValueApi, format_real_vec};

use super::{callback, named_list};
use crate::distn::support::{limit, logical, number, numbers, numeric, set_attr};

fn scalar_value(ctx: &mut Ctx, call: &Call, fun: SexpId, x: f64, routine: &str) -> EvalResult<f64> {
    ctx.scoped(|ctx| {
        let result = callback(ctx, call, fun, &[x], SEXP_NIL)?;
        if !matches!(
            ctx.session().type_of(result)?,
            SexpType::Int | SexpType::Real
        ) || ctx.session().xlength(result)? != 1
        {
            return Err(ctx.stop(
                call.err_call(),
                format!("invalid function value in '{routine}'"),
            ));
        }
        number(ctx, call, result)
    })
}

pub(super) fn minimize(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    ctx.scoped(|ctx| {
        let fun = args.get(0);
        ctx.protect(fun);
        ctx.protect(call.env);
        let a = number(ctx, call, args.get(1))?;
        let b = number(ctx, call, args.get(2))?;
        let maximum = logical(ctx, call, args.get(3))?
            .ok_or_else(|| ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"))?;
        let tol = number(ctx, call, args.get(4))?;
        for (x, message) in [(a, "invalid 'xmin' value"), (b, "invalid 'xmax' value")] {
            if !x.is_finite() {
                return Err(ctx.stop(call.err_call(), message));
            }
        }
        if a >= b {
            return Err(ctx.stop(call.err_call(), "'xmin' not less than 'xmax'"));
        }
        if !tol.is_finite() || tol <= 0.0 {
            return Err(ctx.stop(call.err_call(), "invalid 'tol' value"));
        }
        let sign = if maximum { -1.0 } else { 1.0 };
        let (x, _) = fmin(
            |x| {
                let v = sign * scalar_value(ctx, call, fun, x, "optimize")?;
                finite_for_brent(ctx, call, v)
            },
            a,
            b,
            tol,
        )?;
        let objective = callback(ctx, call, fun, &[x], SEXP_NIL)?;
        let location = ctx.session_mut().scalar_real(x)?;
        ctx.protect(location);
        named_list(
            ctx,
            &[if maximum { "maximum" } else { "minimum" }, "objective"],
            &[location, objective],
        )
    })
}

pub(super) fn finite_for_brent(ctx: &mut Ctx, call: &Call, value: f64) -> EvalResult<f64> {
    if value.is_finite() {
        return Ok(value);
    }
    let (replacement, message) = if value == f64::NEG_INFINITY {
        (-f64::MAX, "-Inf replaced by maximally negative value")
    } else if value.is_infinite() {
        (f64::MAX, "Inf replaced by maximum positive value")
    } else {
        (f64::MAX, "NA/NaN replaced by maximum positive value")
    };
    ctx.warn_now(call.err_call(), message)?;
    Ok(replacement)
}

struct Bracket {
    a: f64,
    b: f64,
    fa: f64,
    fb: f64,
}

impl Bracket {
    fn extend(
        &mut self,
        ctx: &mut Ctx,
        call: &Call,
        fun: SexpId,
        direction: &str,
        maxit: u32,
        trace: f64,
    ) -> EvalResult<u32> {
        let sign = if direction == "upX" { 1.0 } else { -1.0 };
        let need_extension = |a: f64, b: f64| {
            if direction == "yes" {
                a != 0.0 && b != 0.0 && a.signum() == b.signum()
            } else {
                sign * a > 0.0 || sign * b < 0.0
            }
        };
        if !need_extension(self.fa, self.fb) {
            return Ok(0);
        }
        if trace != 0.0 {
            let msg = format!(
                "search in [{},{}]\n",
                display(ctx, self.a, 7),
                display(ctx, self.b, 7)
            );
            let _ = ctx.host().write_out(msg.as_bytes());
        }
        let mut da = 0.01 * self.a.abs().max(0.0001);
        let mut db = 0.01 * self.b.abs().max(0.0001);
        for i in 1..=maxit {
            let lower = if direction == "yes" {
                self.fa.is_finite()
            } else {
                sign * self.fa > 0.0
            };
            let upper = if direction == "yes" {
                self.fb.is_finite()
            } else {
                sign * self.fb < 0.0
            };
            for (change, x, fx, delta, label, step) in [
                (lower, &mut self.a, &mut self.fa, &mut da, "lower", -1.0_f64),
                (upper, &mut self.b, &mut self.fb, &mut db, "upper", 1.0),
            ] {
                if change {
                    let next = step.mul_add(*delta, *x);
                    let v = scalar_value(ctx, call, fun, next, "zeroin")?;
                    if direction == "yes" && v.is_nan() {
                        *delta /= 4.0;
                    } else {
                        (*x, *fx) = (next, v);
                        if trace >= 2.0 && direction != "yes" {
                            let msg = format!(" .. modified {label}: {}\n", display(ctx, next, 7));
                            let _ = ctx.host().write_out(msg.as_bytes());
                        }
                        *delta *= 2.0;
                    }
                }
            }
            if trace >= 2.0 && direction == "yes" {
                let lower = display(ctx, self.a, 6);
                let upper = display(ctx, self.b, 6);
                let msg = format!(" .. modified lower,upper: ({lower:>15},{upper:>15})\n");
                let _ = ctx.host().write_out(msg.as_bytes());
            }
            if self.fa.is_nan() || self.fb.is_nan() {
                return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
            }
            if !need_extension(self.fa, self.fb) {
                return Ok(i);
            }
        }
        Err(ctx.stop(
            call.err_call(),
            format!("no sign change found in {maxit} iterations"),
        ))
    }
}

pub(super) fn root(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    ctx.scoped(|ctx| {
        let fun = args.get(0);
        ctx.protect(fun);
        ctx.protect(call.env);
        let mut interval = Bracket {
            a: number(ctx, call, args.get(1))?,
            b: number(ctx, call, args.get(2))?,
            fa: number(ctx, call, args.get(3))?,
            fb: number(ctx, call, args.get(4))?,
        };
        for (value, message) in [
            (interval.fa, "f.lower = f(lower) is NA"),
            (interval.fb, "f.upper = f(upper) is NA"),
        ] {
            if value.is_nan() {
                return Err(ctx.stop(call.err_call(), message));
            }
        }
        let direction = string(ctx, args.get(5))?;
        let check = logical(ctx, call, args.get(6))?.unwrap_or(false);
        let tol = number(ctx, call, args.get(7))?;
        let iteration_limit = number(ctx, call, args.get(8))?;
        let maxit = (iteration_limit as i32).max(0) as u32;
        let trace = number(ctx, call, args.get(9))?;
        let init = if direction == "no" {
            0
        } else {
            interval.extend(ctx, call, fun, &direction, maxit, trace)?
        };
        if interval.fa != 0.0 && interval.fb != 0.0 && interval.fa.signum() == interval.fb.signum()
        {
            let msg = if direction == "no" {
                "f() values at end points not of opposite sign"
            } else {
                "did not succeed extending the interval endpoints for f(lower) * f(upper) <= 0"
            };
            return Err(ctx.stop(call.err_call(), msg));
        }
        if !tol.is_finite() || tol <= 0.0 {
            return Err(ctx.stop(call.err_call(), "invalid 'tol' value"));
        }
        if maxit == 0 {
            return Err(ctx.stop(call.err_call(), "'maxiter' must be positive"));
        }
        let mut evaluate = |x| {
            let v = scalar_value(ctx, call, fun, x, "zeroin")?;
            if v.is_nan() {
                return Err(ctx.stop(call.err_call(), "invalid function value in 'zeroin'"));
            }
            Ok(v.clamp(-f64::MAX, f64::MAX))
        };
        let result = zeroin(
            &mut evaluate,
            interval.a,
            interval.b,
            interval.fa.clamp(-f64::MAX, f64::MAX),
            interval.fb.clamp(-f64::MAX, f64::MAX),
            tol,
            maxit,
        )?;
        if !result.converged && iteration_limit.fract() != 0.0 {
            return Err(ctx.stop(
                call.err_call(),
                "invalid format '%d'; use format %f, %e, %g or %a for numeric objects",
            ));
        }
        if !result.converged && check {
            return Err(ctx.stop(
                call.err_call(),
                format!(
                    "_NOT_ converged in {maxit} iteration{}",
                    if maxit == 1 { "" } else { "s" }
                ),
            ));
        }
        if !result.converged {
            ctx.warn_now(
                call.err_call(),
                format!(
                    "_NOT_ converged in {maxit} iteration{}",
                    if maxit == 1 { "" } else { "s" }
                ),
            )?;
        }
        let fx = callback(ctx, call, fun, &[result.root], SEXP_NIL)?;
        let root = ctx.session_mut().scalar_real(result.root)?;
        ctx.protect(root);
        let iterations = result.iter.saturating_add(init);
        let iter = if result.converged {
            ctx.session_mut().scalar_int(iterations as i32)?
        } else if init == 0 {
            args.get(8)
        } else if ctx.session().type_of(args.get(8))? == SexpType::Int {
            ctx.session_mut().scalar_int(iterations as i32)?
        } else {
            ctx.session_mut().scalar_real(f64::from(iterations))?
        };
        ctx.protect(iter);
        let init =
            ctx.session_mut()
                .scalar_int(if init == 0 { NA_INTEGER } else { init as i32 })?;
        ctx.protect(init);
        let precision = ctx.session_mut().scalar_real(result.estim_prec)?;
        named_list(
            ctx,
            &["root", "f.root", "iter", "init.it", "estim.prec"],
            &[root, fx, iter, init, precision],
        )
    })
}

struct Integrand<'a> {
    ctx: &'a mut Ctx,
    call: &'a Call,
    fun: SexpId,
}

impl VecFn for Integrand<'_> {
    type Err = Unwind;

    fn eval(&mut self, x: &[f64], out: &mut [f64]) -> EvalResult<()> {
        self.ctx.scoped(|ctx| {
            let result = callback(ctx, self.call, self.fun, x, SEXP_NIL)?;
            if ctx.session().xlength(result)? != x.len() {
                return Err(ctx.stop(
                    self.call.err_call(),
                    "evaluation of function gave a result of wrong length",
                ));
            }
            if !numeric(ctx, result)? || ctx.session().type_of(result)? == SexpType::Lgl {
                return Err(ctx.stop(
                    self.call.err_call(),
                    "evaluation of function gave a result of wrong type",
                ));
            }
            let values = numbers(ctx, self.call, result)?;
            if values.iter().any(|v| !v.is_finite()) {
                return Err(ctx.stop(self.call.err_call(), "non-finite function value"));
            }
            out.copy_from_slice(&values);
            Ok(())
        })
    }
}

pub(super) fn finite_integral(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    integrate(ctx, call, args, true)
}

pub(super) fn infinite_integral(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    integrate(ctx, call, args, false)
}

fn integrate(ctx: &mut Ctx, call: &Call, args: &mut Args, finite: bool) -> EvalResult {
    ctx.scoped(|ctx| {
        let fun = args.get(0);
        let matched = args.get(7);
        for id in [fun, call.env, matched] {
            ctx.protect(id);
        }
        let a = number(ctx, call, args.get(1))?;
        let b = number(ctx, call, args.get(2))?;
        let limit_arg = number(ctx, call, args.get(3))? as usize;
        let rel = number(ctx, call, args.get(4))?;
        let abs = number(ctx, call, args.get(5))?;
        let stop = logical(ctx, call, args.get(6))?;
        if a.is_nan() || b.is_nan() {
            return Err(ctx.stop(call.err_call(), "a limit is NA or NaN"));
        }
        if limit_arg == 0 || rel.is_nan() || abs.is_nan() || abs <= 0.0 && rel < 50.0 * f64::EPSILON
        {
            return Err(ctx.stop(call.err_call(), "invalid parameter values"));
        }
        limit::<[f64; 8]>(ctx, call, limit_arg)?;
        let mut f = Integrand { ctx, call, fun };
        let result = if finite {
            quadpack::dqags(&mut f, a, b, abs, rel, limit_arg)?
        } else {
            quadpack::dqagi(&mut f, a, b, abs, rel, limit_arg)?
        };
        if result.status == quadpack::Status::NonFinite {
            return Err(ctx.stop(call.err_call(), result.status.message()));
        }
        if result.status != quadpack::Status::Ok
            && stop
                .ok_or_else(|| ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"))?
        {
            return Err(ctx.stop(call.err_call(), result.status.message()));
        }
        let value = ctx.session_mut().scalar_real(result.value)?;
        ctx.protect(value);
        let error = ctx.session_mut().scalar_real(result.abs_error)?;
        ctx.protect(error);
        let count = ctx.session_mut().scalar_int(result.subdivisions as i32)?;
        ctx.protect(count);
        let message = ctx.session_mut().scalar_str(result.status.message())?;
        let out = named_list(
            ctx,
            &["value", "abs.error", "subdivisions", "message", "call"],
            &[value, error, count, message, matched],
        )?;
        ctx.protect(out);
        let class = ctx.session_mut().scalar_str("integrate")?;
        ctx.protect(class);
        set_attr(ctx, out, "class", class)?;
        Ok(out)
    })
}

pub(super) fn string(ctx: &Ctx, x: SexpId) -> EvalResult<String> {
    let chars = ctx.session().str_slice(x)?;
    let bytes = match chars.first() {
        Some(&id) => ctx.session().char_bytes(id)?,
        None => b"",
    };
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

pub(super) fn display(ctx: &Ctx, value: f64, digits: usize) -> String {
    let format = FormatOpts {
        digits,
        scipen: ctx.opts().scipen,
    };
    format_real_vec(&[value], format)
        .into_iter()
        .next()
        .unwrap_or_default()
}

pub(super) fn print(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let digits = number(ctx, call, args.get(1))?.clamp(1.0, 22.0) as usize;
    let message = string(ctx, ctx.session().list_get(x, 3)?)?;
    let line = if message == "OK" {
        let value = number(ctx, call, ctx.session().list_get(x, 0)?)?;
        let error = number(ctx, call, ctx.session().list_get(x, 1)?)?;
        format!(
            "{} with absolute error < {}\n",
            display(ctx, value, digits),
            display(ctx, error, 2)
        )
    } else {
        format!("failed with message ‘{message}’\n")
    };
    let _ = ctx.host().write_out(line.as_bytes());
    Ok(x)
}

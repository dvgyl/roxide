//! Base R bindings for real special functions.

#![deny(missing_docs)]

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, GroupGeneric, Module, PrimDef, PrimKind,
    Visibility,
};
use roxide_math::{
    Flags,
    special::{self, aux},
};
use roxide_value::{NA_REAL, RLogical, SEXP_MISSING, SexpType, ValueApi, is_na_real};

use crate::ops::{
    attributes::has_class,
    common::{copy_attrs, error, tag, vector_limit},
};

/// Base special functions and their argument-matching wrappers.
pub const MODULE: Module = Module {
    prims: &[
        unary("gamma", gamma),
        unary("lgamma", lgamma),
        unary("digamma", digamma),
        unary("trigamma", trigamma),
        unary("sinpi", sinpi),
        unary("cospi", cospi),
        unary("tanpi", tanpi),
        internal("psigamma", 2, psigamma),
        internal("beta", 2, beta),
        internal("lbeta", 2, lbeta),
        internal("choose", 2, choose),
        internal("lchoose", 2, lchoose),
        internal("besselI", 3, bessel_i),
        internal("besselJ", 2, bessel_j),
        internal("besselK", 3, bessel_k),
        internal("besselY", 2, bessel_y),
    ],
    prelude: r#"
factorial <- function(x) gamma(x + 1)
lfactorial <- function(x) lgamma(x + 1)
psigamma <- function(x, deriv = 0) .Internal(psigamma(x, deriv))
beta <- function(a, b) .Internal(beta(a, b))
lbeta <- function(a, b) .Internal(lbeta(a, b))
choose <- function(n, k) .Internal(choose(n, k))
lchoose <- function(n, k) .Internal(lchoose(n, k))
besselI <- function(x, nu, expon.scaled = FALSE) .Internal(besselI(x, nu, expon.scaled))
besselJ <- function(x, nu) .Internal(besselJ(x, nu))
besselK <- function(x, nu, expon.scaled = FALSE) .Internal(besselK(x, nu, expon.scaled))
besselY <- function(x, nu) .Internal(besselY(x, nu))
"#,
};

const fn unary(name: &'static str, fun: roxide_eval::PrimFn) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Primitive,
        args: ArgEval::Eval,
        arity: Arity::Any,
        dispatch: Dispatch::Group(GroupGeneric::Math),
        visibility: Visibility::On,
        fun,
    }
}

const fn internal(name: &'static str, arity: usize, fun: roxide_eval::PrimFn) -> PrimDef {
    PrimDef {
        kind: PrimKind::Internal,
        name,
        args: ArgEval::Eval,
        arity: Arity::Exact(arity),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
        fun,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Gamma,
    Lgamma,
    Digamma,
    Trigamma,
    Sinpi,
    Cospi,
    Tanpi,
    Psigamma,
    Beta,
    Lbeta,
    Choose,
    Lchoose,
    BesselI,
    BesselJ,
    BesselK,
    BesselY,
}

macro_rules! bindings {
    ($($name:ident: $op:ident),* $(,)?) => {$(
        fn $name(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            evaluate(ctx, call, args, Operation::$op)
        }
    )*};
}

bindings!(
    gamma: Gamma, lgamma: Lgamma, digamma: Digamma, trigamma: Trigamma,
    sinpi: Sinpi, cospi: Cospi, tanpi: Tanpi, psigamma: Psigamma,
    beta: Beta, lbeta: Lbeta, choose: Choose, lchoose: Lchoose,
    bessel_i: BesselI, bessel_j: BesselJ, bessel_k: BesselK, bessel_y: BesselY,
);

impl Operation {
    fn apply(self, x: f64, y: f64, scaled: bool, flags: &mut Flags) -> f64 {
        match self {
            Self::Gamma => special::gamma(x, flags),
            Self::Lgamma => special::lgamma(x, flags),
            Self::Digamma => special::digamma(x, flags),
            Self::Trigamma => special::trigamma(x, flags),
            Self::Sinpi => aux::sinpi(x),
            Self::Cospi => aux::cospi(x),
            Self::Tanpi => aux::tanpi(x),
            Self::Psigamma => special::psigamma(x, y.round_ties_even() as i32, flags),
            Self::Beta => special::beta(x, y, flags),
            Self::Lbeta => special::lbeta(x, y, flags),
            Self::Choose => special::choose(x, y, flags),
            Self::Lchoose => special::lchoose(x, y, flags),
            Self::BesselI => special::bessel_i(x, y, scaled, flags),
            Self::BesselJ => special::bessel_j(x, y, flags),
            Self::BesselK => special::bessel_k(x, y, scaled, flags),
            Self::BesselY => special::bessel_y(x, y, flags),
        }
    }

    fn unary_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Gamma => "gamma",
            Self::Lgamma => "lgamma",
            Self::Digamma => "digamma",
            Self::Trigamma => "trigamma",
            Self::Sinpi => "sinpi",
            Self::Cospi => "cospi",
            Self::Tanpi => "tanpi",
            _ => return None,
        })
    }

    fn bessel(self) -> Option<(char, f64)> {
        Some(match self {
            Self::BesselI => ('i', 1e9),
            Self::BesselJ => ('j', 1e9),
            Self::BesselK => ('k', 1e9),
            Self::BesselY => ('y', 1e7),
            _ => return None,
        })
    }

    fn warning(self, x: f64, y: f64, flags: Flags) -> Option<String> {
        if flags.has(Flags::NONINT_X) {
            let k = y.round_ties_even();
            return Some(format!("'k' ({y:.2}) must be integer, rounded to {k:.0}"));
        }
        if self == Self::Psigamma && y.round_ties_even() as i32 > 100 {
            return Some(format!(
                "deriv = {} > 100 (= n_max)",
                y.round_ties_even() as i32
            ));
        }
        if let Some((kind, max_order)) = self.bessel() {
            if x < 0.0 {
                return Some(format!("value out of range in 'bessel_{kind}'"));
            }
            if y.abs() > max_order {
                let order = short_number(y.abs());
                let message = match kind {
                    'y' if y < 0.0 => format!(
                        "besselY(x, -nu): nu = {order} > 1e+07 =: max_alpha_y, too large for bessel_y() algorithm"
                    ),
                    'y' => format!(
                        "besselY(x, nu): nu={order} > 1e+07 =: max_alpha_y, too large for bessel_y() algorithm"
                    ),
                    _ => {
                        let name = kind.to_ascii_uppercase();
                        let order_name = if kind == 'k' { "|nu|" } else { "nu" };
                        format!(
                            "bessel{name}(x, nu): {order_name}={order} > max_alpha_{kind} (= 1e+09): too large for bessel_{kind}() algorithm"
                        )
                    }
                };
                return Some(message);
            }
            if flags.has(Flags::UNDERFLOW) {
                return Some("value out of range in 'J_bessel'".to_owned());
            }
            if flags.has(Flags::PRECISION) {
                let (x, order) = (short_number(x), short_number(y.abs()));
                return Some(format!(
                    "bessel_{kind}({x},nu={order}): precision lost in result"
                ));
            }
        }
        if self == Self::Gamma && flags.has(Flags::UNDERFLOW) {
            return Some("value out of range in 'gammafn'".to_owned());
        }
        if flags.has(Flags::PRECISION) {
            let name = if self == Self::Gamma {
                "gammafn"
            } else {
                "lgamma"
            };
            return Some(format!(
                "full precision may not have been achieved in '{name}'"
            ));
        }
        None
    }
}

fn evaluate(ctx: &mut Ctx, call: &Call, args: &Args, op: Operation) -> EvalResult {
    if let Some(name) = op.unary_name() {
        if args.len() != 1 {
            return Err(error(
                ctx,
                call,
                format!(
                    "{} arguments passed to '{name}' which requires 1",
                    args.len()
                ),
            ));
        }
        if let Some(arg) = tag(ctx.session(), args.name(0))
            && arg != "x"
        {
            let msg = format!("supplied argument name '{arg}' does not match 'x'");
            return Err(error(ctx, call, msg));
        }
        if args.get(0) == SEXP_MISSING {
            return Err(error(ctx, call, "argument 1 is empty"));
        }
    }

    let count = args.len().min(2);
    let mut numbers = [SEXP_MISSING; 2];
    let mut lengths = [1; 3];
    for (i, value) in numbers.iter_mut().enumerate().take(count) {
        let input = args.get(i);
        let s = ctx.session_mut();
        let ty = s.type_of(input)?;
        if ty == SexpType::Cplx {
            return Err(error(ctx, call, "unimplemented complex function"));
        }
        let factor = has_class(s, input, "factor");
        if !matches!(ty, SexpType::Int | SexpType::Lgl | SexpType::Real) || factor {
            let message = if factor {
                op.unary_name()
                    .map(|name| format!("‘{name}’ not meaningful for factors"))
            } else {
                None
            };
            return Err(error(
                ctx,
                call,
                message
                    .as_deref()
                    .unwrap_or("non-numeric argument to mathematical function"),
            ));
        }
        lengths[i] = s.xlength(input)?;
        vector_limit(ctx, call, lengths[i], size_of::<f64>())?;
        *value = ctx.session_mut().coerce(input, SexpType::Real)?.0;
        ctx.protect(*value);
    }
    let scale = if args.len() == 3 {
        let s = ctx.session_mut();
        let input = args.get(2);
        lengths[2] = s.xlength(input)?;
        vector_limit(ctx, call, lengths[2], size_of::<RLogical>())?;
        let value = ctx.session_mut().coerce(input, SexpType::Lgl)?.0;
        ctx.protect(value);
        Some(value)
    } else {
        None
    };

    let len = if lengths.contains(&0) {
        0
    } else {
        lengths[0].max(lengths[1]).max(lengths[2])
    };
    vector_limit(ctx, call, len, size_of::<f64>())?;
    let mut values = Vec::with_capacity(len);
    let mut nan_produced = false;
    for i in 0..len {
        let s = ctx.session();
        let x = s.real(numbers[0])?.get(i % lengths[0]);
        let y = if count == 2 {
            s.real(numbers[1])?.get(i % lengths[1])
        } else {
            0.0
        };
        let scaled = match scale {
            Some(value) => s.lgl(value)?.get(i % lengths[2]),
            None => RLogical::from_option(Some(false)),
        };
        let value = if is_na_real(x) || is_na_real(y) || scaled.get().is_none() {
            NA_REAL
        } else if x.is_nan() || y.is_nan() {
            f64::NAN
        } else {
            let mut flags = Flags::default();
            let value = op.apply(x, y, scaled.get() == Some(true), &mut flags);
            if let Some(warning) = op.warning(x, y, flags) {
                ctx.warn_now(call.err_call(), warning)?;
            }
            nan_produced |= value.is_nan();
            value
        };
        values.push(value);
    }
    if nan_produced {
        ctx.warn_now(call.err_call(), "NaNs produced")?;
    }

    let s = ctx.session_mut();
    let out = s.alloc_real_from(&values)?;
    if lengths[0] == len {
        copy_attrs(s, args.get(0), out, |_| true)?;
    } else if count == 2 && lengths[1] == len && len != 0 {
        copy_attrs(s, args.get(1), out, |_| true)?;
    }
    Ok(out)
}

// Format a diagnostic number as C's default %g conversion.
fn short_number(x: f64) -> String {
    if x.is_infinite() {
        return if x.is_sign_negative() { "-inf" } else { "inf" }.to_owned();
    }
    let scientific = format!("{x:.5e}");
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return scientific;
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return scientific;
    };
    if (-4..6).contains(&exponent) {
        let decimals = (5 - exponent) as usize;
        let fixed = format!("{x:.decimals$}");
        return if decimals == 0 {
            fixed
        } else {
            fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
        };
    }
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    format!("{mantissa}e{exponent:+03}")
}

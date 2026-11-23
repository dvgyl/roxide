use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{SexpType, ValueApi};

use crate::distn::support::{copy_attrs, has_class, number, numbers};

pub(super) fn initializer(ctx: &mut Ctx, _call: &Call, args: &mut Args) -> EvalResult {
    Ok(ctx.session_mut().alloc_expr_from(&[args.get(0)])?)
}

pub(super) fn link_math(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = args.get(0);
    let operation = number(ctx, call, args.get(1))?;
    let kind = ctx.session().type_of(input)?;
    let valid_type = matches!(kind, SexpType::Real | SexpType::Int)
        || (kind == SexpType::Lgl && operation != 1.0);
    if operation <= 3.0
        && (!valid_type || ctx.session().xlength(input)? == 0 || has_class(ctx, input, "factor")?)
    {
        let arg = if operation == 1.0 { "mu" } else { "eta" };
        return Err(ctx.stop(
            call.err_call(),
            format!("Argument {arg} must be a nonempty numeric vector"),
        ));
    }
    let mut values = numbers(ctx, call, input)?;
    let epsilon = f64::EPSILON;
    let mut invalid = false;
    for value in &mut values {
        let x = *value;
        if x.is_nan() {
            continue;
        }
        *value = match operation as u8 {
            1 => (x / (1.0 - x)).ln(),
            2 => {
                let odds = if x < -30.0 {
                    epsilon
                } else if x > 30.0 {
                    epsilon.recip()
                } else {
                    x.exp()
                };
                odds / (1.0 + odds)
            }
            3 if x.abs() > 30.0 => epsilon,
            3 => {
                let tail = (-x.abs()).exp();
                tail / (1.0 + tail).powi(2)
            }
            4 => (-(-x).ln_1p()).ln(),
            5 => (-(-x.exp()).exp_m1()).clamp(epsilon, 1.0 - epsilon),
            6 if x > 700.0 => epsilon,
            6 => (x - x.exp()).exp().max(epsilon),
            _ => return Err(ctx.stop(call.err_call(), "invalid link operation")),
        };
        invalid |= value.is_nan();
    }
    if invalid {
        ctx.warn_now(call.err_call(), "NaNs produced")?;
    }
    let result = ctx.session_mut().alloc_real_from(&values)?;
    ctx.protect(result);
    copy_attrs(ctx, input, result)?;
    Ok(result)
}

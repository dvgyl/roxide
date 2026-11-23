use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{Flags, special::digamma};
use roxide_value::ValueApi;

use crate::distn::support::{number, numbers};

pub(super) fn gamma_shapes(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut weights = numbers(ctx, call, args.get(0))?;
    let deviance = number(ctx, call, args.get(1))?;
    let count = weights.len() as f64;
    let target = deviance / 2.0;
    let mut low = count / deviance;
    let mut high = 2.0 * low;
    let mut flags = Flags::default();
    // The bounds follow from 1/(2x) < log(x) - digamma(x) < 1/x.
    for _ in 0..54 {
        let middle = low + (high - low) / 2.0;
        let score = weights.iter().fold(0.0, |sum, &weight| {
            let shape = weight * middle;
            let term = if shape > 1e5 {
                let reciprocal = shape.recip();
                reciprocal * (0.5 + reciprocal / 12.0)
            } else {
                shape.ln() - digamma(shape, &mut flags)
            };
            sum + weight * term
        });
        if score > target {
            low = middle;
        } else {
            high = middle;
        }
    }
    let multiplier = low + (high - low) / 2.0;
    for weight in &mut weights {
        *weight *= multiplier;
    }
    Ok(ctx.session_mut().alloc_real_from(&weights)?)
}

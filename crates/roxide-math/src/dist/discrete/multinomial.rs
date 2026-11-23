//! Multinomial masses and conditional binomial random draws.

use super::binomial::binomial_log;
use super::*;
use crate::core::MathError;
use crate::rng::Rng;

fn weight_scale(prob: &[f64]) -> Option<f64> {
    let mut max: f64 = 0.0;
    for &weight in prob {
        if !weight.is_finite() || weight < 0.0 {
            return None;
        }
        max = max.max(weight);
    }
    if max == 0.0 {
        return None;
    }
    // Scale only when the weight sum can overflow.
    Some(if max > f64::MAX / prob.len() as f64 {
        max
    } else {
        1.0
    })
}

/// Return the multinomial mass at the count vector `x`.
pub fn dmultinom(x: &[f64], prob: &[f64], log: bool, flags: &mut Flags) -> f64 {
    if x.len() != prob.len() {
        return invalid(flags);
    }
    let Some(scale) = weight_scale(prob) else {
        return invalid(flags);
    };
    let mut count = 0.0;
    let mut weight = 0.0;
    let mut result = 0.0;
    for (&x, &p) in x.iter().zip(prob).rev() {
        let x = x.trunc();
        if !x.is_finite() || x < 0.0 || x > INT_MAX {
            return invalid(flags);
        }
        let p = p / scale;
        let total = p + weight;
        count += x;
        if total > 0.0 {
            result += binomial_log(x, count, p / total, weight / total);
        } else if x != 0.0 {
            return zero(log);
        }
        weight = total;
    }
    on_scale(result, log)
}

/// Draw a multinomial count vector in input order and write it to `out`.
///
/// Return [`MathError::Dim`] for invalid size, weights, or output length.
pub fn rmultinom(rng: &mut Rng, size: i32, prob: &[f64], out: &mut [i32]) -> Result<(), MathError> {
    if size < 0 || prob.len() != out.len() || !prob.iter().sum::<f64>().is_finite() {
        return Err(MathError::Dim);
    }
    let scale = weight_scale(prob).ok_or(MathError::Dim)?;
    let mut suffix = Vec::new();
    suffix
        .try_reserve_exact(prob.len())
        .map_err(|_| MathError::TooLarge)?;
    suffix.extend(prob.iter().rev().scan(0.0, |total, &weight| {
        let remaining = *total;
        *total += weight / scale;
        Some(remaining)
    }));

    let mut remaining = size;
    for ((&weight, slot), later) in prob.iter().zip(out).zip(suffix.into_iter().rev()) {
        if remaining == 0 || weight == 0.0 {
            *slot = 0;
        } else if later == 0.0 {
            *slot = remaining;
        } else {
            let weight = weight / scale;
            *slot = rbinom(
                rng,
                f64::from(remaining),
                weight / (weight + later),
                &mut Flags::default(),
            ) as i32;
        }
        remaining -= *slot;
    }
    Ok(())
}

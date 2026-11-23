//! Studentized range probabilities and quantiles.

use crate::dist::normal::pnorm;
use crate::special::aux::{lgamma1p, log1mexp};

use super::quadrature::{log_chisq_scale, log_integral};
use super::*;

const INF: f64 = f64::INFINITY;

fn log_normal_interval(center: f64, width: f64, log_width: f64) -> f64 {
    if width < 1e-5 {
        return log_width - LOG_SQRT_2PI - 0.5 * center * center
            + ((center * center - 1.0) * width * width / 24.0).ln_1p();
    }
    let low = 0.5 * width - center.abs();
    let high = -0.5 * width - center.abs();
    let large = pnorm(low, 0.0, 1.0, LOG_TAIL, &mut Flags::default());
    let small = pnorm(high, 0.0, 1.0, LOG_TAIL, &mut Flags::default());
    large + log1mexp(large - small)
}

fn log_range(log_width: f64, nmeans: f64, lower: bool) -> f64 {
    let width = log_width.exp();
    if width.is_infinite() {
        return if lower { 0.0 } else { f64::NEG_INFINITY };
    }
    if nmeans == 2.0 {
        let upper = pnorm(
            -width * std::f64::consts::FRAC_1_SQRT_2,
            0.0,
            1.0,
            LOG_TAIL,
            &mut Flags::default(),
        ) + std::f64::consts::LN_2;
        if !lower {
            return upper;
        }
        if width < 1e-5 {
            return log_width - 0.5 * std::f64::consts::PI.ln() + (-width * width / 12.0).ln_1p();
        }
        return log1mexp(-upper);
    }
    if width >= 32.0 + 8.0 * nmeans.ln().sqrt() {
        // At this separation, intersections of distinct pair events are negligible.
        let upper = pnorm(
            -width * std::f64::consts::FRAC_1_SQRT_2,
            0.0,
            1.0,
            LOG_TAIL,
            &mut Flags::default(),
        ) + nmeans.ln()
            + (nmeans - 1.0).ln();
        return if lower { log1mexp(-upper) } else { upper };
    }
    let power = nmeans - 1.0;
    if width <= 4.0 {
        let center = 0.5 * width;
        let left = -center - 8.0;
        let f = |t: f64| {
            -0.5 * t * t - LOG_SQRT_2PI
                + nmeans.ln()
                + power * log_normal_interval(t + center, width, log_width)
        };
        let lp = log_integral(left, 0.0, 1.0 / nmeans.sqrt().max(1.0), f).min(0.0);
        if lower { lp } else { log1mexp(-lp) }
    } else {
        let f = |t: f64| {
            let min_tail = pnorm(t, 0.0, 1.0, LOG_UPPER, &mut Flags::default());
            let max_tail = pnorm(t + width, 0.0, 1.0, LOG_UPPER, &mut Flags::default());
            let ratio = (max_tail - min_tail).min(0.0);
            let conditional = if ratio < -40.0 {
                power.ln() + ratio
            } else {
                log1mexp(-power * log1mexp(-ratio))
            };
            -0.5 * t * t - LOG_SQRT_2PI + nmeans.ln() + power * min_tail + conditional
        };
        let lp = log_integral(-0.5 * width - (2.0 * nmeans.ln()).sqrt(), 0.0, 1.0, f).min(0.0);
        if lower { log1mexp(-lp) } else { lp }
    }
}

fn tukey_log_tail(log_q: f64, nmeans: f64, df: f64, nranges: f64, lower: bool) -> f64 {
    let range = |log_width| {
        if lower {
            nranges * log_range(log_width, nmeans, true)
        } else {
            let upper = log_range(log_width, nmeans, false);
            if upper < -40.0 && upper + nranges.ln() < -40.0 {
                upper + nranges.ln()
            } else {
                log1mexp(-nranges * log1mexp(-upper))
            }
        }
    };
    if df > 25_000.0 {
        return range(log_q);
    }
    let root_df = df.sqrt();
    let rise = 0.5 * (nranges * (nmeans - 1.0) / df).ln_1p() * root_df;
    let lower_guess = (4.0_f64.ln() - log_q).min(0.0) * root_df;
    let f = |w| log_chisq_scale(w, df) + range(log_q + w / root_df);
    log_integral(lower_guess, rise, 3.0, f).min(0.0)
}

fn valid(nmeans: f64, df: f64, nranges: f64) -> bool {
    df >= 2.0 && nmeans >= 2.0 && nranges >= 1.0 && nmeans.is_finite() && nranges.is_finite()
}

/// Evaluate a studentized range tail. All ranges share the chi-square denominator.
pub fn ptukey(q: f64, nmeans: f64, df: f64, nranges: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if q.is_nan() || nmeans.is_nan() || df.is_nan() || nranges.is_nan() {
        return q + nmeans + df + nranges;
    }
    if q <= 0.0 {
        return step(false, tail);
    }
    if !valid(nmeans, df, nranges) {
        return invalid(flags);
    }
    if q.is_infinite() {
        return step(true, tail);
    }
    on_scale(
        tukey_log_tail(q.ln(), nmeans, df, nranges, tail.lower),
        tail.log,
    )
}

/// Evaluate a studentized range quantile.
pub fn qtukey(p: f64, nmeans: f64, df: f64, nranges: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || nmeans.is_nan() || df.is_nan() || nranges.is_nan() {
        return p + nmeans + df + nranges;
    }
    if !valid(nmeans, df, nranges) || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, 0.0, INF) {
        return x;
    }
    let (lp, lq) = probability_logs(p, tail);
    let lower = lp <= lq;
    let target = lp.min(lq);
    let power = (nmeans - 1.0) * nranges;
    let initial = if lower {
        let a = 0.5 * df;
        let chi_moment = if df.is_infinite() || a > 1e5 {
            0.0
        } else {
            lgamma1p(a + 0.5 * power)
                - lgamma1p(a)
                - (0.5 * power) * a.ln()
                - (0.5 * power / a).ln_1p()
        };
        (lp - 0.5 * nranges * nmeans.ln() - chi_moment) / power + LOG_SQRT_2PI
    } else if df.is_infinite() {
        (2.0 * (-lq).sqrt()).ln()
    } else {
        (2.0 * (-lq).sqrt()).ln().max(-lq / df)
    };
    let evaluate = |x| tukey_log_tail(x, nmeans, df, nranges, lower);
    let initial = initial.clamp(-745.0, 709.0);
    let mut low = initial;
    let mut high = initial;
    let mut stride = 0.5;
    if (evaluate(initial) < target) == lower {
        for _ in 0..16 {
            high = (initial + stride).min(710.0);
            if (evaluate(high) >= target) == lower || high == 710.0 {
                break;
            }
            stride *= 2.0;
        }
    } else {
        for _ in 0..16 {
            low = (initial - stride).max(-746.0);
            if (evaluate(low) <= target) == lower || low == -746.0 {
                break;
            }
            stride *= 2.0;
        }
    }
    let mut prior = (low, evaluate(low));
    let start = high;
    let z = invert(low, high, start, target, lower, |z| {
        let value = evaluate(z);
        let slope = (value - prior.1) / (z - prior.0);
        prior = (z, value);
        (value, slope)
    });
    z.exp()
}

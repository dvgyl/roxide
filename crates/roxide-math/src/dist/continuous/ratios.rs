//! Chi-square, Student's t, and F distributions.

use std::f64::consts::LN_2;

use crate::dist::normal::{dnorm, pnorm, qnorm};
use crate::rng::Rng;
use crate::special::lbeta;

use super::beta::{beta_factor, beta_quantile_odds, logistic_coordinates};
use super::elementary::{dcauchy, pcauchy, qcauchy};
use super::gamma::{dgamma, pgamma, qgamma, rgamma};
use super::*;

const INF: f64 = f64::INFINITY;

/// Evaluate a chi-square density.
pub fn dchisq(x: f64, df: f64, log: bool, flags: &mut Flags) -> f64 {
    dgamma(x, 0.5 * df, 2.0, log, flags)
}

/// Evaluate a chi-square tail probability.
pub fn pchisq(x: f64, df: f64, tail: Tail, flags: &mut Flags) -> f64 {
    pgamma(x, 0.5 * df, 2.0, tail, flags)
}

/// Evaluate a chi-square quantile.
pub fn qchisq(p: f64, df: f64, tail: Tail, flags: &mut Flags) -> f64 {
    qgamma(p, 0.5 * df, 2.0, tail, flags)
}

/// Draw from a chi-square distribution.
pub fn rchisq(rng: &mut Rng, df: f64, flags: &mut Flags) -> f64 {
    if !df.is_finite() || df < 0.0 {
        return invalid(flags);
    }
    rgamma(rng, 0.5 * df, 2.0, flags)
}

fn t_normalization(df: f64, flags: &mut Flags) -> f64 {
    if df >= 1e8 {
        let r = df.recip();
        -LOG_SQRT_2PI - 0.25 * r + (r * r * r) / 24.0
    } else {
        -0.5 * df.ln() - lbeta(0.5 * df, 0.5, flags)
    }
}

/// Evaluate a Student's t density.
pub fn dt(x: f64, df: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || df.is_nan() {
        return x + df;
    }
    if df <= 0.0 {
        return invalid(flags);
    }
    if df.is_infinite() {
        return dnorm(x, 0.0, 1.0, log, flags);
    }
    if x.is_infinite() {
        return zero(log);
    }
    if df == 1.0 {
        return dcauchy(x, 0.0, 1.0, log, flags);
    }
    let ratio = (x / df.sqrt()).powi(2);
    let power = if ratio < f64::MIN_POSITIVE {
        0.5 * x * x * (1.0 + df.recip())
    } else if ratio.is_infinite() {
        (0.5 * df + 0.5) * (2.0 * x.abs().ln() - df.ln())
    } else {
        (0.5 * df + 0.5) * ratio.ln_1p()
    };
    on_scale(t_normalization(df, flags) - power, log)
}

/// Evaluate a Student's t tail probability.
pub fn pt(x: f64, df: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || df.is_nan() {
        return x + df;
    }
    if df <= 0.0 {
        return invalid(flags);
    }
    if df.is_infinite() {
        return pnorm(x, 0.0, 1.0, tail, flags);
    }
    if x.is_infinite() {
        return step(x > 0.0, tail);
    }
    if df == 1.0 {
        return pcauchy(x, 0.0, 1.0, tail, flags);
    }
    if x.abs() < 1e-5 && x * x < 1e-10 * df {
        let offset =
            x * t_normalization(df, flags).exp() * (1.0 - (1.0 + df.recip()) * x * x / 6.0);
        let p = if tail.lower {
            0.5 + offset
        } else {
            0.5 - offset
        };
        return if tail.log { p.ln() } else { p };
    }
    let odds = df.ln() - 2.0 * x.abs().ln();
    let (z, y) = logistic_coordinates(odds);
    let lp = if z == 0.0 {
        0.5 * df * odds - (0.5 * df).ln() - lbeta(0.5 * df, 0.5, flags)
    } else {
        super::pbeta_raw(z, y, 0.5 * df, 0.5, LOG_TAIL, flags)
    };
    from_log(lp - LN_2, x < 0.0, tail)
}

/// Evaluate a Student's t quantile.
pub fn qt(p: f64, df: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || df.is_nan() {
        return p + df;
    }
    if !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, -INF, INF) {
        return x;
    }
    if df <= 0.0 {
        return invalid(flags);
    }
    if df.is_infinite() {
        return qnorm(p, 0.0, 1.0, tail, flags);
    }
    if df == 1.0 {
        return qcauchy(p, 0.0, 1.0, tail, flags);
    }
    let (lp, lq) = probability_logs(p, tail);
    let lower = lp < lq;
    let small = lp.min(lq);
    let doubled = (small + LN_2).min(0.0);
    let center = if tail.log {
        0.5 * (p + LN_2).exp_m1()
    } else {
        p - 0.5
    };
    if center.abs() < 1e-7 {
        let x = center / t_normalization(df, flags).exp();
        if x * x < 1e-10 * df {
            return if tail.lower { x } else { -x };
        }
    }
    let magnitude = if df == 2.0 {
        (-doubled.exp_m1()) * (-0.5 * (doubled + log1mexp(-small))).exp()
    } else {
        let odds = beta_quantile_odds(doubled, log1mexp(-doubled), 0.5 * df, 0.5, flags);
        (0.5 * (df.ln() - odds)).exp()
    };
    if lower { -magnitude } else { magnitude }
}

/// Draw from a Student's t distribution.
pub fn rt(rng: &mut Rng, df: f64, flags: &mut Flags) -> f64 {
    if df.is_nan() || df <= 0.0 {
        return invalid(flags);
    }
    let normal = rng.norm_rand();
    if df.is_infinite() {
        return normal;
    }
    normal / (rchisq(rng, df, flags) / df).sqrt()
}

pub(super) fn f_coordinates(x: f64, df1: f64, df2: f64) -> (f64, f64) {
    let numerator = x * df1;
    let ratio = numerator / df2;
    if ratio.is_normal() {
        let divisor = 1.0 + ratio;
        (ratio / divisor, divisor.recip())
    } else {
        logistic_coordinates(x.ln() + df1.ln() - df2.ln())
    }
}

fn f_density_at_zero(df1: f64, log: bool) -> f64 {
    if df1 < 2.0 {
        INF
    } else if df1 == 2.0 {
        on_scale(0.0, log)
    } else {
        zero(log)
    }
}

/// Evaluate an F density with numerator and denominator degrees of freedom.
pub fn df(x: f64, df1: f64, df2: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || df1.is_nan() || df2.is_nan() {
        return x + df1 + df2;
    }
    if df1 <= 0.0 || df2 <= 0.0 {
        return invalid(flags);
    }
    if x < 0.0 || x.is_infinite() {
        return zero(log);
    }
    if x == 0.0 {
        return f_density_at_zero(df1, log);
    }
    if df2.is_infinite() {
        if df1.is_infinite() {
            return if x == 1.0 { INF } else { zero(log) };
        }
        return dgamma(x, 0.5 * df1, 2.0 / df1, log, flags);
    }
    if df1.is_infinite() {
        return on_scale(
            dgamma(x.recip(), 0.5 * df2, 2.0 / df2, true, flags) - 2.0 * x.ln(),
            log,
        );
    }
    let (z, y) = f_coordinates(x, df1, df2);
    on_scale(beta_factor(z, y, 0.5 * df1, 0.5 * df2, flags) - x.ln(), log)
}

/// Evaluate an F tail probability.
pub fn pf(x: f64, df1: f64, df2: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || df1.is_nan() || df2.is_nan() {
        return x + df1 + df2;
    }
    if df1 <= 0.0 || df2 <= 0.0 {
        return invalid(flags);
    }
    if x <= 0.0 || x.is_infinite() {
        return step(x > 0.0, tail);
    }
    if df2.is_infinite() {
        if df1.is_infinite() {
            return if x == 1.0 {
                on_scale(-LN_2, tail.log)
            } else {
                step(x > 1.0, tail)
            };
        }
        return pgamma(x, 0.5 * df1, 2.0 / df1, tail, flags);
    }
    if df1.is_infinite() {
        return pgamma(
            x.recip(),
            0.5 * df2,
            2.0 / df2,
            Tail {
                lower: !tail.lower,
                ..tail
            },
            flags,
        );
    }
    let (z, y) = f_coordinates(x, df1, df2);
    super::pbeta_raw(z, y, 0.5 * df1, 0.5 * df2, tail, flags)
}

/// Evaluate an F quantile.
pub fn qf(p: f64, df1: f64, df2: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || df1.is_nan() || df2.is_nan() {
        return p + df1 + df2;
    }
    if df1 <= 0.0 || df2 <= 0.0 || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, 0.0, INF) {
        return x;
    }
    if df2.is_infinite() {
        if df1.is_infinite() {
            return 1.0;
        }
        return qgamma(p, 0.5 * df1, 2.0 / df1, tail, flags);
    }
    if df1.is_infinite() {
        return qgamma(
            p,
            0.5 * df2,
            2.0 / df2,
            Tail {
                lower: !tail.lower,
                ..tail
            },
            flags,
        )
        .recip();
    }
    let (lp, lq) = probability_logs(p, tail);
    let odds = beta_quantile_odds(lp, lq, 0.5 * df1, 0.5 * df2, flags);
    (odds + (df2 / df1).ln()).exp()
}

/// Draw from an F distribution.
pub fn rf(rng: &mut Rng, df1: f64, df2: f64, flags: &mut Flags) -> f64 {
    if df1.is_nan() || df2.is_nan() || df1 <= 0.0 || df2 <= 0.0 {
        return invalid(flags);
    }
    let scaled = |rng: &mut Rng, df, flags: &mut Flags| {
        if df == INF {
            1.0
        } else {
            rchisq(rng, df, flags) / df
        }
    };
    scaled(rng, df1, flags) / scaled(rng, df2, flags)
}

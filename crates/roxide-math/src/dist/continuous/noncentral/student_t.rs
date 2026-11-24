use crate::dist::normal::{dnorm, pnorm, qnorm};
use crate::rng::Rng;

use super::super::beta::{beta_factor, logistic_coordinates};
use super::super::quadrature::{centered_integral, log_chisq_scale, log_integral};
use super::super::ratios::{dt, pt, qt, rchisq};
use super::super::{
    Flags, Tail, endpoints, from_log, invalid, invert, on_scale, pbeta_raw, probability_logs,
    probability_valid, step, zero,
};
use super::INF;

// Expand the normal integral in beta ratios. The odd and even terms
// come from the two parts of the noncentral normal density.
fn t_series(x: f64, df: f64, ncp: f64, flags: &mut Flags) -> Option<f64> {
    let mean = 0.5 * ncp * ncp;
    if mean > 100.0 || df > 400_000.0 || x == 0.0 {
        return None;
    }
    let delta = if x < 0.0 { -ncp } else { ncp };
    let square = x * x;
    let (z, y) = if square.is_normal() && (df + square).is_finite() {
        (square / (df + square), df / (df + square))
    } else {
        logistic_coordinates(2.0 * x.abs().ln() - df.ln())
    };
    let b = 0.5 * df;
    let mut p = 0.5 * (-mean).exp();
    let mut q = (2.0 / std::f64::consts::PI).sqrt() * p * delta;
    let mut remainder = 0.5 - p;
    let mut odd = pbeta_raw(z, y, 0.5, b, Tail::default(), flags);
    let log_y = if y > 0.5 { (-z).ln_1p() } else { y.ln() };
    let power = (b * log_y).exp();
    let mut even = -(b * log_y).exp_m1();
    let mut odd_delta = (beta_factor(z, y, 0.5, b, flags) + std::f64::consts::LN_2).exp();
    let mut even_delta = b * z * power;
    let mut sum = p.mul_add(odd, q * even);
    for j in 1..=1000 {
        let n = f64::from(j);
        odd -= odd_delta;
        even -= even_delta;
        odd_delta *= z * ((n - 0.5 + b) / (n + 0.5));
        even_delta *= z * ((n + b) / (n + 1.0));
        p *= mean / n;
        q *= mean / (n + 0.5);
        remainder -= p;
        sum += p.mul_add(odd, q * even);
        let error = 2.0 * remainder * (odd - odd_delta);
        if error <= 1e-12 {
            break;
        }
        if j == 1000 {
            flags.set(Flags::NO_CONVERGENCE);
        }
    }
    let base = pnorm(-delta, 0.0, 1.0, Tail::default(), flags);
    let cdf = (sum + base).min(1.0);
    if !cdf.is_finite() || cdf < 1e-290 || cdf == 1.0 {
        return None;
    }
    Some(cdf)
}

fn t_mixture(x: f64, df: f64, ncp: f64, lower: bool) -> f64 {
    let root_df = df.sqrt();
    let log_center = (root_df.max(ncp.abs()).max(1.0).ln() - x.abs().ln()).min(0.0);
    let shift = root_df * log_center;
    let range = 4.0_f64.max(root_df.recip());
    let f = |w: f64| {
        let h = w / root_df;
        let z = if h.abs() < 0.5 {
            x.mul_add(h.exp_m1(), x - ncp)
        } else {
            (x.abs().ln() + h).exp().copysign(x) - ncp
        };
        let normal = pnorm(
            z,
            0.0,
            1.0,
            Tail { lower, log: true },
            &mut Flags::default(),
        );
        log_chisq_scale(w, df) + normal
    };
    log_integral(shift, 0.0, range, f)
}

// Locate the mode in log(s), where df * s^2 has a chi-square distribution.
fn t_density(x: f64, df: f64, ncp: f64) -> f64 {
    let root_df = df.sqrt();
    let h = root_df.hypot(x);
    let r = x / h;
    let b = r * ncp;
    let root = (0.5 * b).hypot((df + 1.0).sqrt());
    let m = if b >= 0.0 {
        root + 0.5 * b
    } else {
        (df + 1.0) / (root - 0.5 * b)
    };
    let offset = (m - h) / h;
    let center = if offset.abs() < 0.5 {
        offset.ln_1p()
    } else {
        m.ln() - h.ln()
    };
    let z = if b > 0.0 {
        r * ((df + 1.0) / m) - ((root_df / h) * ncp) * (root_df / h)
    } else {
        r.mul_add(m, -ncp)
    };
    let width = (df + 1.0).sqrt().hypot(m).recip();
    let peak = log_chisq_scale(center * root_df, df)
        + center
        + dnorm(z, 0.0, 1.0, true, &mut Flags::default());
    let log_relative = |u: f64| {
        let v = u * width;
        let e = v.exp_m1();
        let remainder = if v.abs() < 0.25 {
            let series = (2..=19)
                .rev()
                .fold(0.0, |sum, k| (v * sum + 1.0) / f64::from(k));
            v * v * series
        } else {
            e - v
        };
        -(df + 1.0) * remainder - 0.5 * (m * e).powi(2)
    };
    peak + (root_df * width).ln() + centered_integral(log_relative)
}

/// Evaluate the noncentral Student's t density.
pub fn dnt(x: f64, df: f64, ncp: f64, log: bool, flags: &mut Flags) -> f64 {
    if x.is_nan() || df.is_nan() || ncp.is_nan() {
        return x + df + ncp;
    }
    if df <= 0.0 {
        return invalid(flags);
    }
    if ncp.is_infinite() {
        return zero(log);
    }
    if df.is_infinite() {
        return dnorm(x, ncp, 1.0, log, flags);
    }
    if ncp == 0.0 {
        return dt(x, df, log, flags);
    }
    if x.is_infinite() {
        return zero(log);
    }
    if x.abs() > 1e-4 {
        let a = t_series(x, df, ncp, flags);
        let b = t_series(x * ((df + 2.0) / df).sqrt(), df + 2.0, ncp, flags);
        if let (Some(a), Some(b)) = (a, b) {
            let density = ((b - a) / x).abs() * df;
            if density > 0.0 {
                return if log { density.ln() } else { density };
            }
        }
    }
    on_scale(t_density(x, df, ncp), log)
}

/// Evaluate a noncentral Student's t tail probability.
pub fn pnt(x: f64, df: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if x.is_nan() || df.is_nan() || ncp.is_nan() {
        return x + df + ncp;
    }
    if df <= 0.0 {
        return invalid(flags);
    }
    if x.is_infinite() || ncp.is_infinite() {
        return step(x == INF || (x != -INF && ncp == -INF), tail);
    }
    if df.is_infinite() || x == 0.0 {
        return pnorm(x, ncp, 1.0, tail, flags);
    }
    if ncp == 0.0 {
        return pt(x, df, tail, flags);
    }
    if let Some(cdf) = t_series(x, df, ncp, flags) {
        let lower = x >= 0.0;
        if cdf > 1.0 - 1e-10 {
            flags.set(Flags::PRECISION);
        }
        return if lower == tail.lower {
            if tail.log { cdf.ln() } else { cdf }
        } else if tail.log {
            (-cdf).ln_1p()
        } else {
            1.0 - cdf
        };
    }
    let lower = if x >= ncp {
        false
    } else {
        tail.lower || x < 0.0
    };
    from_log(t_mixture(x, df, ncp, lower).min(0.0), lower, tail)
}

/// Evaluate a noncentral Student's t quantile.
pub fn qnt(p: f64, df: f64, ncp: f64, tail: Tail, flags: &mut Flags) -> f64 {
    if p.is_nan() || df.is_nan() || ncp.is_nan() {
        return p + df + ncp;
    }
    if df <= 0.0 || !ncp.is_finite() || !probability_valid(p, tail) {
        return invalid(flags);
    }
    if let Some(x) = endpoints(p, tail, -INF, INF) {
        return x;
    }
    if df.is_infinite() {
        return qnorm(p, ncp, 1.0, tail, flags);
    }
    if ncp == 0.0 {
        return qt(p, df, tail, flags);
    }
    let (lp, lq) = probability_logs(p, tail);
    let lower = lp <= lq;
    let log_tail = Tail { lower, log: true };
    let target = lp.min(lq);
    let seed = ncp + qt(p, df, tail, flags);
    let convert = |z: f64| z.sinh();
    let z = invert(
        -710.0,
        710.0,
        seed.asinh().clamp(-709.0, 709.0),
        target,
        lower,
        |z| {
            let x = convert(z);
            let (lp, ld) = if target < -26.0 {
                (t_mixture(x, df, ncp, lower), t_density(x, df, ncp))
            } else {
                (
                    pnt(x, df, ncp, log_tail, flags),
                    dnt(x, df, ncp, true, flags),
                )
            };
            let slope = (ld - lp + z.cosh().ln()).exp();
            (lp, if lower { slope } else { -slope })
        },
    );
    convert(z)
}

/// Draw from a noncentral Student's t distribution.
pub fn rnt(rng: &mut Rng, df: f64, ncp: f64, flags: &mut Flags) -> f64 {
    if df <= 0.0 || df.is_nan() || !ncp.is_finite() {
        return invalid(flags);
    }
    let normal = rng.norm_rand() + ncp;
    if df.is_infinite() {
        return normal;
    }
    normal / (rchisq(rng, df, flags) / df).sqrt()
}

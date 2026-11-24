//! Scalar C adapters for the Roxide math library.

use std::ffi::c_int;

use roxide_eval::{Ctx, EvalResult};
use roxide_math::dist::{continuous, discrete, normal};
use roxide_math::special::{self, aux};
use roxide_math::{Flags, Tail};
use roxide_value::{NA_INTEGER, NA_REAL, nan_propagate};

use super::{random::draw, vector::api};

macro_rules! scalar {
    ($(fn $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty = $result:expr;)+) => {$(
        /// Evaluate a scalar Rmath function.
        #[allow(non_snake_case)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name($($arg: $ty),*) -> $ret {
            $result
        }
    )+};
}

macro_rules! distributions {
    ($($module:ident::$d:ident, $p:ident, $q:ident => $cd:ident, $cp:ident, $cq:ident ($($param:ident),+);)+) => {
        scalar! {$(
            fn $cd(x: f64, $($param: f64,)+ log: c_int) -> f64 =
                $module::$d(x, $($param,)+ log != 0, &mut Flags::default());
            fn $cp(x: f64, $($param: f64,)+ lower: c_int, log: c_int) -> f64 =
                $module::$p(x, $($param,)+ tail(lower, log), &mut Flags::default());
            fn $cq(x: f64, $($param: f64,)+ lower: c_int, log: c_int) -> f64 =
                $module::$q(x, $($param,)+ tail(lower, log), &mut Flags::default());
        )+}
    };
}

fn tail(lower: c_int, log: c_int) -> Tail {
    Tail {
        lower: lower != 0,
        log: log != 0,
    }
}

distributions! {
    normal::dnorm, pnorm, qnorm => Rf_dnorm4, Rf_pnorm5, Rf_qnorm5(mean, sd);
    continuous::dunif, punif, qunif => Rf_dunif, Rf_punif, Rf_qunif(min, max);
    continuous::dgamma, pgamma, qgamma => Rf_dgamma, Rf_pgamma, Rf_qgamma(shape, scale);
    continuous::dbeta, pbeta, qbeta => Rf_dbeta, Rf_pbeta, Rf_qbeta(a, b);
    continuous::dt, pt, qt => Rf_dt, Rf_pt, Rf_qt(df);
    continuous::df, pf, qf => Rf_df, Rf_pf, Rf_qf(df1, df2);
    continuous::dchisq, pchisq, qchisq => Rf_dchisq, Rf_pchisq, Rf_qchisq(df);
    continuous::dweibull, pweibull, qweibull => Rf_dweibull, Rf_pweibull, Rf_qweibull(shape, scale);
    continuous::dlnorm, plnorm, qlnorm => Rf_dlnorm, Rf_plnorm, Rf_qlnorm(meanlog, sdlog);
    continuous::dlogis, plogis, qlogis => Rf_dlogis, Rf_plogis, Rf_qlogis(location, scale);
    continuous::dcauchy, pcauchy, qcauchy => Rf_dcauchy, Rf_pcauchy, Rf_qcauchy(location, scale);
    discrete::dbinom, pbinom, qbinom => Rf_dbinom, Rf_pbinom, Rf_qbinom(size, prob);
    discrete::dpois, ppois, qpois => Rf_dpois, Rf_ppois, Rf_qpois(lambda);
    discrete::dgeom, pgeom, qgeom => Rf_dgeom, Rf_pgeom, Rf_qgeom(prob);
    discrete::dhyper, phyper, qhyper => Rf_dhyper, Rf_phyper, Rf_qhyper(m, n, k);
    discrete::dnbinom, pnbinom, qnbinom => Rf_dnbinom, Rf_pnbinom, Rf_qnbinom(size, prob);
    discrete::dnbinom_mu, pnbinom_mu, qnbinom_mu => Rf_dnbinom_mu, Rf_pnbinom_mu, Rf_qnbinom_mu(size, mu);
    discrete::dwilcox, pwilcox, qwilcox => Rf_dwilcox, Rf_pwilcox, Rf_qwilcox(m, n);
    discrete::dsignrank, psignrank, qsignrank => Rf_dsignrank, Rf_psignrank, Rf_qsignrank(n);
    continuous::dnt, pnt, qnt => Rf_dnt, Rf_pnt, Rf_qnt(df, ncp);
    continuous::dnf, pnf, qnf => Rf_dnf, Rf_pnf, Rf_qnf(df1, df2, ncp);
    continuous::dnchisq, pnchisq, qnchisq => Rf_dnchisq, Rf_pnchisq, Rf_qnchisq(df, ncp);
    continuous::dnbeta, pnbeta, qnbeta => Rf_dnbeta, Rf_pnbeta, Rf_qnbeta(a, b, ncp);
}

macro_rules! draws {
    ($($name:ident($($arg:ident),+) = $kernel:path, ($($value:expr),+);)+) => {
        api! {$(
            /// Draw a variate from the active random stream.
            fn $name($($arg: f64),+) -> f64 = f64::NAN; |ctx| {
                draw(ctx, |rng| $kernel(rng, $($value,)+ &mut Flags::default()))
            }
        )+}
    };
}

draws! {
    Rf_rnorm(mean, sd) = normal::rnorm, (mean, sd);
    Rf_runif(min, max) = continuous::runif, (min, max);
    Rf_rgamma(shape, scale) = continuous::rgamma, (shape, scale);
    Rf_rbeta(a, b) = continuous::rbeta, (a, b);
    Rf_rt(df) = continuous::rt, (df);
    Rf_rf(df1, df2) = continuous::rf, (df1, df2);
    Rf_rchisq(df) = continuous::rchisq, (df);
    Rf_rweibull(shape, scale) = continuous::rweibull, (shape, scale);
    Rf_rlnorm(meanlog, sdlog) = continuous::rlnorm, (meanlog, sdlog);
    Rf_rlogis(location, scale) = continuous::rlogis, (location, scale);
    Rf_rcauchy(location, scale) = continuous::rcauchy, (location, scale);
    Rf_rbinom(size, prob) = discrete::rbinom, (size, prob);
    Rf_rpois(lambda) = discrete::rpois, (lambda);
    Rf_rgeom(prob) = discrete::rgeom, (prob);
    Rf_rhyper(m, n, k) = discrete::rhyper, (m, n, k);
    Rf_rnbinom(size, prob) = discrete::rnbinom, (size, prob);
    Rf_rnbinom_mu(size, mu) = discrete::rnbinom_mu, (size, mu);
    Rf_rwilcox(m, n) = discrete::rwilcox, (m, n);
    Rf_rsignrank(n) = discrete::rsignrank, (n);
    Rf_rnchisq(df, ncp) = continuous::rnchisq, (df, ncp);
}

scalar! {
    fn Rf_ptukey(x: f64, ranges: f64, means: f64, df: f64, lower: c_int, log: c_int) -> f64 =
        continuous::ptukey(x, means, df, ranges, tail(lower, log), &mut Flags::default());
    fn Rf_qtukey(p: f64, ranges: f64, means: f64, df: f64, lower: c_int, log: c_int) -> f64 =
        continuous::qtukey(p, means, df, ranges, tail(lower, log), &mut Flags::default());

    fn Rf_gammafn(x: f64) -> f64 = special::gamma(x, &mut Flags::default());
    fn Rf_lgammafn(x: f64) -> f64 = special::lgamma(x, &mut Flags::default());
    fn Rf_digamma(x: f64) -> f64 = special::digamma(x, &mut Flags::default());
    fn Rf_trigamma(x: f64) -> f64 = special::trigamma(x, &mut Flags::default());
    fn Rf_tetragamma(x: f64) -> f64 = special::psigamma(x, 2, &mut Flags::default());
    fn Rf_pentagamma(x: f64) -> f64 = special::psigamma(x, 3, &mut Flags::default());
    fn Rf_psigamma(x: f64, derivative: f64) -> f64 =
        if derivative.is_finite() {
            special::psigamma(x, derivative.round_ties_even() as c_int, &mut Flags::default())
        } else {
            if x.is_nan() || derivative.is_nan() { nan_propagate(x, derivative) } else { f64::NAN }
        };
    fn Rf_beta(a: f64, b: f64) -> f64 = special::beta(a, b, &mut Flags::default());
    fn Rf_lbeta(a: f64, b: f64) -> f64 = special::lbeta(a, b, &mut Flags::default());
    fn Rf_choose(n: f64, k: f64) -> f64 = special::choose(n, k, &mut Flags::default());
    fn Rf_lchoose(n: f64, k: f64) -> f64 = special::lchoose(n, k, &mut Flags::default());
    fn Rf_bessel_i(x: f64, nu: f64, expo: f64) -> f64 =
        special::bessel_i(x, nu, expo == 2.0, &mut Flags::default());
    fn Rf_bessel_j(x: f64, nu: f64) -> f64 = special::bessel_j(x, nu, &mut Flags::default());
    fn Rf_bessel_k(x: f64, nu: f64, expo: f64) -> f64 =
        special::bessel_k(x, nu, expo == 2.0, &mut Flags::default());
    fn Rf_bessel_y(x: f64, nu: f64) -> f64 = special::bessel_y(x, nu, &mut Flags::default());
    fn Rf_fmax2(x: f64, y: f64) -> f64 = if x.is_nan() || y.is_nan() { nan_propagate(x, y) } else { x.max(y) };
    fn Rf_fmin2(x: f64, y: f64) -> f64 = if x.is_nan() || y.is_nan() { nan_propagate(x, y) } else { x.min(y) };
    fn Rf_fprec(x: f64, digits: f64) -> f64 = precision(x, digits);
    fn Rf_fround(x: f64, digits: f64) -> f64 = round(x, digits);
    fn Rf_fsign(x: f64, y: f64) -> f64 =
        if x.is_nan() || y.is_nan() { nan_propagate(x, y) } else if y < 0.0 { -x.abs() } else { x.abs() };
    fn Rf_ftrunc(x: f64) -> f64 = x.trunc();
    fn Rf_imax2(x: c_int, y: c_int) -> c_int = if x == NA_INTEGER || y == NA_INTEGER { NA_INTEGER } else { x.max(y) };
    fn Rf_imin2(x: c_int, y: c_int) -> c_int = x.min(y);
    fn Rf_sign(x: f64) -> f64 = if x == 0.0 { 0.0 } else if x.is_nan() { x } else { x.signum() };
    fn Rf_log1pmx(x: f64) -> f64 = aux::log1pmx(x);
    fn Rf_lgamma1p(x: f64) -> f64 = aux::lgamma1p(x);
    fn Rf_log1mexp(x: f64) -> f64 = aux::log1mexp(x);
    fn Rf_log1pexp(x: f64) -> f64 = aux::log1pexp(x);
    fn Rf_logspace_add(x: f64, y: f64) -> f64 = log_add(x, y);
    fn Rf_logspace_sub(x: f64, y: f64) -> f64 = x + aux::log1mexp(x - y);
    fn Rf_dpois_raw(x: f64, lambda: f64, log: c_int) -> f64 =
        density_scale(poisson_log(x, lambda), log);
    fn Rf_dbinom_raw(x: f64, n: f64, p: f64, q: f64, log: c_int) -> f64 =
        density_scale(binomial_log(x, n, p, q), log);

    fn Rf_dexp(x: f64, scale: f64, log: c_int) -> f64 =
        if x.is_nan() || scale.is_nan() { nan_propagate(x, scale) }
        else if scale <= 0.0 { f64::NAN }
        else {
            let density = continuous::dexp(x / scale, 1.0, log != 0, &mut Flags::default());
            if log != 0 { density - scale.ln() } else { density / scale }
        };
    fn Rf_pexp(x: f64, scale: f64, lower: c_int, log: c_int) -> f64 =
        if x.is_nan() || scale.is_nan() { nan_propagate(x, scale) }
        else if scale < 0.0 { f64::NAN }
        else {
            let unit = if x == 0.0 { 0.0 } else { x / scale.abs() };
            continuous::pexp(unit, 1.0, tail(lower, log), &mut Flags::default())
        };
    fn Rf_qexp(p: f64, scale: f64, lower: c_int, log: c_int) -> f64 =
        if p.is_nan() || scale.is_nan() { nan_propagate(p, scale) }
        else if scale < 0.0 { f64::NAN }
        else {
            let unit = continuous::qexp(p, 1.0, tail(lower, log), &mut Flags::default());
            if unit == 0.0 { unit } else { unit * scale }
        };
    fn cospi(x: f64) -> f64 = aux::cospi(x);
    fn sinpi(x: f64) -> f64 = aux::sinpi(x);
    fn tanpi(x: f64) -> f64 = aux::tanpi(x);
    fn Rtanpi(x: f64) -> f64 = aux::tanpi(x);
    fn R_pow(x: f64, y: f64) -> f64 = power(x, y);
    fn R_pow_di(x: f64, n: c_int) -> f64 = if x.is_nan() { x } else if n == NA_INTEGER { NA_REAL } else if x.is_infinite() { power(x, f64::from(n)) } else { x.powi(n) };
    fn Rf_pow1p(x: f64, y: f64) -> f64 = power_one_plus(x, y);
    fn pow1p(x: f64, y: f64) -> f64 = power_one_plus(x, y);
}

fn power_one_plus(x: f64, y: f64) -> f64 {
    if x == 0.0 || y == 0.0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() {
        return nan_propagate(x, y);
    }
    let base = 1.0 + x;
    if y == 1.0 {
        base
    } else if x <= -1.0 || base - 1.0 == x {
        base.powf(y)
    } else {
        (y * x.ln_1p()).exp()
    }
}

fn power(x: f64, y: f64) -> f64 {
    if x == 1.0 || y == 0.0 {
        1.0
    } else if x.is_nan() || y.is_nan() {
        nan_propagate(x, y)
    } else if x == 0.0 {
        if y > 0.0 { 0.0 } else { f64::INFINITY }
    } else if x < 0.0 && (!y.is_finite() || y.fract() != 0.0) {
        f64::NAN
    } else {
        x.powf(y)
    }
}

fn log_add(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return nan_propagate(x, y);
    }
    let hi = x.max(y);
    if hi.is_infinite() {
        hi
    } else {
        hi + (x.min(y) - hi).exp().ln_1p()
    }
}

fn precision(x: f64, digits: f64) -> f64 {
    if digits.is_nan() {
        return nan_propagate(x, digits);
    }
    if x == 0.0 || !x.is_finite() || digits > 22.0 {
        return x;
    }
    let places = digits.round().max(1.0) - 1.0 - x.abs().log10().floor();
    if places > 308.0 {
        round(x * 1e308, places - 308.0) / 1e308
    } else {
        round(x, places)
    }
}

fn round(x: f64, digits: f64) -> f64 {
    if digits.is_nan() {
        return nan_propagate(x, digits);
    }
    if !x.is_finite() || digits > 308.0 {
        return x;
    }
    if digits < -308.0 {
        return 0.0_f64.copysign(x);
    }
    let digits = digits.round() as i32;
    let scale = 10_f64.powi(digits.abs());
    let (scaled, residual) = if digits >= 0 {
        let scaled = x * scale;
        (scaled, x.mul_add(scale, -scaled))
    } else {
        let scaled = x / scale;
        (scaled, (-scaled).mul_add(scale, x))
    };
    if !scaled.is_finite() {
        return x;
    }
    // A rounded product can appear to be a tie. The residual gives its direction.
    let rounded = if scaled.fract().abs() == 0.5 && residual != 0.0 {
        if residual.is_sign_positive() {
            scaled.ceil()
        } else {
            scaled.floor()
        }
    } else {
        scaled.round_ties_even()
    };
    if digits >= 0 {
        rounded / scale
    } else {
        rounded * scale
    }
}

/// Return the logarithm of the absolute gamma value and optionally write its sign.
///
/// # Safety
/// A non-null sign pointer must be writable.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_lgammafn_sign(x: f64, sign: *mut c_int) -> f64 {
    let (value, direction) = special::lgamma_sign(x, &mut Flags::default());
    if !sign.is_null() {
        // SAFETY: The caller supplies writable storage.
        unsafe { sign.write(direction) };
    }
    value
}

/// Sum probabilities represented on the logarithmic scale.
///
/// # Safety
/// For a positive count, the input must point to that many readable doubles.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_logspace_sum(logx: *const f64, count: c_int) -> f64 {
    if count <= 0 {
        return f64::NEG_INFINITY;
    }
    if logx.is_null() {
        return f64::NAN;
    }
    // SAFETY: The caller supplies the readable range. A C int count fits in isize.
    let values = unsafe { std::slice::from_raw_parts(logx, count as usize) };
    values.iter().copied().fold(f64::NEG_INFINITY, log_add)
}

/// Write the requested standard normal tail probabilities.
///
/// # Safety
/// Provide writable storage for each requested result.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_pnorm_both(
    x: f64,
    cum: *mut f64,
    ccum: *mut f64,
    tail: c_int,
    log: c_int,
) {
    // SAFETY: The caller supplies writable pointers for the requested tails.
    unsafe {
        if tail != 1 && !cum.is_null() {
            cum.write(Rf_pnorm5(x, 0.0, 1.0, 1, log));
        }
        if tail != 0 && !ccum.is_null() {
            ccum.write(Rf_pnorm5(x, 0.0, 1.0, 0, log));
        }
    }
}

unsafe fn multinomial(
    ctx: &mut Ctx,
    n: c_int,
    probabilities: *mut f64,
    count: c_int,
    out: *mut c_int,
) -> EvalResult<()> {
    if count < 0 || probabilities.is_null() || out.is_null() {
        return Err(ctx.stop(roxide_value::SEXP_NIL, "invalid multinomial buffers"));
    }
    if count == 0 {
        return Ok(());
    }
    let count = count as usize;
    super::vector::check_capacity(ctx, count, size_of::<f64>())?;
    let mut counts = Vec::new();
    counts
        .try_reserve_exact(count)
        .map_err(|_| ctx.stop(roxide_value::SEXP_NIL, "cannot allocate multinomial counts"))?;
    counts.resize(count, 0);
    // SAFETY: C supplies count initialized probabilities.
    let probabilities = unsafe { std::slice::from_raw_parts(probabilities, count) };
    draw(ctx, |rng| {
        discrete::rmultinom(rng, n, probabilities, &mut counts)
    })?
    .map_err(|err| ctx.stop(roxide_value::SEXP_NIL, err.to_string()))?;
    // SAFETY: C supplies storage for count elements. The source is a separate allocation.
    unsafe { std::ptr::copy_nonoverlapping(counts.as_ptr(), out, count) };
    Ok(())
}

api! {
    /// Draw an exponential variate with the specified scale.
    fn Rf_rexp(scale: f64) -> f64 = f64::NAN; |ctx| {
        if scale == 0.0 {
            return Ok(0.0);
        }
        if scale < 0.0 || !scale.is_finite() {
            return Ok(f64::NAN);
        }
        draw(ctx, |rng| scale * rng.exp_rand())
    }

    /// Write multinomial counts to the output array.
    fn Rf_rmultinom(n: c_int, probabilities: *mut f64, count: c_int, out: *mut c_int) -> () = (); |ctx| {
        multinomial(ctx, n, probabilities, count, out)
    }
}

fn density_scale(log_value: f64, log: c_int) -> f64 {
    if log != 0 { log_value } else { log_value.exp() }
}

fn poisson_log(x: f64, lambda: f64) -> f64 {
    if x.is_nan() || lambda.is_nan() {
        return nan_propagate(x, lambda);
    }
    if x < 0.0 || !x.is_finite() || !lambda.is_finite() {
        return f64::NEG_INFINITY;
    }
    if x == 0.0 {
        return -lambda;
    }
    if lambda <= 0.0 {
        return f64::NEG_INFINITY;
    }
    let (deviance, residual) = aux::ebd0(x, lambda);
    -deviance - (residual + aux::stirlerr(x) + 0.918_938_533_204_672_8 + 0.5 * x.ln())
}

fn binomial_log(x: f64, n: f64, p: f64, q: f64) -> f64 {
    if x.is_nan() || n.is_nan() || p.is_nan() || q.is_nan() {
        return nan_propagate(nan_propagate(x, n), nan_propagate(p, q));
    }
    if p == 0.0 {
        return if x == 0.0 { 0.0 } else { f64::NEG_INFINITY };
    }
    if q == 0.0 {
        return if x == n { 0.0 } else { f64::NEG_INFINITY };
    }
    if x < 0.0 || x > n {
        return f64::NEG_INFINITY;
    }
    if x == 0.0 {
        return n * if p < 0.5 { (-p).ln_1p() } else { q.ln() };
    }
    if x == n {
        return n * if q < 0.5 { (-q).ln_1p() } else { p.ln() };
    }
    let y = n - x;
    let small = x.min(y);
    let (dx, ex) = aux::ebd0(x, n * p);
    let (dy, ey) = aux::ebd0(y, n * q);
    let correction = aux::stirlerr(n)
        - aux::stirlerr(x)
        - aux::stirlerr(y)
        - ex
        - ey
        - 0.918_938_533_204_672_8
        - 0.5 * (small.ln() + (-small / n).ln_1p());
    (correction - dx) - dy
}

scalar! {
    fn Rf_bessel_i_ex(x: f64, nu: f64, expo: f64, _work: *mut f64) -> f64 = Rf_bessel_i(x, nu, expo);
    fn Rf_bessel_j_ex(x: f64, nu: f64, _work: *mut f64) -> f64 = Rf_bessel_j(x, nu);
    fn Rf_bessel_k_ex(x: f64, nu: f64, expo: f64, _work: *mut f64) -> f64 = Rf_bessel_k(x, nu, expo);
    fn Rf_bessel_y_ex(x: f64, nu: f64, _work: *mut f64) -> f64 = Rf_bessel_y(x, nu);
}

/// Release signed-rank tables. The kernels retain no tables between calls.
#[unsafe(no_mangle)]
pub extern "C" fn signrank_free() {}

/// Release Wilcoxon tables. The kernels retain no tables between calls.
#[unsafe(no_mangle)]
pub extern "C" fn wilcox_free() {}

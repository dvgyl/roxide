//! Check special-function identities and status flags.

#![forbid(unsafe_code)]

use std::f64::consts::PI;

use roxide_math::Flags;
use roxide_math::special::{
    aux::{
        bd0, chebyshev_eval, cospi, ebd0, lgamma1p, log1mexp, log1pexp, log1pmx, sinpi, stirlerr,
        tanpi,
    },
    bessel_i, bessel_j, bessel_k, bessel_y, choose, gamma, lbeta, lgamma_sign, psigamma,
};

fn near(actual: f64, expected: f64, rel: f64) {
    assert!(
        (actual - expected).abs() <= rel * expected.abs(),
        "{actual:.17e} differs from {expected:.17e}"
    );
}

#[test]
fn reduced_trigonometry_keeps_exact_zeros_and_poles() {
    assert_eq!(sinpi(-3.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(sinpi(4_503_599_627_370_495.5), -1.0);
    assert_eq!(cospi(-3.5).to_bits(), 0.0_f64.to_bits());
    assert_eq!(tanpi(-0.0).to_bits(), 0.0_f64.to_bits());
    assert!(tanpi(0.5).is_nan());
    assert!(sinpi(f64::INFINITY).is_nan());
}

#[test]
fn auxiliary_functions_keep_small_terms() {
    near(log1pmx(1e-20), -0.5e-40, 2e-16);
    near(lgamma1p(-1e-22), 5.772_156_649_015_329e-23, 2e-16);
    near(stirlerr(0.5), 0.153_426_409_720_027_36, 2e-16);
    near(stirlerr(1e200), 1e-200 / 12.0, 3e-16);
    near(log1mexp(1e-20), 1e-20_f64.ln(), 2e-16);
    assert_eq!(log1pexp(1000.0), 1000.0);
    assert_eq!(chebyshev_eval(0.25, &[2.0, 3.0, 4.0]), -1.75);
    assert_eq!(bd0(0.0, 10.0), 10.0);
}

#[test]
fn deviance_split_retains_near_mean_and_product_residuals() {
    let x = 1e16;
    let mean = x - 2.0;
    near(bd0(x, mean), 2.0e-16, 3e-16);
    let (head, tail) = ebd0(10.0, 3.0);
    near(head + tail, 10.0 * (10.0_f64 / 3.0).ln() - 7.0, 3e-16);
    assert!(tail.abs() <= f64::EPSILON * head.abs());
    assert!(bd0(1.0, 0.0).is_nan());
}

#[test]
fn gamma_beta_and_psi_keep_values_at_large_and_small_arguments() {
    let mut flags = Flags::default();
    near(gamma(-0.5, &mut flags), -2.0 * PI.sqrt(), 2e-16);
    let (log, sign) = lgamma_sign(-1.5, &mut flags);
    near(log.exp(), 4.0 * PI.sqrt() / 3.0, 5e-16);
    assert_eq!(sign, 1);
    near(lbeta(2.0, 1e300, &mut flags), -2.0 * 1e300_f64.ln(), 2e-16);
    near(psigamma(1.0, 1, &mut flags), PI * PI / 6.0, 3e-16);
    near(psigamma(-0.5, 1, &mut flags), 4.0 + PI * PI / 2.0, 3e-16);
    assert_eq!(flags.bits(), 0);
    assert!(gamma(-2.0, &mut flags).is_nan());
    assert!(flags.has(Flags::NAN_PRODUCED));
}

#[test]
fn choose_preserves_signed_integer_values_and_rejects_bad_derivatives() {
    let mut flags = Flags::default();
    assert_eq!(choose(-4.0, 5.0, &mut flags), -56.0);
    assert_eq!(choose(10.0, 8.0, &mut flags), 45.0);
    assert_eq!(choose(10.0, 2.5, &mut flags), 45.0);
    assert!(flags.has(Flags::NONINT_X));
    assert!(psigamma(1.0, -1, &mut flags).is_nan());
    assert!(flags.has(Flags::NAN_PRODUCED));
}

#[test]
fn bessel_recurrences_hold_across_seed_and_transition_regions() {
    let mut flags = Flags::default();
    for x in [0.5_f64, 2.0, 10.0, 20.0, 24.0, 50.0, 500.0] {
        let j = bessel_j(x, 0.5, &mut flags);
        let y = bessel_y(x, 0.5, &mut flags);
        let scale = (2.0 / PI / x).sqrt();
        near(j, scale * x.sin(), 5e-15);
        near(y, -scale * x.cos(), 5e-15);
        near(bessel_j(x, -0.5, &mut flags), -y, 5e-15);
        near(
            bessel_k(x, 0.5, true, &mut flags),
            (0.5 * PI / x).sqrt(),
            5e-15,
        );
        let k = bessel_k(x, 2.5, true, &mut flags);
        let expected = (0.5 * PI / x).sqrt() * (1.0 + 3.0 / x + 3.0 / (x * x));
        near(k, expected, 8e-15);
        let i = bessel_i(x, 2.5, true, &mut flags);
        near(
            bessel_i(x, -2.5, true, &mut flags),
            i + 2.0 / PI * k * (-2.0 * x).exp(),
            5e-15,
        );
        let next_i = bessel_i(x, 3.5, true, &mut flags);
        let previous_i = bessel_i(x, 1.5, true, &mut flags);
        near(i * (5.0 / x) + next_i, previous_i, 4e-15);
    }
    assert_eq!(flags.bits(), 0);
}

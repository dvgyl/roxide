//! Negative binomial masses, beta tails, and gamma–Poisson random draws.

use super::*;
use crate::dist::continuous::{pbeta_raw, rgamma};
use crate::rng::Rng;
use crate::special::aux::{bd0, log1pmx, stirlerr};

#[derive(Clone, Copy)]
struct Parameters {
    size: f64,
    prob: f64,
    failure: f64,
    mean: f64,
    from_mean: bool,
}

impl Parameters {
    fn with_prob(size: f64, prob: f64) -> Option<Self> {
        (size >= 0.0 && prob > 0.0 && prob <= 1.0).then(|| {
            let failure = 1.0 - prob;
            Self {
                size,
                prob,
                failure,
                mean: if failure == 0.0 {
                    0.0
                } else {
                    (size * failure) / prob
                },
                from_mean: false,
            }
        })
    }

    fn with_mean(size: f64, mean: f64) -> Option<Self> {
        (size >= 0.0 && mean >= 0.0 && mean.is_finite()).then(|| {
            let (prob, failure) = if mean == 0.0 || size == INF {
                (1.0, 0.0)
            } else if size >= mean {
                let ratio = mean / size;
                (1.0 / (1.0 + ratio), ratio / (1.0 + ratio))
            } else {
                let ratio = size / mean;
                (ratio / (1.0 + ratio), 1.0 / (1.0 + ratio))
            };
            Self {
                size,
                prob,
                failure,
                mean,
                from_mean: true,
            }
        })
    }

    fn atom(self) -> bool {
        self.size == 0.0 || (self.mean == 0.0 && self.failure == 0.0)
    }

    fn mass(self, x: f64, log: bool, flags: &mut Flags) -> f64 {
        let Some(x) = observation(x, true, flags) else {
            return zero(log);
        };
        if self.atom() {
            return if x == 0.0 {
                on_scale(0.0, log)
            } else {
                zero(log)
            };
        }
        let Self {
            size,
            prob,
            failure,
            mean,
            ..
        } = self;
        if size == INF && mean == INF {
            return zero(log);
        }
        if size == INF || (failure == 0.0 && mean.is_finite()) {
            return dpois(x, mean, log, flags);
        }
        let log_prob = if failure < 0.5 {
            (-failure).ln_1p()
        } else if prob > 0.0 {
            prob.ln()
        } else {
            size.ln() - mean.ln()
        };
        let log_failure = if prob < 0.5 {
            (-prob).ln_1p()
        } else {
            failure.ln()
        };
        if x == 0.0 {
            return on_scale(size * log_prob, log);
        }

        if size < 1e-8 && x < 8.0 {
            let correction: f64 = (1..x as u32).map(|j| (size / f64::from(j)).ln_1p()).sum();
            return on_scale(
                size.ln() - x.ln() + correction + size * log_prob + x * log_failure,
                log,
            );
        }
        if x >= 8.0 && size <= 8.0 && size <= 0.125 * x {
            let gamma_ratio = size * (x.ln() - 1.0)
                + (x + size - 0.5) * (size / x).ln_1p()
                + (stirlerr(x + size) - stirlerr(x));
            let correction =
                gamma_ratio - x.ln() - crate::special::lgamma(size, flags) + size * log_prob;
            return if log {
                x.mul_add(log_failure, correction)
            } else {
                // Separate the failure power from the moderate gamma ratio.
                let lp = x * log_failure;
                if lp > -700.0 && correction.abs() < 700.0 {
                    if self.prob >= 0.25 {
                        failure.powf(x) * correction.exp()
                    } else {
                        lp.exp() * (correction + x.mul_add(log_failure, -lp)).exp()
                    }
                } else {
                    x.mul_add(log_failure, correction).exp()
                }
            };
        }
        // The mass is the binomial mass at `x`, multiplied by size / (size + x).
        // Calculate the size deviance from its displacement to preserve small means.
        let total = size + x;
        let displacement = (x * prob - size * failure) / size;
        let size_deviance = if displacement.abs() < 0.5 {
            -size * log1pmx(displacement)
        } else {
            bd0(size, x * prob + size * prob)
        };
        let small = size.min(x);
        let large = size.max(x);
        let log_normalization = (small / large).ln_1p();
        let log_ratio = if x <= size {
            -log_normalization
        } else {
            size.ln() - x.ln() - log_normalization
        };
        let lp = stirlerr(total)
            - stirlerr(size)
            - stirlerr(x)
            - size_deviance
            - bd0(x, x * failure + size * failure)
            - LOG_SQRT_2PI
            - 0.5 * (small.ln() - log_normalization)
            + log_ratio;
        on_scale(lp, log)
    }

    fn cumulative(self, q: f64, tail: Tail, flags: &mut Flags) -> f64 {
        if self.size == INF && !self.from_mean {
            return invalid(flags);
        }
        if q < 0.0 {
            return step(false, tail);
        }
        let q = count_floor(q);
        if q == INF || self.atom() {
            return step(true, tail);
        }
        if self.size == INF {
            return ppois(q, self.mean, tail, flags);
        }
        if q == 0.0 && tail.log && !tail.lower && self.prob <= 0.5 {
            let log_mass = self.size * self.prob.ln();
            let log_size = self.size.ln();
            // Keep the beta-power rounding at ordinary jump points. Near zero,
            // adding the normalization before the complement can lose the mass.
            let mass = if log_mass.abs() < f64::EPSILON.sqrt() * log_size.abs() {
                log_mass
            } else {
                (log_mass + log_size) - log_size
            };
            return from_log(mass, true, tail);
        }
        pbeta_raw(self.prob, self.failure, self.size, q + 1.0, tail, flags)
    }

    fn quantile(self, p: f64, tail: Tail, flags: &mut Flags) -> f64 {
        if !valid_probability(p, tail) {
            return invalid(flags);
        }
        if self.atom() {
            return 0.0;
        }
        if let Some(x) = endpoints(p, tail, 0.0, INF) {
            return x;
        }
        if self.size == INF && !self.from_mean {
            return invalid(flags);
        }
        if self.size == INF {
            return qpois(p, self.mean, tail, flags);
        }
        let sd = self.mean.sqrt() / self.prob.sqrt();
        let guess = cornish_fisher(
            p,
            tail,
            self.mean,
            sd,
            (1.0 + self.failure) / (sd * self.prob),
            flags,
        );
        quantile_search(p, tail, guess, INF, |x, tail| {
            self.cumulative(x, tail, flags)
        })
    }
}

macro_rules! negative_binomial {
    ($d:ident, $p:ident, $q:ident, $parameter:ident, $constructor:ident) => {
        #[doc = concat!("Return the negative binomial mass with parameter `", stringify!($parameter), "`.")]
        pub fn $d(x: f64, size: f64, $parameter: f64, log: bool, flags: &mut Flags) -> f64 {
            if x.is_nan() || size.is_nan() || $parameter.is_nan() {
                return x + size + $parameter;
            }
            match Parameters::$constructor(size, $parameter) {
                Some(parameters) => parameters.mass(x, log, flags),
                None => invalid(flags),
            }
        }

        #[doc = concat!("Return the requested negative binomial tail with parameter `", stringify!($parameter), "`.")]
        pub fn $p(q: f64, size: f64, $parameter: f64, tail: Tail, flags: &mut Flags) -> f64 {
            if q.is_nan() || size.is_nan() || $parameter.is_nan() {
                return q + size + $parameter;
            }
            match Parameters::$constructor(size, $parameter) {
                Some(parameters) => parameters.cumulative(q, tail, flags),
                None => invalid(flags),
            }
        }

        #[doc = concat!("Return the smallest negative binomial count at the requested probability with parameter `", stringify!($parameter), "`.")]
        pub fn $q(p: f64, size: f64, $parameter: f64, tail: Tail, flags: &mut Flags) -> f64 {
            if p.is_nan() || size.is_nan() || $parameter.is_nan() {
                return p + size + $parameter;
            }
            match Parameters::$constructor(size, $parameter) {
                Some(parameters) => parameters.quantile(p, tail, flags),
                None => invalid(flags),
            }
        }
    };
}

negative_binomial!(dnbinom, pnbinom, qnbinom, prob, with_prob);
negative_binomial!(dnbinom_mu, pnbinom_mu, qnbinom_mu, mu, with_mean);

/// Draw a negative binomial count from a size and success probability.
pub fn rnbinom(rng: &mut Rng, size: f64, prob: f64, flags: &mut Flags) -> f64 {
    if size.is_nan() || size <= 0.0 || !(prob > 0.0 && prob <= 1.0) {
        return invalid(flags);
    }
    let shape = if size == INF { f64::MAX / 2.0 } else { size };
    let mean = rgamma(rng, shape, (1.0 - prob) / prob, flags);
    rpois(rng, mean, flags)
}

/// Draw a negative binomial count from a size and mean.
pub fn rnbinom_mu(rng: &mut Rng, size: f64, mu: f64, flags: &mut Flags) -> f64 {
    if size <= 0.0 || size.is_nan() || !mu.is_finite() || mu < 0.0 {
        return invalid(flags);
    }
    let mean = if size == INF {
        mu
    } else {
        rgamma(rng, size, mu / size, flags)
    };
    rpois(rng, mean, flags)
}

use roxide_eval::{Args, Call, Ctx, EvalResult, PrimDef};
use roxide_math::dist::{continuous, discrete, normal};

use super::support::internal;
use super::{KernelWarnings, RandomOutput, density, random, tail};

macro_rules! vector {
    ($kind:ident, $entry:ident, $kernel:path, [$($arg:ident),+]) => {{
        fn apply(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            vector!(@apply $kind ctx, call, args, |[$($arg),+], option, flags| {
                $kernel($($arg),+, option, flags)
            })
        }
        let option_count = vector!(@options $kind);
        internal(stringify!($entry), [$(stringify!($arg)),+].len() + option_count, apply)
    }};
    (@apply density $ctx:ident, $call:ident, $args:ident, $kernel:expr) => {
        density($ctx, $call, $args, $kernel)
    };
    (@apply tail $ctx:ident, $call:ident, $args:ident, $kernel:expr) => {
        tail($ctx, $call, $args, KernelWarnings::Default, $kernel)
    };
    (@options density) => { 1 };
    (@options tail) => { 2 };
}

macro_rules! random {
    ($entry:ident, $kernel:path, [$($arg:ident),+], $output:expr) => {{
        fn apply(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            random(ctx, call, args, $output, |[$($arg),+], rng, flags| {
                $kernel(rng, $($arg),+, flags)
            })
        }
        internal(stringify!($entry), [$(stringify!($arg)),+].len() + 1, apply)
    }};
}

pub(super) const TABLE: &[PrimDef] = &[
    vector!(density, C_dnorm, normal::dnorm, [a, b, c]),
    vector!(tail, C_pnorm, normal::pnorm, [a, b, c]),
    vector!(tail, C_qnorm, normal::qnorm, [a, b, c]),
    random!(C_rnorm, normal::rnorm, [a, b], RandomOutput::Real),
    vector!(density, C_dunif, continuous::dunif, [a, b, c]),
    vector!(tail, C_punif, continuous::punif, [a, b, c]),
    vector!(tail, C_qunif, continuous::qunif, [a, b, c]),
    random!(C_runif, continuous::runif, [a, b], RandomOutput::Real),
    vector!(density, C_dexp, continuous::dexp, [a, b]),
    vector!(tail, C_pexp, continuous::pexp, [a, b]),
    vector!(tail, C_qexp, continuous::qexp, [a, b]),
    random!(C_rexp, continuous::rexp, [a], RandomOutput::Real),
    vector!(density, C_dlnorm, continuous::dlnorm, [a, b, c]),
    vector!(tail, C_plnorm, continuous::plnorm, [a, b, c]),
    vector!(tail, C_qlnorm, continuous::qlnorm, [a, b, c]),
    random!(C_rlnorm, continuous::rlnorm, [a, b], RandomOutput::Real),
    vector!(density, C_dlogis, continuous::dlogis, [a, b, c]),
    vector!(tail, C_plogis, continuous::plogis, [a, b, c]),
    vector!(tail, C_qlogis, continuous::qlogis, [a, b, c]),
    random!(C_rlogis, continuous::rlogis, [a, b], RandomOutput::Real),
    vector!(density, C_dcauchy, continuous::dcauchy, [a, b, c]),
    vector!(tail, C_pcauchy, continuous::pcauchy, [a, b, c]),
    vector!(tail, C_qcauchy, continuous::qcauchy, [a, b, c]),
    random!(C_rcauchy, continuous::rcauchy, [a, b], RandomOutput::Real),
    vector!(density, C_dweibull, continuous::dweibull, [a, b, c]),
    vector!(tail, C_pweibull, continuous::pweibull, [a, b, c]),
    vector!(tail, C_qweibull, continuous::qweibull, [a, b, c]),
    random!(C_rweibull, continuous::rweibull, [a, b], RandomOutput::Real),
    vector!(density, C_dgamma, continuous::dgamma, [a, b, c]),
    vector!(tail, C_pgamma, continuous::pgamma, [a, b, c]),
    vector!(tail, C_qgamma, continuous::qgamma, [a, b, c]),
    random!(C_rgamma, continuous::rgamma, [a, b], RandomOutput::Real),
    vector!(density, C_dbeta, continuous::dbeta, [a, b, c]),
    vector!(density, C_dnbeta, continuous::dnbeta, [a, b, c, d]),
    vector!(tail, C_pbeta, continuous::pbeta, [a, b, c]),
    vector!(tail, C_pnbeta, continuous::pnbeta, [a, b, c, d]),
    vector!(tail, C_qbeta, continuous::qbeta, [a, b, c]),
    vector!(tail, C_qnbeta, continuous::qnbeta, [a, b, c, d]),
    random!(C_rbeta, continuous::rbeta, [a, b], RandomOutput::Real),
    vector!(density, C_dchisq, continuous::dchisq, [a, b]),
    vector!(density, C_dnchisq, continuous::dnchisq, [a, b, c]),
    vector!(tail, C_pchisq, continuous::pchisq, [a, b]),
    vector!(tail, C_pnchisq, continuous::pnchisq, [a, b, c]),
    vector!(tail, C_qchisq, continuous::qchisq, [a, b]),
    vector!(tail, C_qnchisq, continuous::qnchisq, [a, b, c]),
    random!(C_rchisq, continuous::rchisq, [a], RandomOutput::Real),
    random!(C_rnchisq, continuous::rnchisq, [a, b], RandomOutput::Real),
    vector!(density, C_dt, continuous::dt, [a, b]),
    vector!(density, C_dnt, continuous::dnt, [a, b, c]),
    vector!(tail, C_pt, continuous::pt, [a, b]),
    vector!(tail, C_pnt, continuous::pnt, [a, b, c]),
    vector!(tail, C_qt, continuous::qt, [a, b]),
    vector!(tail, C_qnt, continuous::qnt, [a, b, c]),
    random!(C_rt, continuous::rt, [a], RandomOutput::Real),
    vector!(density, C_df, continuous::df, [a, b, c]),
    vector!(density, C_dnf, continuous::dnf, [a, b, c, d]),
    vector!(tail, C_pf, continuous::pf, [a, b, c]),
    vector!(tail, C_pnf, continuous::pnf, [a, b, c, d]),
    vector!(tail, C_qf, continuous::qf, [a, b, c]),
    vector!(tail, C_qnf, continuous::qnf, [a, b, c, d]),
    random!(C_rf, continuous::rf, [a, b], RandomOutput::Real),
    vector!(tail, C_ptukey, continuous::ptukey, [a, b, c, d]),
    internal("C_qtukey", 6, qtukey),
    vector!(density, C_dbinom, discrete::dbinom, [a, b, c]),
    internal("C_pbinom", 5, pbinom),
    vector!(tail, C_qbinom, discrete::qbinom, [a, b, c]),
    random!(C_rbinom, discrete::rbinom, [a, b], RandomOutput::Integer),
    vector!(density, C_dpois, discrete::dpois, [a, b]),
    vector!(tail, C_ppois, discrete::ppois, [a, b]),
    vector!(tail, C_qpois, discrete::qpois, [a, b]),
    random!(C_rpois, discrete::rpois, [a], RandomOutput::Integer),
    vector!(density, C_dnbinom, discrete::dnbinom, [a, b, c]),
    vector!(density, C_dnbinom_mu, discrete::dnbinom_mu, [a, b, c]),
    vector!(tail, C_pnbinom, discrete::pnbinom, [a, b, c]),
    vector!(tail, C_pnbinom_mu, discrete::pnbinom_mu, [a, b, c]),
    vector!(tail, C_qnbinom, discrete::qnbinom, [a, b, c]),
    vector!(tail, C_qnbinom_mu, discrete::qnbinom_mu, [a, b, c]),
    random!(C_rnbinom, discrete::rnbinom, [a, b], RandomOutput::Integer),
    random!(
        C_rnbinom_mu,
        discrete::rnbinom_mu,
        [a, b],
        RandomOutput::Real
    ),
    vector!(density, C_dgeom, discrete::dgeom, [a, b]),
    vector!(tail, C_pgeom, discrete::pgeom, [a, b]),
    vector!(tail, C_qgeom, discrete::qgeom, [a, b]),
    random!(C_rgeom, discrete::rgeom, [a], RandomOutput::Integer),
    vector!(density, C_dhyper, discrete::dhyper, [a, b, c, d]),
    vector!(tail, C_phyper, discrete::phyper, [a, b, c, d]),
    vector!(tail, C_qhyper, discrete::qhyper, [a, b, c, d]),
    random!(C_rhyper, discrete::rhyper, [a, b, c], RandomOutput::Integer),
    vector!(density, C_dwilcox, discrete::dwilcox, [a, b, c]),
    vector!(tail, C_pwilcox, discrete::pwilcox, [a, b, c]),
    vector!(tail, C_qwilcox, discrete::qwilcox, [a, b, c]),
    random!(C_rwilcox, discrete::rwilcox, [a, b], RandomOutput::Integer),
    vector!(density, C_dsignrank, discrete::dsignrank, [a, b]),
    vector!(tail, C_psignrank, discrete::psignrank, [a, b]),
    vector!(tail, C_qsignrank, discrete::qsignrank, [a, b]),
    random!(C_rsignrank, discrete::rsignrank, [a], RandomOutput::Integer),
    internal("C_dmultinom", 4, super::multinomial::density),
    internal("C_rmultinom", 3, super::multinomial::random),
];

fn pbinom(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    tail(
        ctx,
        call,
        args,
        KernelWarnings::BinomialSize,
        |[q, n, p], options, flags| discrete::pbinom(q, n, p, options, flags),
    )
}

fn qtukey(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    tail(
        ctx,
        call,
        args,
        KernelWarnings::TukeyQuantile,
        |[p, n, df, ranges], options, flags| continuous::qtukey(p, n, df, ranges, options, flags),
    )
}

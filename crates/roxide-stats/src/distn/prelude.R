dnorm <- function(x, mean = 0, sd = 1, log = FALSE) .Internal(C_dnorm(x, mean, sd, log))
pnorm <- function(q, mean = 0, sd = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_pnorm(q, mean, sd, lower.tail, log.p))
qnorm <- function(p, mean = 0, sd = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_qnorm(p, mean, sd, lower.tail, log.p))
rnorm <- function(n, mean = 0, sd = 1) .Internal(C_rnorm(n, mean, sd))
dunif <- function(x, min = 0, max = 1, log = FALSE) .Internal(C_dunif(x, min, max, log))
punif <- function(q, min = 0, max = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_punif(q, min, max, lower.tail, log.p))
qunif <- function(p, min = 0, max = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_qunif(p, min, max, lower.tail, log.p))
runif <- function(n, min = 0, max = 1) .Internal(C_runif(n, min, max))
dexp <- function(x, rate = 1, log = FALSE) .Internal(C_dexp(x, rate, log))
pexp <- function(q, rate = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_pexp(q, rate, lower.tail, log.p))
qexp <- function(p, rate = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_qexp(p, rate, lower.tail, log.p))
rexp <- function(n, rate = 1) .Internal(C_rexp(n, rate))
dlnorm <- function(x, meanlog = 0, sdlog = 1, log = FALSE) .Internal(C_dlnorm(x, meanlog, sdlog, log))
plnorm <- function(q, meanlog = 0, sdlog = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_plnorm(q, meanlog, sdlog, lower.tail, log.p))
qlnorm <- function(p, meanlog = 0, sdlog = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_qlnorm(p, meanlog, sdlog, lower.tail, log.p))
rlnorm <- function(n, meanlog = 0, sdlog = 1) .Internal(C_rlnorm(n, meanlog, sdlog))
dlogis <- function(x, location = 0, scale = 1, log = FALSE) .Internal(C_dlogis(x, location, scale, log))
plogis <- function(q, location = 0, scale = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_plogis(q, location, scale, lower.tail, log.p))
qlogis <- function(p, location = 0, scale = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_qlogis(p, location, scale, lower.tail, log.p))
rlogis <- function(n, location = 0, scale = 1) .Internal(C_rlogis(n, location, scale))
dcauchy <- function(x, location = 0, scale = 1, log = FALSE) .Internal(C_dcauchy(x, location, scale, log))
pcauchy <- function(q, location = 0, scale = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_pcauchy(q, location, scale, lower.tail, log.p))
qcauchy <- function(p, location = 0, scale = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_qcauchy(p, location, scale, lower.tail, log.p))
rcauchy <- function(n, location = 0, scale = 1) .Internal(C_rcauchy(n, location, scale))
dweibull <- function(x, shape, scale = 1, log = FALSE) .Internal(C_dweibull(x, shape, scale, log))
pweibull <- function(q, shape, scale = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_pweibull(q, shape, scale, lower.tail, log.p))
qweibull <- function(p, shape, scale = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_qweibull(p, shape, scale, lower.tail, log.p))
rweibull <- function(n, shape, scale = 1) .Internal(C_rweibull(n, shape, scale))
dgamma <- function(x, shape, rate = 1, scale = 1/rate, log = FALSE) {
    if (!missing(rate) && !missing(scale)) {
        if (abs(rate * scale - 1) > 1e-15) stop("specify 'rate' or 'scale' but not both")
        warning("specify 'rate' or 'scale' but not both")
    }
    .Internal(C_dgamma(x, shape, scale, log))
}
pgamma <- function(q, shape, rate = 1, scale = 1/rate, lower.tail = TRUE, log.p = FALSE) {
    if (!missing(rate) && !missing(scale)) {
        if (abs(rate * scale - 1) > 1e-15) stop("specify 'rate' or 'scale' but not both")
        warning("specify 'rate' or 'scale' but not both")
    }
    .Internal(C_pgamma(q, shape, scale, lower.tail, log.p))
}
qgamma <- function(p, shape, rate = 1, scale = 1/rate, lower.tail = TRUE, log.p = FALSE) {
    if (!missing(rate) && !missing(scale)) {
        if (abs(rate * scale - 1) > 1e-15) stop("specify 'rate' or 'scale' but not both")
        warning("specify 'rate' or 'scale' but not both")
    }
    .Internal(C_qgamma(p, shape, scale, lower.tail, log.p))
}
rgamma <- function(n, shape, rate = 1, scale = 1/rate) {
    if (!missing(rate) && !missing(scale)) {
        if (abs(rate * scale - 1) > 1e-15) stop("specify 'rate' or 'scale' but not both")
        warning("specify 'rate' or 'scale' but not both")
    }
    .Internal(C_rgamma(n, shape, scale))
}
dbeta <- function(x, shape1, shape2, ncp = 0, log = FALSE) if (missing(ncp)) .Internal(C_dbeta(x, shape1, shape2, log)) else .Internal(C_dnbeta(x, shape1, shape2, ncp, log))
pbeta <- function(q, shape1, shape2, ncp = 0, lower.tail = TRUE, log.p = FALSE) if (missing(ncp)) .Internal(C_pbeta(q, shape1, shape2, lower.tail, log.p)) else .Internal(C_pnbeta(q, shape1, shape2, ncp, lower.tail, log.p))
qbeta <- function(p, shape1, shape2, ncp = 0, lower.tail = TRUE, log.p = FALSE) if (missing(ncp)) .Internal(C_qbeta(p, shape1, shape2, lower.tail, log.p)) else .Internal(C_qnbeta(p, shape1, shape2, ncp, lower.tail, log.p))
rbeta <- function(n, shape1, shape2, ncp = 0) if (missing(ncp)) .Internal(C_rbeta(n, shape1, shape2)) else {
    x <- rchisq(n, 2 * shape1, ncp)
    x / (x + rchisq(n, 2 * shape2))
}
dchisq <- function(x, df, ncp = 0, log = FALSE) if (missing(ncp)) .Internal(C_dchisq(x, df, log)) else .Internal(C_dnchisq(x, df, ncp, log))
pchisq <- function(q, df, ncp = 0, lower.tail = TRUE, log.p = FALSE) if (missing(ncp)) .Internal(C_pchisq(q, df, lower.tail, log.p)) else .Internal(C_pnchisq(q, df, ncp, lower.tail, log.p))
qchisq <- function(p, df, ncp = 0, lower.tail = TRUE, log.p = FALSE) if (missing(ncp)) .Internal(C_qchisq(p, df, lower.tail, log.p)) else .Internal(C_qnchisq(p, df, ncp, lower.tail, log.p))
rchisq <- function(n, df, ncp = 0) if (missing(ncp)) .Internal(C_rchisq(n, df)) else .Internal(C_rnchisq(n, df, ncp))
dt <- function(x, df, ncp, log = FALSE) if (missing(ncp)) .Internal(C_dt(x, df, log)) else .Internal(C_dnt(x, df, ncp, log))
pt <- function(q, df, ncp, lower.tail = TRUE, log.p = FALSE) if (missing(ncp)) .Internal(C_pt(q, df, lower.tail, log.p)) else .Internal(C_pnt(q, df, ncp, lower.tail, log.p))
qt <- function(p, df, ncp, lower.tail = TRUE, log.p = FALSE) if (missing(ncp)) .Internal(C_qt(p, df, lower.tail, log.p)) else .Internal(C_qnt(p, df, ncp, lower.tail, log.p))
rt <- function(n, df, ncp) if (missing(ncp)) .Internal(C_rt(n, df)) else rnorm(n, ncp) / sqrt(rchisq(n, df) / df)
df <- function(x, df1, df2, ncp, log = FALSE) if (missing(ncp)) .Internal(C_df(x, df1, df2, log)) else .Internal(C_dnf(x, df1, df2, ncp, log))
pf <- function(q, df1, df2, ncp, lower.tail = TRUE, log.p = FALSE) if (missing(ncp)) .Internal(C_pf(q, df1, df2, lower.tail, log.p)) else .Internal(C_pnf(q, df1, df2, ncp, lower.tail, log.p))
qf <- function(p, df1, df2, ncp, lower.tail = TRUE, log.p = FALSE) if (missing(ncp)) .Internal(C_qf(p, df1, df2, lower.tail, log.p)) else .Internal(C_qnf(p, df1, df2, ncp, lower.tail, log.p))
rf <- function(n, df1, df2, ncp) if (missing(ncp)) .Internal(C_rf(n, df1, df2)) else (rchisq(n, df1, ncp) / df1) / (rchisq(n, df2) / df2)
ptukey <- function(q, nmeans, df, nranges = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_ptukey(q, nmeans, df, nranges, lower.tail, log.p))
qtukey <- function(p, nmeans, df, nranges = 1, lower.tail = TRUE, log.p = FALSE) .Internal(C_qtukey(p, nmeans, df, nranges, lower.tail, log.p))
dbinom <- function(x, size, prob, log = FALSE) .Internal(C_dbinom(x, size, prob, log))
pbinom <- function(q, size, prob, lower.tail = TRUE, log.p = FALSE) .Internal(C_pbinom(q, size, prob, lower.tail, log.p))
qbinom <- function(p, size, prob, lower.tail = TRUE, log.p = FALSE) .Internal(C_qbinom(p, size, prob, lower.tail, log.p))
rbinom <- function(n, size, prob) .Internal(C_rbinom(n, size, prob))
dpois <- function(x, lambda, log = FALSE) .Internal(C_dpois(x, lambda, log))
ppois <- function(q, lambda, lower.tail = TRUE, log.p = FALSE) .Internal(C_ppois(q, lambda, lower.tail, log.p))
qpois <- function(p, lambda, lower.tail = TRUE, log.p = FALSE) .Internal(C_qpois(p, lambda, lower.tail, log.p))
rpois <- function(n, lambda) .Internal(C_rpois(n, lambda))
dnbinom <- function(x, size, prob, mu, log = FALSE) {
    if (!missing(mu)) {
        if (!missing(prob)) stop("'prob' and 'mu' both specified")
        .Internal(C_dnbinom_mu(x, size, mu, log))
    } else .Internal(C_dnbinom(x, size, prob, log))
}
pnbinom <- function(q, size, prob, mu, lower.tail = TRUE, log.p = FALSE) {
    if (!missing(mu)) {
        if (!missing(prob)) stop("'prob' and 'mu' both specified")
        .Internal(C_pnbinom_mu(q, size, mu, lower.tail, log.p))
    } else .Internal(C_pnbinom(q, size, prob, lower.tail, log.p))
}
qnbinom <- function(p, size, prob, mu, lower.tail = TRUE, log.p = FALSE) {
    if (!missing(mu)) {
        if (!missing(prob)) stop("'prob' and 'mu' both specified")
        .Internal(C_qnbinom_mu(p, size, mu, lower.tail, log.p))
    } else .Internal(C_qnbinom(p, size, prob, lower.tail, log.p))
}
rnbinom <- function(n, size, prob, mu) {
    if (!missing(mu)) {
        if (!missing(prob)) stop("'prob' and 'mu' both specified")
        .Internal(C_rnbinom_mu(n, size, mu))
    } else .Internal(C_rnbinom(n, size, prob))
}
dgeom <- function(x, prob, log = FALSE) .Internal(C_dgeom(x, prob, log))
pgeom <- function(q, prob, lower.tail = TRUE, log.p = FALSE) .Internal(C_pgeom(q, prob, lower.tail, log.p))
qgeom <- function(p, prob, lower.tail = TRUE, log.p = FALSE) .Internal(C_qgeom(p, prob, lower.tail, log.p))
rgeom <- function(n, prob) .Internal(C_rgeom(n, prob))
dhyper <- function(x, m, n, k, log = FALSE) .Internal(C_dhyper(x, m, n, k, log))
phyper <- function(q, m, n, k, lower.tail = TRUE, log.p = FALSE) .Internal(C_phyper(q, m, n, k, lower.tail, log.p))
qhyper <- function(p, m, n, k, lower.tail = TRUE, log.p = FALSE) .Internal(C_qhyper(p, m, n, k, lower.tail, log.p))
rhyper <- function(nn, m, n, k) .Internal(C_rhyper(nn, m, n, k))
dwilcox <- function(x, m, n, log = FALSE) .Internal(C_dwilcox(x, m, n, log))
pwilcox <- function(q, m, n, lower.tail = TRUE, log.p = FALSE) .Internal(C_pwilcox(q, m, n, lower.tail, log.p))
qwilcox <- function(p, m, n, lower.tail = TRUE, log.p = FALSE) .Internal(C_qwilcox(p, m, n, lower.tail, log.p))
rwilcox <- function(nn, m, n) .Internal(C_rwilcox(nn, m, n))
dsignrank <- function(x, n, log = FALSE) .Internal(C_dsignrank(x, n, log))
psignrank <- function(q, n, lower.tail = TRUE, log.p = FALSE) .Internal(C_psignrank(q, n, lower.tail, log.p))
qsignrank <- function(p, n, lower.tail = TRUE, log.p = FALSE) .Internal(C_qsignrank(p, n, lower.tail, log.p))
rsignrank <- function(nn, n) .Internal(C_rsignrank(nn, n))

dmultinom <- function(x, size = NULL, prob, log = FALSE) .Internal(C_dmultinom(x, size, prob, log))
rmultinom <- function(n, size, prob) .Internal(C_rmultinom(n, size, prob))

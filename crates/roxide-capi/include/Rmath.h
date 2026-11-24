/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_RMATH_H
#define ROXIDE_RMATH_H

#include <math.h>
#include <R_ext/Constants.h>

#ifdef __cplusplus
extern "C" {
#endif

#define R_VERSION_STRING "4.6.2"
#define R_DBL_MIN DBL_MIN
#define R_DBL_MAX DBL_MAX
#define R_DBL_EPSILON DBL_EPSILON

#ifdef MATHLIB_STANDALONE
void set_seed(unsigned int first, unsigned int second);
void get_seed(unsigned int *first, unsigned int *second);
extern double R_NaN;
extern double R_PosInf;
extern double R_NegInf;
#endif

double unif_rand(void);
double norm_rand(void);
double exp_rand(void);
void signrank_free(void);
void wilcox_free(void);
double Rtanpi(double x);
double sinpi(double x);
double cospi(double x);
double tanpi(double x);
double pow1p(double x, double y);
double R_pow(double x, double y);
double R_pow_di(double x, int n);

#define dnorm dnorm4
#define pnorm pnorm5
#define qnorm qnorm5

#ifdef MATHLIB_STANDALONE
#define ROXIDE_MATH_NAME(name) name
#else
#define ROXIDE_MATH_NAME(name) Rf_ ## name
#endif

double ROXIDE_MATH_NAME(bessel_i)(double x, double nu, double expo);
double ROXIDE_MATH_NAME(bessel_i_ex)(double x, double nu, double expo, double *work);
double ROXIDE_MATH_NAME(bessel_j)(double x, double nu);
double ROXIDE_MATH_NAME(bessel_j_ex)(double x, double nu, double *work);
double ROXIDE_MATH_NAME(bessel_k)(double x, double nu, double expo);
double ROXIDE_MATH_NAME(bessel_k_ex)(double x, double nu, double expo, double *work);
double ROXIDE_MATH_NAME(bessel_y)(double x, double nu);
double ROXIDE_MATH_NAME(bessel_y_ex)(double x, double nu, double *work);
double ROXIDE_MATH_NAME(beta)(double x, double y);
double ROXIDE_MATH_NAME(choose)(double x, double y);
double ROXIDE_MATH_NAME(dbeta)(double x, double a, double b, int give_log);
double ROXIDE_MATH_NAME(dbinom)(double x, double size, double prob, int give_log);
double ROXIDE_MATH_NAME(dbinom_raw)(double x, double n, double p, double q, int give_log);
double ROXIDE_MATH_NAME(dcauchy)(double x, double location, double scale, int give_log);
double ROXIDE_MATH_NAME(dchisq)(double x, double df, int give_log);
double ROXIDE_MATH_NAME(dexp)(double x, double scale, int give_log);
double ROXIDE_MATH_NAME(df)(double x, double df1, double df2, int give_log);
double ROXIDE_MATH_NAME(dgamma)(double x, double shape, double scale, int give_log);
double ROXIDE_MATH_NAME(dgeom)(double x, double prob, int give_log);
double ROXIDE_MATH_NAME(dhyper)(double x, double m, double n, double k, int give_log);
double ROXIDE_MATH_NAME(digamma)(double x);
double ROXIDE_MATH_NAME(dlnorm)(double x, double meanlog, double sdlog, int give_log);
double ROXIDE_MATH_NAME(dlogis)(double x, double location, double scale, int give_log);
double ROXIDE_MATH_NAME(dnbeta)(double x, double a, double b, double ncp, int give_log);
double ROXIDE_MATH_NAME(dnbinom)(double x, double size, double prob, int give_log);
double ROXIDE_MATH_NAME(dnbinom_mu)(double x, double size, double mu, int give_log);
double ROXIDE_MATH_NAME(dnchisq)(double x, double df, double ncp, int give_log);
double ROXIDE_MATH_NAME(dnf)(double x, double df1, double df2, double ncp, int give_log);
double ROXIDE_MATH_NAME(dnorm4)(double x, double mean, double sd, int give_log);
double ROXIDE_MATH_NAME(dnt)(double x, double df, double ncp, int give_log);
double ROXIDE_MATH_NAME(dpois)(double x, double lambda, int give_log);
double ROXIDE_MATH_NAME(dpois_raw)(double x, double lambda, int give_log);
void ROXIDE_MATH_NAME(dpsifn)(double x, int n, int kode, int m, double *ans, int *nz, int *ierr);
double ROXIDE_MATH_NAME(dsignrank)(double x, double n, int give_log);
double ROXIDE_MATH_NAME(dt)(double x, double df, int give_log);
double ROXIDE_MATH_NAME(dunif)(double x, double min, double max, int give_log);
double ROXIDE_MATH_NAME(dweibull)(double x, double shape, double scale, int give_log);
double ROXIDE_MATH_NAME(dwilcox)(double x, double m, double n, int give_log);
double ROXIDE_MATH_NAME(fmax2)(double x, double y);
double ROXIDE_MATH_NAME(fmin2)(double x, double y);
double ROXIDE_MATH_NAME(fprec)(double x, double y);
double ROXIDE_MATH_NAME(fround)(double x, double y);
double ROXIDE_MATH_NAME(fsign)(double x, double y);
double ROXIDE_MATH_NAME(ftrunc)(double x);
double ROXIDE_MATH_NAME(gammafn)(double x);
int ROXIDE_MATH_NAME(imax2)(int x, int y);
int ROXIDE_MATH_NAME(imin2)(int x, int y);
double ROXIDE_MATH_NAME(lbeta)(double x, double y);
double ROXIDE_MATH_NAME(lchoose)(double x, double y);
double ROXIDE_MATH_NAME(lgamma1p)(double x);
double ROXIDE_MATH_NAME(lgammafn)(double x);
double ROXIDE_MATH_NAME(lgammafn_sign)(double x, int *sign);
double ROXIDE_MATH_NAME(log1mexp)(double x);
double ROXIDE_MATH_NAME(log1pexp)(double x);
double ROXIDE_MATH_NAME(log1pmx)(double x);
double ROXIDE_MATH_NAME(logspace_add)(double x, double y);
double ROXIDE_MATH_NAME(logspace_sub)(double x, double y);
double ROXIDE_MATH_NAME(logspace_sum)(const double *logx, int n);
double ROXIDE_MATH_NAME(pbeta)(double x, double a, double b, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pbinom)(double x, double size, double prob, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pcauchy)(double x, double location, double scale, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pchisq)(double x, double df, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pentagamma)(double x);
double ROXIDE_MATH_NAME(pexp)(double x, double scale, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pf)(double x, double df1, double df2, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pgamma)(double x, double shape, double scale, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pgeom)(double x, double prob, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(phyper)(double x, double m, double n, double k, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(plnorm)(double x, double meanlog, double sdlog, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(plogis)(double x, double location, double scale, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pnbeta)(double x, double a, double b, double ncp, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pnbinom)(double x, double size, double prob, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pnbinom_mu)(double x, double size, double mu, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pnchisq)(double x, double df, double ncp, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pnf)(double x, double df1, double df2, double ncp, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pnorm5)(double x, double mean, double sd, int lower_tail, int log_p);
void ROXIDE_MATH_NAME(pnorm_both)(double x, double *cum, double *ccum, int tail, int log_p);
double ROXIDE_MATH_NAME(pnt)(double x, double df, double ncp, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(ppois)(double x, double lambda, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(psigamma)(double x, double y);
double ROXIDE_MATH_NAME(psignrank)(double x, double n, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pt)(double x, double df, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(ptukey)(double x, double ranges, double means, double df, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(punif)(double x, double min, double max, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pweibull)(double x, double shape, double scale, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(pwilcox)(double x, double m, double n, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qbeta)(double p, double a, double b, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qbinom)(double p, double size, double prob, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qcauchy)(double p, double location, double scale, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qchisq)(double p, double df, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qexp)(double p, double scale, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qf)(double p, double df1, double df2, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qgamma)(double p, double shape, double scale, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qgeom)(double p, double prob, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qhyper)(double p, double m, double n, double k, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qlnorm)(double p, double meanlog, double sdlog, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qlogis)(double p, double location, double scale, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qnbeta)(double p, double a, double b, double ncp, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qnbinom)(double p, double size, double prob, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qnbinom_mu)(double p, double size, double mu, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qnchisq)(double p, double df, double ncp, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qnf)(double p, double df1, double df2, double ncp, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qnorm5)(double p, double mean, double sd, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qnt)(double p, double df, double ncp, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qpois)(double p, double lambda, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qsignrank)(double p, double n, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qt)(double p, double df, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qtukey)(double p, double ranges, double means, double df, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qunif)(double p, double min, double max, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qweibull)(double p, double shape, double scale, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(qwilcox)(double p, double m, double n, int lower_tail, int log_p);
double ROXIDE_MATH_NAME(rbeta)(double a, double b);
double ROXIDE_MATH_NAME(rbinom)(double size, double prob);
double ROXIDE_MATH_NAME(rcauchy)(double location, double scale);
double ROXIDE_MATH_NAME(rchisq)(double df);
double ROXIDE_MATH_NAME(rexp)(double scale);
double ROXIDE_MATH_NAME(rf)(double df1, double df2);
double ROXIDE_MATH_NAME(rgamma)(double shape, double scale);
double ROXIDE_MATH_NAME(rgeom)(double prob);
double ROXIDE_MATH_NAME(rhyper)(double m, double n, double k);
double ROXIDE_MATH_NAME(rlnorm)(double meanlog, double sdlog);
double ROXIDE_MATH_NAME(rlogis)(double location, double scale);
void ROXIDE_MATH_NAME(rmultinom)(int n, double *probabilities, int count, int *out);
double ROXIDE_MATH_NAME(rnbinom)(double size, double prob);
double ROXIDE_MATH_NAME(rnbinom_mu)(double size, double mu);
double ROXIDE_MATH_NAME(rnchisq)(double df, double ncp);
double ROXIDE_MATH_NAME(rnorm)(double mean, double sd);
double ROXIDE_MATH_NAME(rpois)(double lambda);
double ROXIDE_MATH_NAME(rsignrank)(double n);
double ROXIDE_MATH_NAME(rt)(double df);
double ROXIDE_MATH_NAME(runif)(double min, double max);
double ROXIDE_MATH_NAME(rweibull)(double shape, double scale);
double ROXIDE_MATH_NAME(rwilcox)(double m, double n);
double ROXIDE_MATH_NAME(sign)(double x);
double ROXIDE_MATH_NAME(tetragamma)(double x);
double ROXIDE_MATH_NAME(trigamma)(double x);

#undef ROXIDE_MATH_NAME

#if !defined(MATHLIB_STANDALONE) && !defined(R_NO_REMAP_RMATH)
#define bessel_i Rf_bessel_i
#define bessel_i_ex Rf_bessel_i_ex
#define bessel_j Rf_bessel_j
#define bessel_j_ex Rf_bessel_j_ex
#define bessel_k Rf_bessel_k
#define bessel_k_ex Rf_bessel_k_ex
#define bessel_y Rf_bessel_y
#define bessel_y_ex Rf_bessel_y_ex
#define beta Rf_beta
#define choose Rf_choose
#define dbeta Rf_dbeta
#define dbinom Rf_dbinom
#define dbinom_raw Rf_dbinom_raw
#define dcauchy Rf_dcauchy
#define dchisq Rf_dchisq
#define dexp Rf_dexp
#define df Rf_df
#define dgamma Rf_dgamma
#define dgeom Rf_dgeom
#define dhyper Rf_dhyper
#define digamma Rf_digamma
#define dlnorm Rf_dlnorm
#define dlogis Rf_dlogis
#define dnbeta Rf_dnbeta
#define dnbinom Rf_dnbinom
#define dnbinom_mu Rf_dnbinom_mu
#define dnchisq Rf_dnchisq
#define dnf Rf_dnf
#define dnorm4 Rf_dnorm4
#define dnt Rf_dnt
#define dpois Rf_dpois
#define dpois_raw Rf_dpois_raw
#define dpsifn Rf_dpsifn
#define dsignrank Rf_dsignrank
#define dt Rf_dt
#define dunif Rf_dunif
#define dweibull Rf_dweibull
#define dwilcox Rf_dwilcox
#define fmax2 Rf_fmax2
#define fmin2 Rf_fmin2
#define fprec Rf_fprec
#define fround Rf_fround
#define fsign Rf_fsign
#define ftrunc Rf_ftrunc
#define gammafn Rf_gammafn
#define imax2 Rf_imax2
#define imin2 Rf_imin2
#define lbeta Rf_lbeta
#define lchoose Rf_lchoose
#define lgamma1p Rf_lgamma1p
#define lgammafn Rf_lgammafn
#define lgammafn_sign Rf_lgammafn_sign
#define log1mexp Rf_log1mexp
#define log1pexp Rf_log1pexp
#define log1pmx Rf_log1pmx
#define logspace_add Rf_logspace_add
#define logspace_sub Rf_logspace_sub
#define logspace_sum Rf_logspace_sum
#define pbeta Rf_pbeta
#define pbinom Rf_pbinom
#define pcauchy Rf_pcauchy
#define pchisq Rf_pchisq
#define pentagamma Rf_pentagamma
#define pexp Rf_pexp
#define pf Rf_pf
#define pgamma Rf_pgamma
#define pgeom Rf_pgeom
#define phyper Rf_phyper
#define plnorm Rf_plnorm
#define plogis Rf_plogis
#define pnbeta Rf_pnbeta
#define pnbinom Rf_pnbinom
#define pnbinom_mu Rf_pnbinom_mu
#define pnchisq Rf_pnchisq
#define pnf Rf_pnf
#define pnorm5 Rf_pnorm5
#define pnorm_both Rf_pnorm_both
#define pnt Rf_pnt
#define ppois Rf_ppois
#define psigamma Rf_psigamma
#define psignrank Rf_psignrank
#define pt Rf_pt
#define ptukey Rf_ptukey
#define punif Rf_punif
#define pweibull Rf_pweibull
#define pwilcox Rf_pwilcox
#define qbeta Rf_qbeta
#define qbinom Rf_qbinom
#define qcauchy Rf_qcauchy
#define qchisq Rf_qchisq
#define qexp Rf_qexp
#define qf Rf_qf
#define qgamma Rf_qgamma
#define qgeom Rf_qgeom
#define qhyper Rf_qhyper
#define qlnorm Rf_qlnorm
#define qlogis Rf_qlogis
#define qnbeta Rf_qnbeta
#define qnbinom Rf_qnbinom
#define qnbinom_mu Rf_qnbinom_mu
#define qnchisq Rf_qnchisq
#define qnf Rf_qnf
#define qnorm5 Rf_qnorm5
#define qnt Rf_qnt
#define qpois Rf_qpois
#define qsignrank Rf_qsignrank
#define qt Rf_qt
#define qtukey Rf_qtukey
#define qunif Rf_qunif
#define qweibull Rf_qweibull
#define qwilcox Rf_qwilcox
#define rbeta Rf_rbeta
#define rbinom Rf_rbinom
#define rcauchy Rf_rcauchy
#define rchisq Rf_rchisq
#define rexp Rf_rexp
#define rf Rf_rf
#define rgamma Rf_rgamma
#define rgeom Rf_rgeom
#define rhyper Rf_rhyper
#define rlnorm Rf_rlnorm
#define rlogis Rf_rlogis
#define rmultinom Rf_rmultinom
#define rnbinom Rf_rnbinom
#define rnbinom_mu Rf_rnbinom_mu
#define rnchisq Rf_rnchisq
#define rnorm Rf_rnorm
#define rpois Rf_rpois
#define rsignrank Rf_rsignrank
#define rt Rf_rt
#define runif Rf_runif
#define rweibull Rf_rweibull
#define rwilcox Rf_rwilcox
#define sign Rf_sign
#define tetragamma Rf_tetragamma
#define trigamma Rf_trigamma
#endif

#ifdef __cplusplus
}
#endif

#endif

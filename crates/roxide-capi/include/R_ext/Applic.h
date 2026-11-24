/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_APPLIC_H
#define ROXIDE_R_EXT_APPLIC_H

#include <stddef.h>
#include <R_ext/Boolean.h>
#include <R_ext/RS.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef double optimfn(int n, double *parameters, void *data);
typedef void optimgr(int n, double *parameters, double *gradient, void *data);

void vmmin(int n, double *parameters, double *minimum, optimfn fn, optimgr gr,
           int max_iterations, int trace, int *mask, double absolute_tolerance,
           double relative_tolerance, int report, void *data, int *fn_count,
           int *gr_count, int *status);
void nmmin(int n, double *initial, double *parameters, double *minimum,
           optimfn fn, int *status, double absolute_tolerance,
           double relative_tolerance, void *data, double alpha, double beta,
           double gamma, int trace, int *fn_count, int max_iterations);
void cgmin(int n, double *initial, double *parameters, double *minimum,
           optimfn fn, optimgr gr, int *status, double absolute_tolerance,
           double relative_tolerance, void *data, int type, int trace,
           int *fn_count, int *gr_count, int max_iterations);
void lbfgsb(int n, int m, double *parameters, double *lower, double *upper,
            int *bounds, double *minimum, optimfn fn, optimgr gr, int *status,
            void *data, double factr, double pgtol, int *fn_count, int *gr_count,
            int max_iterations, char *message, int trace, int report);
void samin(int n, double *parameters, double *minimum, optimfn fn,
           int max_iterations, int tmax, double temperature, int trace, void *data);

typedef void integr_fn(double *x, int n, void *data);
void Rdqags(integr_fn fn, void *data, double *lower, double *upper,
            double *absolute_tolerance, double *relative_tolerance,
            double *result, double *absolute_error, int *evaluations,
            int *status, int *limit, int *work_length, int *last,
            int *index_work, double *work);
void Rdqagi(integr_fn fn, void *data, double *bound, int *infinite,
            double *absolute_tolerance, double *relative_tolerance,
            double *result, double *absolute_error, int *evaluations,
            int *status, int *limit, int *work_length, int *last,
            int *index_work, double *work);

void F77_NAME(xerbla)(const char *routine, const int *info FCLEN);

void F77_NAME(dqrqty)(double *x, int *n, int *k, double *qraux,
                     double *y, int *ny, double *qty);
void F77_NAME(dqrqy)(double *x, int *n, int *k, double *qraux,
                    double *y, int *ny, double *qy);
void F77_NAME(dqrcf)(double *x, int *n, int *k, double *qraux,
                    double *y, int *ny, double *b, int *info);
void F77_NAME(dqrrsd)(double *x, int *n, int *k, double *qraux,
                     double *y, int *ny, double *rsd);
void F77_NAME(dqrxb)(double *x, int *n, int *k, double *qraux,
                    double *y, int *ny, double *xb);
void F77_NAME(dqrls)(double *x, int *n, int *p, double *y, int *ny, double *tol,
                    double *b, double *rsd, double *qty, int *k, int *pivot,
                    double *qraux, double *work);
void F77_NAME(dqrdc2)(double *x, int *ldx, int *n, int *p, double *tol,
                     int *rank, double *qraux, int *pivot, double *work);

#ifdef __cplusplus
}
#endif

#endif

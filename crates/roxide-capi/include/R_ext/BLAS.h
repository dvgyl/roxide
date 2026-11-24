/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_BLAS_H
#define ROXIDE_R_EXT_BLAS_H

#include <R_ext/Complex.h>
#include <R_ext/RS.h>

#ifdef __cplusplus
extern "C" {
#endif

double F77_NAME(dasum)(const int *, const double *, const int *);
void F77_NAME(daxpy)(const int *, const double *, const double *, const int *, double *, const int *);
double F77_NAME(dcabs1)(const Rcomplex *);
void F77_NAME(dcopy)(const int *, const double *, const int *, double *, const int *);
double F77_NAME(ddot)(const int *, const double *, const int *, const double *, const int *);
void F77_NAME(dgbmv)(const char *, const int *, const int *, const int *, const int *, const double *,
    const double *, const int *, const double *, const int *, const double *, double *, const int * FCLEN);
void F77_NAME(dgemm)(const char *, const char *, const int *, const int *, const int *, const double *,
    const double *, const int *, const double *, const int *, const double *, double *, const int * FCLEN FCLEN);
void F77_NAME(dgemv)(const char *, const int *, const int *, const double *, const double *,
    const int *, const double *, const int *, const double *, double *, const int * FCLEN);
void F77_NAME(dger)(const int *, const int *, const double *, const double *,
    const int *, const double *, const int *, double *, const int *);
double F77_NAME(dnrm2)(const int *, const double *, const int *);
void F77_NAME(drot)(const int *, double *, const int *,
    double *, const int *, const double *, const double *);
void F77_NAME(drotg)(double *, double *, double *, double *);
void F77_NAME(drotm)(const int *, double *, const int *, double *, const int *, const double *);
void F77_NAME(drotmg)(double *, double *, double *, const double *, double *);
void F77_NAME(dsbmv)(const char *, const int *, const int *, const double *, const double *,
    const int *, const double *, const int *, const double *, double *, const int * FCLEN);
void F77_NAME(dscal)(const int *, const double *, double *, const int *);
void F77_NAME(dspmv)(const char *, const int *, const double *, const double *,
    const double *, const int *, const double *, double *, const int * FCLEN);
void F77_NAME(dspr2)(const char *, const int *, const double *, const double *,
    const int *, const double *, const int *, double * FCLEN);
void F77_NAME(dspr)(const char *, const int *, const double *,
    const double *, const int *, double * FCLEN);
void F77_NAME(dswap)(const int *, double *, const int *, double *, const int *);
void F77_NAME(dsymm)(const char *, const char *, const int *, const int *, const double *, const double *,
    const int *, const double *, const int *, const double *, double *, const int * FCLEN FCLEN);
void F77_NAME(dsymv)(const char *, const int *, const double *, const double *, const int *,
    const double *, const int *, const double *, double *, const int * FCLEN);
void F77_NAME(dsyr2)(const char *, const int *, const double *, const double *,
    const int *, const double *, const int *, double *, const int * FCLEN);
void F77_NAME(dsyr2k)(const char *, const char *, const int *, const int *, const double *, const double *,
    const int *, const double *, const int *, const double *, double *, const int * FCLEN FCLEN);
void F77_NAME(dsyr)(const char *, const int *, const double *,
    const double *, const int *, double *, const int * FCLEN);
void F77_NAME(dsyrk)(const char *, const char *, const int *, const int *, const double *,
    const double *, const int *, const double *, double *, const int * FCLEN FCLEN);
void F77_NAME(dtbmv)(const char *, const char *, const char *, const int *,
    const int *, const double *, const int *, double *, const int * FCLEN FCLEN FCLEN);
void F77_NAME(dtbsv)(const char *, const char *, const char *, const int *,
    const int *, const double *, const int *, double *, const int * FCLEN FCLEN FCLEN);
void F77_NAME(dtpmv)(const char *, const char *, const char *,
    const int *, const double *, double *, const int * FCLEN FCLEN FCLEN);
void F77_NAME(dtpsv)(const char *, const char *, const char *,
    const int *, const double *, double *, const int * FCLEN FCLEN FCLEN);
void F77_NAME(dtrmm)(const char *, const char *, const char *, const char *, const int *,
    const int *, const double *, const double *, const int *, double *, const int * FCLEN FCLEN FCLEN FCLEN);
void F77_NAME(dtrmv)(const char *, const char *, const char *, const int *,
    const double *, const int *, double *, const int * FCLEN FCLEN FCLEN);
void F77_NAME(dtrsm)(const char *, const char *, const char *, const char *, const int *,
    const int *, const double *, const double *, const int *, double *, const int * FCLEN FCLEN FCLEN FCLEN);
void F77_NAME(dtrsv)(const char *, const char *, const char *, const int *,
    const double *, const int *, double *, const int * FCLEN FCLEN FCLEN);
double F77_NAME(dzasum)(const int *, const Rcomplex *, const int *);
double F77_NAME(dznrm2)(const int *, const Rcomplex *, const int *);
int F77_NAME(idamax)(const int *, const double *, const int *);
int F77_NAME(izamax)(const int *, const Rcomplex *, const int *);
void F77_NAME(zaxpy)(const int *, const Rcomplex *, const Rcomplex *,
    const int *, Rcomplex *, const int *);
void F77_NAME(zcopy)(const int *, const Rcomplex *, const int *, Rcomplex *, const int *);
Rcomplex F77_NAME(zdotc)(const int *, const Rcomplex *, const int *, const Rcomplex *, const int *);
Rcomplex F77_NAME(zdotu)(const int *, const Rcomplex *, const int *, const Rcomplex *, const int *);
void F77_NAME(zdrot)(const int *, Rcomplex *, const int *,
    Rcomplex *, const int *, const double *, const double *);
void F77_NAME(zdscal)(const int *, const double *, Rcomplex *, const int *);
void F77_NAME(zgbmv)(const char *, const int *, const int *, const int *, const int *, const Rcomplex *,
    const Rcomplex *, const int *, const Rcomplex *, const int *, const Rcomplex *, Rcomplex *, const int * FCLEN);
void F77_NAME(zgemm)(const char *, const char *, const int *, const int *, const int *, const Rcomplex *,
    const Rcomplex *, const int *, const Rcomplex *, const int *, const Rcomplex *, Rcomplex *, const int * FCLEN FCLEN);
void F77_NAME(zgemv)(const char *, const int *, const int *, const Rcomplex *, const Rcomplex *,
    const int *, const Rcomplex *, const int *, const Rcomplex *, Rcomplex *, const int * FCLEN);
void F77_NAME(zgerc)(const int *, const int *, const Rcomplex *, const Rcomplex *,
    const int *, const Rcomplex *, const int *, Rcomplex *, const int *);
void F77_NAME(zgeru)(const int *, const int *, const Rcomplex *, const Rcomplex *,
    const int *, const Rcomplex *, const int *, Rcomplex *, const int *);
void F77_NAME(zhbmv)(const char *, const int *, const int *, const Rcomplex *, const Rcomplex *,
    const int *, const Rcomplex *, const int *, const Rcomplex *, Rcomplex *, const int * FCLEN);
void F77_NAME(zhemm)(const char *, const char *, const int *, const int *, const Rcomplex *, const Rcomplex *,
    const int *, const Rcomplex *, const int *, const Rcomplex *, Rcomplex *, const int * FCLEN FCLEN);
void F77_NAME(zhemv)(const char *, const int *, const Rcomplex *, const Rcomplex *, const int *,
    const Rcomplex *, const int *, const Rcomplex *, Rcomplex *, const int * FCLEN);
void F77_NAME(zher2)(const char *, const int *, const Rcomplex *, const Rcomplex *,
    const int *, const Rcomplex *, const int *, Rcomplex *, const int * FCLEN);
void F77_NAME(zher2k)(const char *, const char *, const int *, const int *, const Rcomplex *, const Rcomplex *,
    const int *, const Rcomplex *, const int *, const double *, Rcomplex *, const int * FCLEN FCLEN);
void F77_NAME(zher)(const char *, const int *, const double *,
    const Rcomplex *, const int *, Rcomplex *, const int * FCLEN);
void F77_NAME(zherk)(const char *, const char *, const int *, const int *, const double *,
    const Rcomplex *, const int *, const double *, Rcomplex *, const int * FCLEN FCLEN);
void F77_NAME(zhpmv)(const char *, const int *, const Rcomplex *, const Rcomplex *,
    const Rcomplex *, const int *, const Rcomplex *, Rcomplex *, const int * FCLEN);
void F77_NAME(zhpr2)(const char *, const int *, const Rcomplex *, const Rcomplex *,
    const int *, const Rcomplex *, const int *, Rcomplex * FCLEN);
void F77_NAME(zhpr)(const char *, const int *, const double *,
    const Rcomplex *, const int *, Rcomplex * FCLEN);
void F77_NAME(zrotg)(Rcomplex *, const Rcomplex *, double *, Rcomplex *);
void F77_NAME(zscal)(const int *, const Rcomplex *, Rcomplex *, const int *);
void F77_NAME(zspmv)(const char *, const int *, const Rcomplex *, const Rcomplex *,
    const Rcomplex *, const int *, const Rcomplex *, Rcomplex *, const int * FCLEN);
void F77_NAME(zswap)(const int *, Rcomplex *, const int *, Rcomplex *, const int *);
void F77_NAME(zsymm)(const char *, const char *, const int *, const int *, const Rcomplex *, const Rcomplex *,
    const int *, const Rcomplex *, const int *, const Rcomplex *, Rcomplex *, const int * FCLEN FCLEN);
void F77_NAME(zsyr2k)(const char *, const char *, const int *, const int *, const Rcomplex *, const Rcomplex *,
    const int *, const Rcomplex *, const int *, const Rcomplex *, Rcomplex *, const int * FCLEN FCLEN);
void F77_NAME(zsyrk)(const char *, const char *, const int *, const int *, const Rcomplex *,
    const Rcomplex *, const int *, const Rcomplex *, Rcomplex *, const int * FCLEN FCLEN);
void F77_NAME(ztbmv)(const char *, const char *, const char *, const int *,
    const int *, const Rcomplex *, const int *, Rcomplex *, const int * FCLEN FCLEN FCLEN);
void F77_NAME(ztbsv)(const char *, const char *, const char *, const int *,
    const int *, const Rcomplex *, const int *, Rcomplex *, const int * FCLEN FCLEN FCLEN);
void F77_NAME(ztpmv)(const char *, const char *, const char *,
    const int *, const Rcomplex *, Rcomplex *, const int * FCLEN FCLEN FCLEN);
void F77_NAME(ztpsv)(const char *, const char *, const char *,
    const int *, const Rcomplex *, Rcomplex *, const int * FCLEN FCLEN FCLEN);
void F77_NAME(ztrmm)(const char *, const char *, const char *, const char *, const int *,
    const int *, const Rcomplex *, const Rcomplex *, const int *, Rcomplex *, const int * FCLEN FCLEN FCLEN FCLEN);
void F77_NAME(ztrmv)(const char *, const char *, const char *, const int *,
    const Rcomplex *, const int *, Rcomplex *, const int * FCLEN FCLEN FCLEN);
void F77_NAME(ztrsm)(const char *, const char *, const char *, const char *, const int *,
    const int *, const Rcomplex *, const Rcomplex *, const int *, Rcomplex *, const int * FCLEN FCLEN FCLEN FCLEN);
void F77_NAME(ztrsv)(const char *, const char *, const char *, const int *,
    const Rcomplex *, const int *, Rcomplex *, const int * FCLEN FCLEN FCLEN);

#ifdef __cplusplus
}
#endif

#endif

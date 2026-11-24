/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_LAPACK_H
#define ROXIDE_R_EXT_LAPACK_H

#include <R_ext/BLAS.h>

#ifdef __cplusplus
extern "C" {
#endif

void F77_NAME(dgbsv)(const int *, const int *, const int *, const int *, double *,
    const int *, int *, double *, const int *, int *);
void F77_NAME(dgbtrf)(const int *, const int *, const int *, const int *,
    double *, const int *, int *, int *);
void F77_NAME(dgbtrs)(const char *, const int *, const int *, const int *, const int *,
    const double *, const int *, const int *, double *, const int *, int * FCLEN);
void F77_NAME(dgebrd)(const int *, const int *, double *, const int *, double *,
    double *, double *, double *, double *, const int *, int *);
void F77_NAME(dgecon)(const char *, const int *, const double *, const int *,
    const double *, double *, double *, int *, int * FCLEN);
void F77_NAME(dgeequ)(const int *, const int *, const double *, const int *, double *,
    double *, double *, double *, double *, int *);
void F77_NAME(dgeev)(const char *, const char *, const int *, double *, const int *, double *, double *,
    double *, const int *, double *, const int *, double *, const int *, int * FCLEN FCLEN);
void F77_NAME(dgehrd)(const int *, const int *, const int *, double *,
    const int *, double *, double *, const int *, int *);
void F77_NAME(dgelqf)(const int *, const int *, double *, const int *,
    double *, double *, const int *, int *);
void F77_NAME(dgels)(const char *, const int *, const int *, const int *, double *,
    const int *, double *, const int *, double *, const int *, int * FCLEN);
void F77_NAME(dgelsd)(const int *, const int *, const int *, double *, const int *, double *, const int *,
    double *, const double *, int *, double *, const int *, int *, int *);
void F77_NAME(dgelss)(const int *, const int *, const int *, double *, const int *, double *,
    const int *, double *, const double *, int *, double *, const int *, int *);
void F77_NAME(dgelsy)(const int *, const int *, const int *, double *, const int *, double *,
    const int *, int *, const double *, int *, double *, const int *, int *);
void F77_NAME(dgeqp3)(const int *, const int *, double *, const int *,
    int *, double *, double *, const int *, int *);
void F77_NAME(dgeqr2)(const int *, const int *, double *, const int *, double *, double *, int *);
void F77_NAME(dgeqrf)(const int *, const int *, double *, const int *,
    double *, double *, const int *, int *);
void F77_NAME(dgerqf)(const int *, const int *, double *, const int *,
    double *, double *, const int *, int *);
void F77_NAME(dgesdd)(const char *, const int *, const int *, double *, const int *, double *, double *,
    const int *, double *, const int *, double *, const int *, int *, int * FCLEN);
void F77_NAME(dgesv)(const int *, const int *, double *, const int *,
    int *, double *, const int *, int *);
void F77_NAME(dgesvd)(const char *, const char *, const int *, const int *, double *, const int *, double *,
    double *, const int *, double *, const int *, double *, const int *, int * FCLEN FCLEN);
void F77_NAME(dgetf2)(const int *, const int *, double *, const int *, int *, int *);
void F77_NAME(dgetrf)(const int *, const int *, double *, const int *, int *, int *);
void F77_NAME(dgetri)(const int *, double *, const int *, const int *, double *, const int *, int *);
void F77_NAME(dgetrs)(const char *, const int *, const int *, const double *,
    const int *, const int *, double *, const int *, int * FCLEN);
void F77_NAME(dggev)(const char *, const char *, const int *, double *, const int *, double *, const int *, double *,
    double *, double *, double *, const int *, double *, const int *, double *, const int *, int * FCLEN FCLEN);
void F77_NAME(dgtsv)(const int *, const int *, double *, double *,
    double *, double *, const int *, int *);
void F77_NAME(dlacpy)(const char *, const int *, const int *,
    const double *, const int *, double *, const int * FCLEN);
double F77_NAME(dlamch)(const char * FCLEN);
double F77_NAME(dlange)(const char *, const int *, const int *,
    const double *, const int *, double * FCLEN);
double F77_NAME(dlansy)(const char *, const char *, const int *,
    const double *, const int *, double * FCLEN FCLEN);
double F77_NAME(dlapy2)(const double *, const double *);
double F77_NAME(dlapy3)(const double *, const double *, const double *);
void F77_NAME(dlarfg)(const int *, double *, double *, const int *, double *);
void F77_NAME(dlascl)(const char *, const int *, const int *, const double *, const double *,
    const int *, const int *, double *, const int *, int * FCLEN);
void F77_NAME(dlaset)(const char *, const int *, const int *,
    const double *, const double *, double *, const int * FCLEN);
void F77_NAME(dlasrt)(const char *, const int *, double *, int * FCLEN);
void F77_NAME(dlaswp)(const int *, double *, const int *,
    const int *, const int *, const int *, const int *);
void F77_NAME(dorgbr)(const char *, const int *, const int *, const int *, double *,
    const int *, const double *, double *, const int *, int * FCLEN);
void F77_NAME(dorglq)(const int *, const int *, const int *, double *,
    const int *, const double *, double *, const int *, int *);
void F77_NAME(dorgqr)(const int *, const int *, const int *, double *,
    const int *, const double *, double *, const int *, int *);
void F77_NAME(dormlq)(const char *, const char *, const int *, const int *, const int *, double *,
    const int *, const double *, double *, const int *, double *, const int *, int * FCLEN FCLEN);
void F77_NAME(dormqr)(const char *, const char *, const int *, const int *, const int *, double *,
    const int *, const double *, double *, const int *, double *, const int *, int * FCLEN FCLEN);
void F77_NAME(dpbsv)(const char *, const int *, const int *, const int *,
    double *, const int *, double *, const int *, int * FCLEN);
void F77_NAME(dpocon)(const char *, const int *, const double *, const int *,
    const double *, double *, double *, int *, int * FCLEN);
void F77_NAME(dposv)(const char *, const int *, const int *, double *,
    const int *, double *, const int *, int * FCLEN);
void F77_NAME(dpotf2)(const char *, const int *, double *, const int *, int * FCLEN);
void F77_NAME(dpotrf2)(const char *, const int *, double *, const int *, int * FCLEN);
void F77_NAME(dpotrf)(const char *, const int *, double *, const int *, int * FCLEN);
void F77_NAME(dpotri)(const char *, const int *, double *, const int *, int * FCLEN);
void F77_NAME(dpotrs)(const char *, const int *, const int *, const double *,
    const int *, double *, const int *, int * FCLEN);
void F77_NAME(dppsv)(const char *, const int *, const int *,
    double *, double *, const int *, int * FCLEN);
void F77_NAME(dpstrf)(const char *, const int *, double *, const int *,
    int *, int *, const double *, double *, int * FCLEN);
void F77_NAME(dptsv)(const int *, const int *, double *, double *, double *, const int *, int *);
void F77_NAME(dspev)(const char *, const char *, const int *, double *,
    double *, double *, const int *, double *, int * FCLEN FCLEN);
void F77_NAME(dsyequb)(const char *, const int *, const double *, const int *,
    double *, double *, double *, double *, int * FCLEN);
void F77_NAME(dsyev)(const char *, const char *, const int *, double *,
    const int *, double *, double *, const int *, int * FCLEN FCLEN);
void F77_NAME(dsyevd)(const char *, const char *, const int *, double *, const int *,
    double *, double *, const int *, int *, const int *, int * FCLEN FCLEN);
void F77_NAME(dsyevr)(const char *, const char *, const char *, const int *, double *, const int *, const double *, const double *, const int *, const int *,
    const double *, int *, double *, double *, const int *, int *, double *, const int *, int *, const int *, int * FCLEN FCLEN FCLEN);
void F77_NAME(dsyevx)(const char *, const char *, const char *, const int *, double *, const int *, const double *, const double *, const int *, const int *,
    const double *, int *, double *, double *, const int *, double *, const int *, int *, int *, int * FCLEN FCLEN FCLEN);
void F77_NAME(dsygv)(const int *, const char *, const char *, const int *, double *, const int *,
    double *, const int *, double *, double *, const int *, int * FCLEN FCLEN);
void F77_NAME(dsygvd)(const int *, const char *, const char *, const int *, double *, const int *, double *,
    const int *, double *, double *, const int *, int *, const int *, int * FCLEN FCLEN);
void F77_NAME(dsysv)(const char *, const int *, const int *, double *, const int *,
    int *, double *, const int *, double *, const int *, int * FCLEN);
void F77_NAME(dsytrd)(const char *, const int *, double *, const int *, double *,
    double *, double *, double *, const int *, int * FCLEN);
void F77_NAME(dsytrf)(const char *, const int *, double *, const int *,
    int *, double *, const int *, int * FCLEN);
void F77_NAME(dsytri)(const char *, const int *, double *,
    const int *, const int *, double *, int * FCLEN);
void F77_NAME(dsytrs)(const char *, const int *, const int *, const double *,
    const int *, const int *, double *, const int *, int * FCLEN);
void F77_NAME(dtrcon)(const char *, const char *, const char *, const int *, const double *,
    const int *, double *, double *, int *, int * FCLEN FCLEN FCLEN);
void F77_NAME(dtrtri)(const char *, const char *, const int *,
    double *, const int *, int * FCLEN FCLEN);
void F77_NAME(dtrtrs)(const char *, const char *, const char *, const int *, const int *,
    const double *, const int *, double *, const int *, int * FCLEN FCLEN FCLEN);
int F77_NAME(ilaenv)(const int *, const char *, const char *,
    const int *, const int *, const int *, const int * FCLEN FCLEN);
void F77_NAME(ilaver)(int *, int *, int *);
int F77_NAME(lsame)(const char *, const char * FCLEN FCLEN);
void F77_NAME(zgebrd)(const int *, const int *, Rcomplex *, const int *, double *,
    double *, Rcomplex *, Rcomplex *, Rcomplex *, const int *, int *);
void F77_NAME(zgecon)(const char *, const int *, const Rcomplex *, const int *,
    const double *, double *, Rcomplex *, double *, int * FCLEN);
void F77_NAME(zgeev)(const char *, const char *, const int *, Rcomplex *, const int *, Rcomplex *, Rcomplex *,
    const int *, Rcomplex *, const int *, Rcomplex *, const int *, double *, int * FCLEN FCLEN);
void F77_NAME(zgehrd)(const int *, const int *, const int *, Rcomplex *,
    const int *, Rcomplex *, Rcomplex *, const int *, int *);
void F77_NAME(zgelqf)(const int *, const int *, Rcomplex *, const int *,
    Rcomplex *, Rcomplex *, const int *, int *);
void F77_NAME(zgels)(const char *, const int *, const int *, const int *, Rcomplex *,
    const int *, Rcomplex *, const int *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(zgelsd)(const int *, const int *, const int *, Rcomplex *, const int *, Rcomplex *, const int *,
    double *, const double *, int *, Rcomplex *, const int *, double *, int *, int *);
void F77_NAME(zgeqp3)(const int *, const int *, Rcomplex *, const int *, int *,
    Rcomplex *, Rcomplex *, const int *, double *, int *);
void F77_NAME(zgeqr2)(const int *, const int *, Rcomplex *,
    const int *, Rcomplex *, Rcomplex *, int *);
void F77_NAME(zgeqrf)(const int *, const int *, Rcomplex *, const int *,
    Rcomplex *, Rcomplex *, const int *, int *);
void F77_NAME(zgesdd)(const char *, const int *, const int *, Rcomplex *, const int *, double *, Rcomplex *,
    const int *, Rcomplex *, const int *, Rcomplex *, const int *, double *, int *, int * FCLEN);
void F77_NAME(zgesv)(const int *, const int *, Rcomplex *, const int *,
    int *, Rcomplex *, const int *, int *);
void F77_NAME(zgesvd)(const char *, const char *, const int *, const int *, Rcomplex *, const int *, double *,
    Rcomplex *, const int *, Rcomplex *, const int *, Rcomplex *, const int *, double *, int * FCLEN FCLEN);
void F77_NAME(zgetrf2)(const int *, const int *, Rcomplex *, const int *, int *, int *);
void F77_NAME(zgetrf)(const int *, const int *, Rcomplex *, const int *, int *, int *);
void F77_NAME(zgetri)(const int *, Rcomplex *, const int *,
    const int *, Rcomplex *, const int *, int *);
void F77_NAME(zgetrs)(const char *, const int *, const int *, const Rcomplex *,
    const int *, const int *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(zheev)(const char *, const char *, const int *, Rcomplex *, const int *,
    double *, Rcomplex *, const int *, double *, int * FCLEN FCLEN);
void F77_NAME(zheevd)(const char *, const char *, const int *, Rcomplex *, const int *, double *,
    Rcomplex *, const int *, double *, const int *, int *, const int *, int * FCLEN FCLEN);
void F77_NAME(zhetrd)(const char *, const int *, Rcomplex *, const int *, double *,
    double *, Rcomplex *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(zhetrf)(const char *, const int *, Rcomplex *, const int *,
    int *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(zhetri)(const char *, const int *, Rcomplex *,
    const int *, const int *, Rcomplex *, int * FCLEN);
void F77_NAME(zhetrs)(const char *, const int *, const int *, const Rcomplex *,
    const int *, const int *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(zlacpy)(const char *, const int *, const int *,
    const Rcomplex *, const int *, Rcomplex *, const int * FCLEN);
double F77_NAME(zlange)(const char *, const int *, const int *,
    const Rcomplex *, const int *, double * FCLEN);
double F77_NAME(zlanhe)(const char *, const char *, const int *,
    const Rcomplex *, const int *, double * FCLEN FCLEN);
void F77_NAME(zlarfg)(const int *, Rcomplex *, Rcomplex *, const int *, Rcomplex *);
void F77_NAME(zlaset)(const char *, const int *, const int *,
    const Rcomplex *, const Rcomplex *, Rcomplex *, const int * FCLEN);
void F77_NAME(zlaswp)(const int *, Rcomplex *, const int *,
    const int *, const int *, const int *, const int *);
void F77_NAME(zpocon)(const char *, const int *, const Rcomplex *, const int *,
    const double *, double *, Rcomplex *, double *, int * FCLEN);
void F77_NAME(zpotrf2)(const char *, const int *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(zpotrf)(const char *, const int *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(zpotri)(const char *, const int *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(zpotrs)(const char *, const int *, const int *, const Rcomplex *,
    const int *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(zpstrf)(const char *, const int *, Rcomplex *, const int *,
    int *, int *, const double *, double *, int * FCLEN);
void F77_NAME(zsytrf)(const char *, const int *, Rcomplex *, const int *,
    int *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(zsytri)(const char *, const int *, Rcomplex *,
    const int *, const int *, Rcomplex *, int * FCLEN);
void F77_NAME(zsytrs)(const char *, const int *, const int *, const Rcomplex *,
    const int *, const int *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(ztrtri)(const char *, const char *, const int *,
    Rcomplex *, const int *, int * FCLEN FCLEN);
void F77_NAME(ztrtrs)(const char *, const char *, const char *, const int *, const int *,
    const Rcomplex *, const int *, Rcomplex *, const int *, int * FCLEN FCLEN FCLEN);
void F77_NAME(zungbr)(const char *, const int *, const int *, const int *, Rcomplex *,
    const int *, const Rcomplex *, Rcomplex *, const int *, int * FCLEN);
void F77_NAME(zunglq)(const int *, const int *, const int *, Rcomplex *,
    const int *, const Rcomplex *, Rcomplex *, const int *, int *);
void F77_NAME(zungqr)(const int *, const int *, const int *, Rcomplex *,
    const int *, const Rcomplex *, Rcomplex *, const int *, int *);
void F77_NAME(zunmlq)(const char *, const char *, const int *, const int *, const int *, const Rcomplex *,
    const int *, const Rcomplex *, Rcomplex *, const int *, Rcomplex *, const int *, int * FCLEN FCLEN);
void F77_NAME(zunmqr)(const char *, const char *, const int *, const int *, const int *, const Rcomplex *,
    const int *, const Rcomplex *, Rcomplex *, const int *, Rcomplex *, const int *, int * FCLEN FCLEN);

typedef int (*R_LapackDSelect2)(const double *, const double *);
void F77_NAME(dgees)(const char *job_vectors, const char *sort,
    R_LapackDSelect2 select, const int *n, double *a, const int *lda,
    int *selected, double *wr, double *wi, double *vs, const int *ldvs,
    double *work, const int *lwork, int *logical_work, int *info FCLEN FCLEN);

#ifdef __cplusplus
}
#endif

#endif

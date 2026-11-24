/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_LINPACK_H
#define ROXIDE_R_EXT_LINPACK_H

#include <R_ext/RS.h>

#ifdef __cplusplus
extern "C" {
#endif

void F77_NAME(dchdc)(double *, const int *, const int *, double *, int *, const int *, int *);
void F77_NAME(dchdd)(double *, const int *, const int *, const double *, double *, const int *,
    const int *, const double *, double *, double *, double *, int *);
void F77_NAME(dchex)(double *, const int *, const int *, const int *, const int *,
    double *, const int *, const int *, double *, double *, const int *);
void F77_NAME(dchud)(double *, const int *, const int *, const double *, double *,
    const int *, const int *, const double *, double *, double *, double *);
void F77_NAME(dgbco)(double *, const int *, const int *, const int *,
    const int *, int *, double *, double *);
void F77_NAME(dgbdi)(const double *, const int *, const int *,
    const int *, const int *, const double *, double *);
void F77_NAME(dgbfa)(double *, const int *, const int *, const int *, const int *, int *, int *);
void F77_NAME(dgbsl)(const double *, const int *, const int *, const int *,
    const int *, const int *, double *, const int *);
void F77_NAME(dgeco)(double *, const int *, const int *, int *, double *, double *);
void F77_NAME(dgedi)(double *, const int *, const int *,
    const int *, double *, double *, const int *);
void F77_NAME(dgefa)(double *, const int *, const int *, int *, int *);
void F77_NAME(dgesl)(const double *, const int *, const int *, const int *, double *, const int *);
void F77_NAME(dgtsl)(const int *, double *, double *, double *, double *, int *);
void F77_NAME(dpbco)(double *, const int *, const int *, const int *, double *, double *, int *);
void F77_NAME(dpbdi)(const double *, const int *, const int *, const int *, double *);
void F77_NAME(dpbfa)(double *, const int *, const int *, const int *, int *);
void F77_NAME(dpbsl)(const double *, const int *, const int *, const int *, double *);
void F77_NAME(dpoco)(double *, const int *, const int *, double *, double *, int *);
void F77_NAME(dpodi)(double *, const int *, const int *, double *, const int *);
void F77_NAME(dpofa)(double *, const int *, const int *, int *);
void F77_NAME(dposl)(const double *, const int *, const int *, double *);
void F77_NAME(dppco)(double *, const int *, double *, double *, int *);
void F77_NAME(dppdi)(double *, const int *, double *, const int *);
void F77_NAME(dppfa)(double *, const int *, int *);
void F77_NAME(dppsl)(const double *, const int *, double *);
void F77_NAME(dptsl)(const int *, double *, double *, double *);
void F77_NAME(dqrdc)(double *, const int *, const int *, const int *,
    double *, int *, double *, const int *);
void F77_NAME(dqrsl)(const double *, const int *, const int *, const int *, const double *, const double *,
    double *, double *, double *, double *, double *, const int *, int *);
void F77_NAME(dsico)(double *, const int *, const int *, int *, double *, double *);
void F77_NAME(dsidi)(double *, const int *, const int *, const int *,
    double *, int *, double *, const int *);
void F77_NAME(dsifa)(double *, const int *, const int *, int *, int *);
void F77_NAME(dsisl)(const double *, const int *, const int *, const int *, double *);
void F77_NAME(dspco)(double *, const int *, int *, double *, double *);
void F77_NAME(dspdi)(double *, const int *, const int *, double *, int *, double *, const int *);
void F77_NAME(dspfa)(double *, const int *, int *, int *);
void F77_NAME(dspsl)(const double *, const int *, const int *, double *);
void F77_NAME(dsvdc)(double *, const int *, const int *, const int *, double *, double *,
    double *, const int *, double *, const int *, double *, const int *, int *);
void F77_NAME(dtrco)(const double *, const int *, const int *, double *, double *, const int *);
void F77_NAME(dtrsl)(const double *, const int *, const int *, double *, const int *, int *);

#ifdef __cplusplus
}
#endif

#endif

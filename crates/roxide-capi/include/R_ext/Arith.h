/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_ARITH_H
#define ROXIDE_R_EXT_ARITH_H

#include <math.h>
#include <R_ext/libextern.h>

#ifdef __cplusplus
extern "C" {
#endif

LibExtern double R_NaN;
LibExtern double R_PosInf;
LibExtern double R_NegInf;
LibExtern double R_NaReal;
LibExtern int R_NaInt;

#define NA_LOGICAL R_NaInt
#define NA_INTEGER R_NaInt
#define NA_REAL R_NaReal

int R_IsNA(double x);
int R_IsNaN(double x);
int R_finite(double x);
int R_isnancpp(double x);

#define ISNA(x) R_IsNA(x)
#ifdef __cplusplus
#define ISNAN(x) R_isnancpp(x)
#else
#define ISNAN(x) isnan(x)
#endif
#define R_FINITE(x) R_finite(x)

double R_pow(double x, double y);
double R_pow_di(double x, int n);

#ifdef __cplusplus
}
#endif

#endif

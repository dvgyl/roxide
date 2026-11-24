/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_UTILS_H
#define ROXIDE_R_EXT_UTILS_H

#include <stddef.h>
#include <R_ext/Boolean.h>
#include <R_ext/Complex.h>
#include <R_ext/RS.h>

#ifdef __cplusplus
extern "C" {
#endif

void R_CheckStack(void);
void R_CheckStack2(size_t extra);
void R_CheckUserInterrupt(void);
const char *R_ExpandFileName(const char *path);
void R_ShowMessage(const char *message);

void R_csort(Rcomplex *x, int n);
void R_isort(int *x, int n);
void R_rsort(double *x, int n);
void Rf_cPsort(Rcomplex *x, int n, int k);
void Rf_iPsort(int *x, int n, int k);
void Rf_rPsort(double *x, int n, int k);
void Rf_revsort(double *x, int *index, int n);
void rsort_with_index(double *x, int *index, int n);
void R_qsort(double *x, size_t first, size_t last);
void R_qsort_I(double *x, int *index, int first, int last);
void R_qsort_int(int *x, size_t first, size_t last);
void R_qsort_int_I(int *x, int *index, int first, int last);
int findInterval(double *xt, int n, double x, Rboolean rightmost_closed,
                 Rboolean all_inside, int ilo, int *mflag);
int findInterval2(double *xt, int n, double x, Rboolean rightmost_closed,
                  Rboolean all_inside, Rboolean left_open, int ilo, int *mflag);

char *R_tmpnam(const char *prefix, const char *directory);
char *R_tmpnam2(const char *prefix, const char *directory, const char *extension);
void R_free_tmpnam(char *name);
double R_atof(const char *text);
double R_strtod(const char *text, char **end);
const char *acopy_string(const char *text);
void R_max_col(double *matrix, int *rows, int *columns, int *maxima, int *ties);
void F77_NAME(qsort3)(double *x, int *first, int *last);
void F77_NAME(qsort4)(double *x, int *index, int *first, int *last);
int F77_NAME(interv)(double *x, int *n, double *value, int *rightmost_closed,
                     int *all_inside, int *index, int *flag);
double F77_NAME(d1mach)(const int *index);
int F77_NAME(i1mach)(const int *index);
void F77_NAME(rchkusr)(void);

#if !defined(R_NO_REMAP) && !defined(__cplusplus)
#define cPsort Rf_cPsort
#define iPsort Rf_iPsort
#define rPsort Rf_rPsort
#define revsort Rf_revsort
#endif

#ifdef __cplusplus
}
#endif

#endif

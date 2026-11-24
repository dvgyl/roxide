/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_PRINT_H
#define ROXIDE_R_EXT_PRINT_H

#include <stdarg.h>
#include <R_ext/Error.h>
#include <R_ext/RS.h>

#ifdef __cplusplus
extern "C" {
#endif

void Rprintf(const char *format, ...) ROXIDE_PRINTF(1, 2);
void REprintf(const char *format, ...) ROXIDE_PRINTF(1, 2);
void Rvprintf(const char *format, va_list args) ROXIDE_PRINTF(1, 0);
void REvprintf(const char *format, va_list args) ROXIDE_PRINTF(1, 0);

void F77_NAME(labelpr)(const char *label, const int *length FCLEN);
void F77_NAME(dblepr)(const char *label, const int *length,
                      const double *data, const int *count FCLEN);
void F77_NAME(realpr)(const char *label, const int *length,
                      const float *data, const int *count FCLEN);
void F77_NAME(intpr)(const char *label, const int *length,
                     const int *data, const int *count FCLEN);
void F77_NAME(dblepr1)(const char *label, const int *length, const double *value FCLEN);
void F77_NAME(realpr1)(const char *label, const int *length, const float *value FCLEN);
void F77_NAME(intpr1)(const char *label, const int *length, const int *value FCLEN);

#ifdef __cplusplus
}
#endif

#endif

/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_ERROR_H
#define ROXIDE_R_EXT_ERROR_H

#include <R_ext/RS.h>

#ifdef __cplusplus
extern "C" {
#endif

#if defined(__cplusplus)
#define NORET [[noreturn]]
#else
#define NORET _Noreturn
#endif
#if defined(__GNUC__)
#define ROXIDE_PRINTF(position, first) __attribute__((format(printf, position, first)))
#else
#define ROXIDE_PRINTF(position, first)
#endif

NORET void UNIMPLEMENTED(const char *msg);
NORET void Rf_error(const char *format, ...) ROXIDE_PRINTF(1, 2);
void Rf_warning(const char *format, ...) ROXIDE_PRINTF(1, 2);
const char *R_curErrorBuf(void);
NORET void F77_NAME(rexit)(const char *message FCLEN);
void F77_NAME(rwarn)(const char *message FCLEN);

#if !defined(R_NO_REMAP) && !defined(__cplusplus)
#define error Rf_error
#define warning Rf_warning
#endif

#ifdef __cplusplus
}
#endif

#endif

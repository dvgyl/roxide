/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_RS_H
#define ROXIDE_R_EXT_RS_H

#include <stddef.h>
#include <string.h>
#include <Rconfig.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifndef F77_CALL
#define F77_CALL(name) name ## _
#endif
#define F77_NAME(name) F77_CALL(name)
#define F77_SUB(name) F77_CALL(name)

#if defined(USE_FC_LEN_T) && defined(FC_LEN_T)
#define FCLEN , FC_LEN_T
#define FCONE , (FC_LEN_T)1
#else
#define FCLEN
#define FCONE
#endif

void *R_chk_calloc(size_t count, size_t size);
void *R_chk_realloc(void *ptr, size_t size);
void R_chk_free(void *ptr);
void *R_chk_memcpy(void *target, const void *source, size_t size);
void *R_chk_memset(void *target, int value, size_t size);

#define R_Calloc(n, type) ((type *)R_chk_calloc((n), sizeof(type)))
#define R_Realloc(ptr, n, type) ((type *)R_chk_realloc((ptr), (n) * sizeof(type)))
#define R_Free(ptr) ((void)(R_chk_free(ptr), (ptr) = NULL))
#define CallocCharBuf(n) R_Calloc((n) + 1, char)
#define Memcpy(target, source, n) R_chk_memcpy((target), (source), (n) * sizeof(*(source)))
#define Memzero(target, n) R_chk_memset((target), 0, (n) * sizeof(*(target)))

#if !defined(STRICT_R_HEADERS) && !defined(__cplusplus)
#define Calloc R_Calloc
#define Realloc R_Realloc
#define Free R_Free
#endif

#ifdef __cplusplus
}
#endif

#endif

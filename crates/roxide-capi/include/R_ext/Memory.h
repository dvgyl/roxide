/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_MEMORY_H
#define ROXIDE_R_EXT_MEMORY_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

void *R_calloc_gc(size_t count, size_t size);
void *R_malloc_gc(size_t size);
void *R_realloc_gc(void *ptr, size_t size);

void *vmaxget(void);
void vmaxset(const void *mark);
char *R_alloc(size_t count, int size);
long double *R_allocLD(size_t count);
char *S_alloc(long count, int size);
char *S_realloc(char *ptr, long count, long old_count, int size);

#ifdef __cplusplus
}
#endif

#endif

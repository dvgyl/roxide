/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_RALLOCATORS_H
#define ROXIDE_R_EXT_RALLOCATORS_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct R_allocator R_allocator_t;
typedef void *(*R_allocator_alloc_t)(R_allocator_t *, size_t);
typedef void (*R_allocator_free_t)(R_allocator_t *, void *);
struct R_allocator {
    R_allocator_alloc_t mem_alloc;
    R_allocator_free_t mem_free;
    void *res;
    void *data;
};

#ifdef __cplusplus
}
#endif

#endif

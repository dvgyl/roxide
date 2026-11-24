/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_ITERMACROS_H
#define ROXIDE_R_EXT_ITERMACROS_H

#include <Rinternals.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Read a vector in blocks. The caller supplies the loop body. */
#define ITERATE_BY_REGION(vector, region, i, n, type, stem, body) \
    do { \
        SEXP roxide_vector = (vector); \
        R_xlen_t roxide_end = XLENGTH(roxide_vector); \
        type region[512]; \
        for (R_xlen_t i = 0; i < roxide_end;) { \
            R_xlen_t n = roxide_end - i; \
            if (n > 512) n = 512; \
            n = stem ## _GET_REGION(roxide_vector, i, n, region); \
            if (n <= 0) break; \
            body \
            i += n; \
        } \
    } while (0)

#ifdef __cplusplus
}
#endif

#endif

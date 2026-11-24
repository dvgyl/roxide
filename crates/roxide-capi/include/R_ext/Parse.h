/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_PARSE_H
#define ROXIDE_R_EXT_PARSE_H

#include <Rinternals.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    PARSE_NULL = 0,
    PARSE_OK = 1,
    PARSE_INCOMPLETE = 2,
    PARSE_ERROR = 3,
    PARSE_EOF = 4
} ParseStatus;

SEXP R_ParseVector(SEXP text, int count, ParseStatus *status, SEXP source_file);
SEXP R_ParseString(const char *text);
SEXP R_ParseEvalString(const char *text, SEXP rho);

#ifdef __cplusplus
}
#endif

#endif

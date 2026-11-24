/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_BOOLEAN_H
#define ROXIDE_R_EXT_BOOLEAN_H

#ifdef __cplusplus
extern "C" {
#endif

#if !defined(R_INCLUDE_BOOLEAN_H) || R_INCLUDE_BOOLEAN_H
#ifndef __cplusplus
#include <stdbool.h>
#endif
#ifdef FALSE
#undef FALSE
#endif
#ifdef TRUE
#undef TRUE
#endif
typedef enum { FALSE = 0, TRUE = 1 } Rboolean;
#endif

#ifdef __cplusplus
}
#endif

#endif

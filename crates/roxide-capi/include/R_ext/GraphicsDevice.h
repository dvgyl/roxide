/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_GRAPHICS_DEVICE_H
#define ROXIDE_R_EXT_GRAPHICS_DEVICE_H

#include <stddef.h>
#include <R_ext/Boolean.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _DevDesc DevDesc;
typedef DevDesc *pDevDesc;

size_t Rf_ucstoutf8(char *s, const unsigned int c);

#ifndef R_NO_REMAP
#define ucstoutf8 Rf_ucstoutf8
#endif

#ifdef __cplusplus
}
#endif

#endif

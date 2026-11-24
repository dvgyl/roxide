/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_INTERFACE_H
#define ROXIDE_R_INTERFACE_H

#include <R_ext/Boolean.h>
#include <R_ext/libextern.h>

#ifdef __cplusplus
extern "C" {
#endif

extern void Rf_onintr(void);
LibExtern Rboolean R_Interactive;

#ifndef R_NO_REMAP
#define onintr Rf_onintr
#endif

#ifdef __cplusplus
}
#endif

#endif

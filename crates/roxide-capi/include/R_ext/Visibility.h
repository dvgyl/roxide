/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_VISIBILITY_H
#define ROXIDE_R_EXT_VISIBILITY_H

#include <Rconfig.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef HAVE_VISIBILITY_ATTRIBUTE
#define attribute_visible __attribute__((visibility("default")))
#define attribute_hidden __attribute__((visibility("hidden")))
#else
#define attribute_visible
#define attribute_hidden
#endif

#ifdef __cplusplus
}
#endif

#endif

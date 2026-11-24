/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_GRAPHICS_ENGINE_H
#define ROXIDE_R_EXT_GRAPHICS_ENGINE_H

#include <R_ext/GraphicsDevice.h>

#ifdef __cplusplus
extern "C" {
#endif

#define R_GE_version 17
void R_GE_checkVersionOrDie(int version);

#ifdef __cplusplus
}
#endif

#endif

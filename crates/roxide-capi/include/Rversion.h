/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_RVERSION_H
#define ROXIDE_RVERSION_H

#ifdef __cplusplus
extern "C" {
#endif

#define R_Version(major, minor, patch) ((major) * 65536 + (minor) * 256 + (patch))
#define R_VERSION R_Version(4, 6, 2)
#define R_MAJOR "4"
#define R_MINOR "6.2"
#define R_STATUS ""

#ifdef __cplusplus
}
#endif

#endif

/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_RCONFIG_H
#define ROXIDE_RCONFIG_H

#ifdef __cplusplus
extern "C" {
#endif

#define SIZEOF_SIZE_T 8
#define USING_R 1
#define HAVE_F77_UNDERSCORE 1
#define HAVE_VISIBILITY_ATTRIBUTE 1
#define HAVE_FC_LEN_T 1
#define FC_LEN_T size_t
#define IEEE_754 1
#define R_INLINE inline

#if defined(__BYTE_ORDER__) && __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
#define WORDS_BIGENDIAN 1
#endif

#ifdef __cplusplus
}
#endif

#endif

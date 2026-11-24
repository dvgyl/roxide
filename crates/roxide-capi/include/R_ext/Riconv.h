/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_RICONV_H
#define ROXIDE_R_EXT_RICONV_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

void *Riconv_open(const char *to, const char *from);
size_t Riconv(void *descriptor, const char **input, size_t *input_left,
              char **output, size_t *output_left);
int Riconv_close(void *descriptor);

#ifdef __cplusplus
}
#endif

#endif

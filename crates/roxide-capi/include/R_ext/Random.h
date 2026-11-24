/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_RANDOM_H
#define ROXIDE_R_EXT_RANDOM_H

#ifdef __cplusplus
extern "C" {
#endif

void GetRNGstate(void);
void PutRNGstate(void);
double unif_rand(void);
double norm_rand(void);
double exp_rand(void);
double R_unif_index(double n);

#ifdef __cplusplus
}
#endif

#endif

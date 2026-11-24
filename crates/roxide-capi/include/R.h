/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_H
#define ROXIDE_R_H

#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <Rconfig.h>
#include <R_ext/Arith.h>
#include <R_ext/Boolean.h>
#include <R_ext/Complex.h>
#include <R_ext/Constants.h>
#include <R_ext/Error.h>
#include <R_ext/Memory.h>
#include <R_ext/Print.h>
#include <R_ext/Random.h>
#include <R_ext/RS.h>
#include <R_ext/Utils.h>

#ifdef __cplusplus
extern "C" {
#endif

#if defined(__cplusplus) && !defined(R_NO_REMAP)
#define R_NO_REMAP
#endif
#define R_PROBLEM_BUFSIZE 4096

#ifdef __cplusplus
}
#endif

#endif

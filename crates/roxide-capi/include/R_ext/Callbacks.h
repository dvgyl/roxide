/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_CALLBACKS_H
#define ROXIDE_R_EXT_CALLBACKS_H

#include <Rinternals.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef Rboolean (*R_ToplevelCallback)(SEXP expression, SEXP value,
                                       Rboolean succeeded, Rboolean visible,
                                       void *data);
typedef struct _Rf_ToplevelCallbackEl R_ToplevelCallbackEl;

R_ToplevelCallbackEl *Rf_addTaskCallback(R_ToplevelCallback callback,
                                         void *data, void (*finalizer)(void *),
                                         const char *name, int *position);
Rboolean Rf_removeTaskCallbackByIndex(int position);
Rboolean Rf_removeTaskCallbackByName(const char *name);

#ifdef __cplusplus
}
#endif

#endif

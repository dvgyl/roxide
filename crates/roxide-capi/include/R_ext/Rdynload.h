/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_RDYNLOAD_H
#define ROXIDE_R_EXT_RDYNLOAD_H

#include <R_ext/Boolean.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void *(*DL_FUNC)(void);
typedef unsigned int R_NativePrimitiveArgType;
#define SINGLESXP 302
typedef struct _DllInfo DllInfo;
typedef struct Rf_RegisteredNativeSymbol R_RegisteredNativeSymbol;

typedef struct {
    const char *name;
    DL_FUNC fun;
    int numArgs;
    R_NativePrimitiveArgType *types;
} R_CMethodDef;
typedef R_CMethodDef R_FortranMethodDef;

typedef struct {
    const char *name;
    DL_FUNC fun;
    int numArgs;
} R_CallMethodDef;
typedef R_CallMethodDef R_ExternalMethodDef;

int R_registerRoutines(DllInfo *info, const R_CMethodDef *c,
                       const R_CallMethodDef *call,
                       const R_FortranMethodDef *fortran,
                       const R_ExternalMethodDef *external);
Rboolean R_useDynamicSymbols(DllInfo *info, Rboolean enabled);
Rboolean R_forceSymbols(DllInfo *info, Rboolean enabled);
DL_FUNC R_FindSymbol(const char *name, const char *package,
                    R_RegisteredNativeSymbol *symbol);
DllInfo *R_getDllInfo(const char *name);
DllInfo *R_getEmbeddingDllInfo(void);
void R_RegisterCCallable(const char *package, const char *name, DL_FUNC fun);
DL_FUNC R_GetCCallable(const char *package, const char *name);

#ifdef __cplusplus
}
#endif

#endif

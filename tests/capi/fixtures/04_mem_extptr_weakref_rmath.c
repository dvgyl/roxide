/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#define R_NO_REMAP
#define R_NO_REMAP_RMATH
#include <R.h>
#include <Rinternals.h>
#include <Rmath.h>

extern void R_RunPendingFinalizers(void);

static int finalized = 0;
static int exit_token = 0;

static void finalize(SEXP pointer)
{
    if (R_ExternalPtrAddr(pointer) == &exit_token)
        Rprintf("Exit finalizer ran.\n");
    ++finalized;
    R_ClearExternalPtr(pointer);
}

SEXP fixture_memory(void)
{
    void *mark = vmaxget();
    int *scratch = (int *)R_alloc(3, sizeof(*scratch));
    scratch[0] = 23;
    scratch[2] = 19;
    int sum = scratch[0] + scratch[2];
    vmaxset(mark);

    int *heap = R_Calloc(2, int);
    int zeroed = heap[0] == 0 && heap[1] == 0;
    heap[0] = sum;
    heap = R_Realloc(heap, 4, int);
    int kept = heap[0] == sum;
    heap[3] = 71;
    int end = heap[3];
    R_Free(heap);
    if (!zeroed || !kept || heap != NULL || end != 71)
        Rf_error("Native allocation check failed.");
    return Rf_ScalarInteger(sum);
}

SEXP fixture_pointer(SEXP value, SEXP on_exit)
{
    SEXP tag = Rf_install("fixture_pointer_tag");
    void *address = LOGICAL(on_exit)[0] ? (void *)&exit_token : (void *)&finalized;
    SEXP pointer = PROTECT(R_MakeExternalPtr(address, tag, value));
    if (R_ExternalPtrAddr(pointer) != address || R_ExternalPtrTag(pointer) != tag ||
        R_ExternalPtrProtected(pointer) != value)
        Rf_error("External pointer field check failed.");
    R_RegisterCFinalizerEx(pointer, finalize, LOGICAL(on_exit)[0] ? TRUE : FALSE);
    UNPROTECT(1);
    return pointer;
}

SEXP fixture_weak(SEXP key, SEXP value)
{
    return R_MakeWeakRef(key, value, R_NilValue, FALSE);
}

SEXP fixture_weak_fields(SEXP reference)
{
    SEXP result = PROTECT(Rf_allocVector(VECSXP, 2));
    SET_VECTOR_ELT(result, 0, R_WeakRefKey(reference));
    SET_VECTOR_ELT(result, 1, R_WeakRefValue(reference));
    UNPROTECT(1);
    return result;
}

SEXP fixture_preserve(SEXP value)
{
    R_PreserveObject(value);
    return R_NilValue;
}

SEXP fixture_release(SEXP value)
{
    R_ReleaseObject(value);
    return R_NilValue;
}

SEXP fixture_pending(void)
{
    R_RunPendingFinalizers();
    return Rf_ScalarInteger(finalized);
}

SEXP fixture_random(void)
{
    SEXP result = PROTECT(Rf_allocVector(REALSXP, 2));
    GetRNGstate();
    REAL(result)[0] = norm_rand();
    REAL(result)[1] = unif_rand();
    PutRNGstate();
    UNPROTECT(1);
    return result;
}

SEXP fixture_multinomial(void)
{
    SEXP counts = PROTECT(Rf_allocVector(INTSXP, 3));
    double probabilities[] = {0.2, 0.3, 0.5};
    GetRNGstate();
    Rf_rmultinom(12, probabilities, 3, INTEGER(counts));
    PutRNGstate();
    UNPROTECT(1);
    return counts;
}

SEXP fixture_math(void)
{
    SEXP result = PROTECT(Rf_allocVector(REALSXP, 7));
    double *values = REAL(result);
    values[0] = Rf_dnorm4(1.5, 0, 1, 0);
    values[1] = Rf_pnorm5(2, 0, 1, 0, 1);
    values[2] = Rf_qnorm5(0.975, 0, 1, 1, 0);
    values[3] = Rf_dgamma(2, 3, 2, 0);
    values[4] = Rf_dbeta(0.25, 2, 3, 0);
    values[5] = Rf_choose(10, 3);
    values[6] = Rf_lgammafn(4.5);

    if (!R_IsNA(NA_REAL) || R_IsNA(R_NaN) || R_IsNaN(NA_REAL) ||
        !R_IsNaN(R_NaN) || R_finite(R_PosInf) || !R_finite(-1.25))
        Rf_error("Non-finite value classification failed.");
    UNPROTECT(1);
    return result;
}

SEXP fixture_sort(void)
{
    SEXP values = PROTECT(Rf_allocVector(REALSXP, 3));
    REAL(values)[0] = 8;
    REAL(values)[1] = -3;
    REAL(values)[2] = 2;
    SEXP indices = PROTECT(Rf_allocVector(INTSXP, 3));
    for (int i = 0; i < 3; ++i)
        INTEGER(indices)[i] = i + 1;
    R_qsort_I(REAL(values), INTEGER(indices), 1, 3);

    double secondary[] = {R_PosInf, 2, -3};
    R_rsort(secondary, 3);
    if (secondary[0] != -3 || secondary[1] != 2 || secondary[2] != R_PosInf)
        Rf_error("R_rsort order check failed.");

    SEXP result = PROTECT(Rf_allocVector(VECSXP, 2));
    SET_VECTOR_ELT(result, 0, values);
    SET_VECTOR_ELT(result, 1, indices);
    UNPROTECT(3);
    return result;
}

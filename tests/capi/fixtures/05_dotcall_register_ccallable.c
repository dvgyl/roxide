/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#define R_NO_REMAP
#include <R.h>
#include <Rinternals.h>

static const char package_name[] = "fixture_dispatch";
static DllInfo *registered_dll;

static SEXP plus_one(SEXP value)
{
    return Rf_ScalarInteger(Rf_asInteger(value) + 1);
}

static SEXP use_callable(SEXP name, SEXP value)
{
    typedef SEXP (*Callable)(SEXP);
    Callable function = (Callable)R_GetCCallable(package_name, CHAR(STRING_ELT(name, 0)));
    return function(value);
}

static SEXP return_null(void)
{
    return NULL;
}

static SEXP external(SEXP args)
{
    SEXP values = CDR(args);
    if (TAG(values) != Rf_install("value"))
        Rf_error("Missing .External argument tag.");
    return CAR(values);
}

static SEXP external2(SEXP call, SEXP op, SEXP args, SEXP env)
{
    if (TYPEOF(call) != LANGSXP || TYPEOF(op) != BUILTINSXP || TYPEOF(env) != ENVSXP)
        Rf_error("Invalid .External2 call context.");
    SEXP values = CDR(args);
    if (TAG(values) != Rf_install("value"))
        Rf_error("Missing .External2 argument tag.");
    SEXP result = PROTECT(Rf_allocVector(VECSXP, 2));
    SET_VECTOR_ELT(result, 0, CAR(args));
    SET_VECTOR_ELT(result, 1, CAR(values));
    UNPROTECT(1);
    return result;
}

static SEXP lookup_mode(SEXP dynamic, SEXP force)
{
    if (!registered_dll)
        Rf_error("Fixture DLL is missing.");
    R_useDynamicSymbols(registered_dll, LOGICAL(dynamic)[0] ? TRUE : FALSE);
    R_forceSymbols(registered_dll, LOGICAL(force)[0] ? TRUE : FALSE);
    return R_NilValue;
}

static void c_args(int *integer, int *logical, double *real, Rcomplex *complex, char **text)
{
    *integer += 2;
    *logical = !*logical;
    *real += 0.5;
    complex->i += 1;
    if (text[0])
        text[0][0] = 'X';
}

static void fortran_text(char *text, int *result)
{
    *result = text[0] == 'a';
}

void F77_NAME(fixture_native_fortran)(int *value)
{
    *value *= 3;
}

static R_NativePrimitiveArgType c_types[] = {INTSXP, LGLSXP, REALSXP, CPLXSXP, STRSXP};
static const R_CMethodDef c_methods[] = {
    {"fixture_c", (DL_FUNC)c_args, 5, c_types},
    {NULL, NULL, 0, NULL}
};
static const R_CallMethodDef call_methods[] = {
    {"fixture_plus_one", (DL_FUNC)plus_one, 1},
    {"fixture_callable", (DL_FUNC)use_callable, 2},
    {"fixture_null", (DL_FUNC)return_null, 0},
    {"fixture_lookup_mode", (DL_FUNC)lookup_mode, 2},
    {NULL, NULL, 0}
};
static const R_FortranMethodDef fortran_methods[] = {
    {"fixture_fortran_text", (DL_FUNC)fortran_text, 2, NULL},
    {NULL, NULL, 0, NULL}
};
static const R_ExternalMethodDef external_methods[] = {
    {"fixture_external", (DL_FUNC)external, 1},
    {"fixture_external2", (DL_FUNC)external2, 1},
    {NULL, NULL, 0}
};

void R_init_f05_dotcall_register_ccallable(DllInfo *dll)
{
    registered_dll = dll;
    R_registerRoutines(dll, c_methods, call_methods, fortran_methods, external_methods);
    R_useDynamicSymbols(dll, TRUE);
    R_forceSymbols(dll, FALSE);
    R_RegisterCCallable(package_name, "plus_one", (DL_FUNC)plus_one);
    Rprintf("Native registration ran.\n");
}

void R_unload_f05_dotcall_register_ccallable(DllInfo *dll)
{
    if (dll != registered_dll)
        Rf_error("Invalid unload handle.");
    registered_dll = NULL;
    Rprintf("Native unload ran.\n");
}

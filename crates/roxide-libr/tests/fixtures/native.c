#include <Rinternals.h>
#include <R_ext/Rdynload.h>

static SEXP increment(SEXP value) {
    return Rf_ScalarInteger(INTEGER(value)[0] + 1);
}

static SEXP tagged_argument(SEXP args) {
    if (TAG(CDR(args)) != Rf_install("argument")) {
        Rf_error("The native call lost the argument name");
    }
    return CAR(CDR(args));
}

static SEXP external_context(SEXP call, SEXP op, SEXP args, SEXP env) {
    if (TYPEOF(call) != LANGSXP || TYPEOF(op) != BUILTINSXP || env != R_GlobalEnv) {
        Rf_error("The native call received an invalid context");
    }
    return tagged_argument(args);
}

static void increment_buffer(int *value) {
    *value += 1;
}

void R_init_native(DllInfo *dll) {
    static const R_CallMethodDef calls[] = {
        {"increment", (DL_FUNC)increment, 1}, {NULL, NULL, 0}
    };
    static const R_ExternalMethodDef external[] = {
        {"tagged_argument", (DL_FUNC)tagged_argument, 1},
        {"external_context", (DL_FUNC)external_context, 1},
        {NULL, NULL, 0}
    };
    static const R_CMethodDef buffers[] = {
        {"increment_buffer", (DL_FUNC)increment_buffer, 1, NULL}, {NULL, NULL, 0, NULL}
    };
    static const R_FortranMethodDef fortran[] = {
        {"increment_fortran", (DL_FUNC)increment_buffer, 1, NULL}, {NULL, NULL, 0, NULL}
    };
    R_registerRoutines(dll, buffers, calls, fortran, external);
    R_useDynamicSymbols(dll, FALSE);
}

/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#define R_NO_REMAP
#include <R.h>
#include <Rinternals.h>
#include <R_ext/Parse.h>
#include <setjmp.h>
#include <string.h>

extern SEXP Rf_findVarInFrame(SEXP rho, SEXP symbol);

static void check(int ok, const char *operation)
{
    if (!ok)
        Rf_error("Environment fixture check failed: %s", operation);
}

SEXP fixture_environment(void)
{
    SEXP env = PROTECT(R_NewEnv(R_BaseEnv, TRUE, 17));
    SEXP x = Rf_install("x");
    SEXP hidden = Rf_install(".hidden");
    SEXP removed = Rf_install("removed");
    SEXP value = PROTECT(Rf_ScalarInteger(42));
    Rf_defineVar(x, value, env);
    Rf_defineVar(hidden, R_NilValue, env);
    Rf_defineVar(removed, value, env);
    check(Rf_findVarInFrame(env, x) == value, "binding lookup");
    check(R_existsVarInFrame(env, removed), "binding exists");
    R_removeVarFromFrame(removed, env);
    check(!R_existsVarInFrame(env, removed), "binding removed");
    SEXP names = PROTECT(R_lsInternal3(env, TRUE, TRUE));
    check(Rf_length(names) == 2 &&
          strcmp(CHAR(STRING_ELT(names, 0)), ".hidden") == 0 &&
          strcmp(CHAR(STRING_ELT(names, 1)), "x") == 0, "binding names");
    R_LockBinding(x, env);
    R_LockEnvironment(env, FALSE);
    UNPROTECT(3);
    return env;
}

struct evaluation {
    SEXP call;
    int cleaned;
    int jumped;
    jmp_buf resume;
};

static SEXP evaluate(void *data)
{
    struct evaluation *state = data;
    return Rf_eval(state->call, R_BaseEnv);
}

static SEXP condition_message(SEXP condition, void *data)
{
    (void)data;
    SEXP call = PROTECT(Rf_lang2(Rf_install("conditionMessage"), condition));
    SEXP result = Rf_eval(call, R_BaseEnv);
    UNPROTECT(1);
    return result;
}

static void cleanup(void *data)
{
    struct evaluation *state = data;
    ++state->cleaned;
    Rprintf("R_ExecWithCleanup ran cleanup.\n");
}

static SEXP execute_with_cleanup(void *data)
{
    return R_ExecWithCleanup(evaluate, data, cleanup, data);
}

static void unwind_cleanup(void *data, Rboolean jump)
{
    struct evaluation *state = data;
    if (jump) {
        Rprintf("R_UnwindProtect ran cleanup for an error.\n");
        longjmp(state->resume, 1);
    }
    ++state->cleaned;
    state->jumped = jump;
}

SEXP fixture_catch(SEXP call)
{
    struct evaluation state = {.call = call};
    SEXP result = PROTECT(R_tryCatchError(execute_with_cleanup, &state,
                                         condition_message, NULL));
    check(state.cleaned == 1, "cleanup count");
    UNPROTECT(1);
    return result;
}

SEXP fixture_unwind(SEXP call)
{
    struct evaluation state = {.call = call};
    SEXP continuation = PROTECT(R_MakeUnwindCont());
    if (setjmp(state.resume))
        R_ContinueUnwind(continuation);
    SEXP result = PROTECT(R_UnwindProtect(evaluate, &state, unwind_cleanup,
                                          &state, continuation));
    check(state.cleaned == 1 && !state.jumped, "normal unwind cleanup");
    UNPROTECT(2);
    return result;
}

SEXP fixture_eval(SEXP text, SEXP failing_call, SEXP env)
{
    ParseStatus status;
    SEXP code = PROTECT(R_ParseVector(text, -1, &status, R_NilValue));
    check(status == PARSE_OK && Rf_length(code) == 1, "parse status");
    SEXP expression = VECTOR_ELT(code, 0);
    SEXP value = PROTECT(Rf_eval(expression, env));

    int failed = -1;
    SEXP second = PROTECT(R_tryEval(expression, env, &failed));
    check(!failed && Rf_asInteger(second) == Rf_asInteger(value), "tryEval");
    R_tryEval(failing_call, env, &failed);
    check(failed && strstr(R_curErrorBuf(), "Eval fixture error.") != NULL,
          "tryEval error buffer");
    failed = 0;
    R_tryEvalSilent(failing_call, env, &failed);
    check(failed && strstr(R_curErrorBuf(), "Eval fixture error.") != NULL,
          "tryEvalSilent error buffer");

    Rprintf("C API option width: %d\n", Rf_asInteger(Rf_GetOption1(Rf_install("width"))));
    UNPROTECT(3);
    return value;
}

SEXP fixture_signal(SEXP call, SEXP kind)
{
    switch (Rf_asInteger(kind)) {
    case 1:
        Rf_warning("Plain C warning.");
        return R_NilValue;
    case 2:
        Rf_warningcall(call, "C warning with a call.");
        return R_NilValue;
    case 3:
        Rf_error("Plain C error.");
    default:
        Rf_errorcall(call, "C error with a call.");
    }
}

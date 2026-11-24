/* SPDX-License-Identifier: MIT OR Apache-2.0 */

#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct SEXPREC *Sexp;
extern Sexp const R_NilValue;
extern _Noreturn void rox_capi_longjmp(int, int);
extern int rox_capi_error_str(const char *, size_t);
extern int rox_capi_errorcall_str(Sexp, const char *, size_t);
extern int rox_capi_warning_str(const char *, size_t);
extern int rox_capi_warningcall_str(Sexp, const char *, size_t);
extern int rox_capi_rprintf_str(const char *, size_t);
extern int rox_capi_reprintf_str(const char *, size_t);

enum { buffer_size = 8192, print_limit = 1024 * 1024 };

enum output { error, error_call, warning, warning_call, standard_out, standard_err };

static int emit(enum output dest, Sexp call, const char *format, va_list args) {
    char stack[buffer_size];
    va_list copy;
    va_copy(copy, args);
    int length = vsnprintf(stack, sizeof(stack), format, copy);
    va_end(copy);
    if (length < 0) {
        stack[0] = '\0';
        length = 0;
    }
    char *buf = stack;
    size_t size = (size_t)length < sizeof(stack) ? (size_t)length : sizeof(stack) - 1;
    if (length >= buffer_size && (dest == standard_out || dest == standard_err)) {
        size_t capacity = (size_t)length < print_limit ? (size_t)length + 1 : print_limit;
        char *heap = malloc(capacity);
        if (heap != NULL) {
            va_copy(copy, args);
            int written = vsnprintf(heap, capacity, format, copy);
            va_end(copy);
            if (written >= 0) {
                buf = heap;
                size = (size_t)written < capacity ? (size_t)written : capacity - 1;
            } else {
                free(heap);
            }
        }
    }

    int target = -1;
    switch (dest) {
    case error: target = rox_capi_error_str(buf, size); break;
    case error_call: target = rox_capi_errorcall_str(call, buf, size); break;
    case warning: target = rox_capi_warning_str(buf, size); break;
    case warning_call: target = rox_capi_warningcall_str(call, buf, size); break;
    case standard_out: target = rox_capi_rprintf_str(buf, size); break;
    case standard_err: target = rox_capi_reprintf_str(buf, size); break;
    }
    if (buf != stack) {
        free(buf);
    }
    return target;
}

static void finish(int target) {
    if (target >= 0) {
        rox_capi_longjmp(target, 1);
    }
}

#define VARARGS(name, dest)                         \
    void name(const char *format, ...) {             \
        va_list args;                               \
        va_start(args, format);                     \
        int target = emit(dest, R_NilValue, format, args); \
        va_end(args);                               \
        finish(target);                             \
    }

#define CONDITION(name, dest, qualifier, args_decl, call, after) \
    qualifier void name args_decl {                \
        va_list args;                               \
        va_start(args, format);                     \
        int target = emit(dest, call, format, args); \
        va_end(args);                               \
        finish(target);                             \
        after                                       \
    }

CONDITION(Rf_error, error, _Noreturn, (const char *format, ...), R_NilValue, abort();)
CONDITION(Rf_errorcall, error_call, _Noreturn, (Sexp call, const char *format, ...), call, abort();)
VARARGS(Rf_warning, warning)
CONDITION(Rf_warningcall, warning_call, , (Sexp call, const char *format, ...), call, )
VARARGS(Rprintf, standard_out)
VARARGS(REprintf, standard_err)

void Rvprintf(const char *format, va_list args) {
    finish(emit(standard_out, R_NilValue, format, args));
}

void REvprintf(const char *format, va_list args) {
    finish(emit(standard_err, R_NilValue, format, args));
}

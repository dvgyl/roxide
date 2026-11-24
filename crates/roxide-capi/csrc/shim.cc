/* SPDX-License-Identifier: MIT OR Apache-2.0 */

#include <cstddef>
#include <cstdio>
#include <cstdlib>
#include <exception>
#include <setjmp.h>

extern "C" {
using Sexp = struct SEXPREC *;
using Body = Sexp (*)(void *);
using Cleanup = void (*)(void *, int);
using Handler = Sexp (*)(Sexp, void *);
using Callback = void (*)(void *);

extern Sexp const R_NilValue;
[[noreturn]] void R_ContinueUnwind(Sexp);
}

#ifndef ROXIDE_TRAMPOLINE_FIXTURE
namespace {
constexpr int max_depth = 256;
thread_local sigjmp_buf targets[max_depth];
thread_local int depth = 0;

enum Status { ok, jumped, cpp_exception, depth_limit };
}

extern "C" int rox_capi_trampoline(Callback fn, void *arg, int *out_code,
                                    char *cpp_err_buf, size_t cpp_err_cap) noexcept {
    if (depth == max_depth) {
        return depth_limit;
    }
    const int slot = depth++;
    const int code = sigsetjmp(targets[slot], 1);
    if (code != 0) {
        depth = slot;
        *out_code = code;
        return jumped;
    }
    int status = ok;
    try {
        fn(arg);
    } catch (const std::exception &e) {
        if (cpp_err_cap != 0) {
            std::snprintf(cpp_err_buf, cpp_err_cap, "%s", e.what());
        }
        status = cpp_exception;
    } catch (...) {
        if (cpp_err_cap != 0) {
            std::snprintf(cpp_err_buf, cpp_err_cap, "%s", "unknown C++ exception");
        }
        status = cpp_exception;
    }
    depth = slot;
    return status;
}

extern "C" [[noreturn]] void rox_capi_longjmp(int target_depth, int code) noexcept {
    if (depth == 0 || target_depth != depth - 1) {
        std::fputs("Roxide C API jump has no matching trampoline\n", stderr);
        std::abort();
    }
    siglongjmp(targets[target_depth], code);
}

extern "C" {
struct BodyCall {
    Body fn;
    void *data;
    Sexp result;
};

struct CleanupCall {
    Cleanup fn;
    void *data;
    int jumping;
};

struct HandlerCall {
    Handler fn;
    Sexp condition;
    void *data;
    Sexp result;
};

struct UnwindProtectCall {
    Body fn;
    void *data;
    Cleanup clean;
    void *clean_data;
    Sexp cont;
    Sexp result;
};

void rox_capi_call_body(void *data) {
    auto *call = static_cast<BodyCall *>(data);
    call->result = call->fn(call->data);
}

void rox_capi_call_cleanup(void *data) {
    auto *call = static_cast<CleanupCall *>(data);
    call->fn(call->data, call->jumping);
}

void rox_capi_call_handler(void *data) {
    auto *call = static_cast<HandlerCall *>(data);
    call->result = call->fn(call->condition, call->data);
}

int rox_capi_unwind_protect(UnwindProtectCall *);

Sexp R_UnwindProtect(Body fn, void *data, Cleanup clean, void *clean_data, Sexp cont) {
    UnwindProtectCall call{fn, data, clean, clean_data, cont, R_NilValue};
    if (rox_capi_unwind_protect(&call) == 0) {
        return call.result;
    }

    // A C++ cleanup can throw or jump to a package-owned target.
    // All Rust borrows and the inner trampoline have ended here.
    if (clean != nullptr) {
        clean(clean_data, 1);
    }
    R_ContinueUnwind(call.cont);
}
}
#else

#include <stdexcept>

extern "C" {
[[noreturn]] void Rf_error(const char *, ...);
void Rprintf(const char *, ...);
void REprintf(const char *, ...);
Sexp Rf_protect(Sexp);
void Rf_unprotect(int);
void Rf_unprotect_ptr(Sexp);
void R_ProtectWithIndex(Sexp, int *);
void R_Reprotect(Sexp, int);
Sexp R_MakeUnwindCont();
Sexp R_UnwindProtect(Body, void *, Cleanup, void *, Sexp);
Sexp R_tryCatchError(Body, void *, Handler, void *);
int R_ToplevelExec(Callback, void *);
extern Sexp const R_TrueValue;
extern Sexp const R_FalseValue;

void rx_test_error(void *) {
    Rf_protect(R_NilValue);
    Rf_error("native error %d", 42);
}

void rx_test_nested(void *data) {
    Rf_protect(R_NilValue);
    struct Bridge {
        Callback fun;
        void *data;
    };
    auto *bridge = static_cast<Bridge *>(data);
    bridge->fun(bridge->data);
    Rf_unprotect(1);
}

void rx_test_long_error(void *) {
    Rf_error("%09000d", 1);
}

void rx_test_print(void *) {
    Rprintf("%09000d", 1);
    REprintf("output %d", 42);
}

void rx_test_imbalance(void *) {
    Rf_protect(R_NilValue);
}

void rx_test_boom(void *) {
    throw std::runtime_error("boom");
}

void rx_test_unknown_exception(void *) {
    throw 42;
}

void rx_test_overflow(void *) {
    Rf_protect(R_NilValue);
    Rf_protect(R_NilValue);
}

void rx_test_protect_index(void *) {
    int index = -1;
    Rf_protect(R_FalseValue);
    R_ProtectWithIndex(R_NilValue, &index);
    R_Reprotect(R_TrueValue, index);
    Rf_unprotect_ptr(R_TrueValue);
    Rf_unprotect_ptr(R_FalseValue);
}

void rx_test_unprotect_missing(void *) {
    Rf_unprotect_ptr(R_TrueValue);
}

struct CleanupProbe {
    int destructed;
    int cleaned;
    bool fail;
    int cpp_unwind;
    Callback collect;
    void *collect_data;
};

struct DropProbe {
    CleanupProbe *probe;
    ~DropProbe() { ++probe->destructed; }
};

struct ResumeUnwind { int unused; };

struct JumpProbe {
    CleanupProbe *probe;
    sigjmp_buf *target;
};

Sexp rx_test_unwind_body(void *data) {
    auto *probe = static_cast<CleanupProbe *>(data);
    if (probe->fail) {
        Rf_error("unwind body error");
    }
    return R_TrueValue;
}

void rx_test_unwind_clean(void *data, int jumping) {
    auto *probe = static_cast<JumpProbe *>(data);
    probe->probe->cleaned += jumping ? 10 : 1;
    probe->probe->collect(probe->probe->collect_data);
    if (jumping) {
        if (probe->probe->cpp_unwind == 2) {
            throw ResumeUnwind{};
        }
        if (probe->target != nullptr) {
            siglongjmp(*probe->target, 1);
        }
    }
}

Sexp rx_test_cpp_frame(CleanupProbe *probe, Sexp cont) {
    DropProbe drop{probe};
    sigjmp_buf target;
    JumpProbe jump{probe, &target};
    if (sigsetjmp(target, 1) != 0) {
        throw ResumeUnwind{};
    }
    return R_UnwindProtect(rx_test_unwind_body, probe, rx_test_unwind_clean, &jump, cont);
}

void rx_test_unwind_protect(void *data) {
    auto *probe = static_cast<CleanupProbe *>(data);
    if (probe->cpp_unwind == 0) {
        JumpProbe jump{probe, nullptr};
        R_UnwindProtect(rx_test_unwind_body, probe, rx_test_unwind_clean, &jump, nullptr);
        return;
    }
    Sexp cont = Rf_protect(R_MakeUnwindCont());
    try {
        probe->cleaned += rx_test_cpp_frame(probe, cont) == R_TrueValue ? 100 : 1000;
    } catch (const ResumeUnwind &) {
        // Leave the exception handler before the R jump.
        probe->cleaned += 100;
    }
    if (probe->fail) {
        R_ContinueUnwind(cont);
    }
    Rf_unprotect(1);
}

Sexp rx_test_catch_body(void *) {
    rx_test_error(nullptr);
    return R_NilValue;
}

Sexp rx_test_catch_handler(Sexp condition, void *data) {
    *static_cast<Sexp *>(data) = condition;
    return R_TrueValue;
}

void rx_test_catch(void *data) {
    auto *values = static_cast<Sexp *>(data);
    values[0] = R_tryCatchError(rx_test_catch_body, nullptr, rx_test_catch_handler, values + 1);
}

void rx_test_toplevel(void *data) {
    auto *values = static_cast<int *>(data);
    values[0] = R_ToplevelExec(rx_test_error, nullptr);
    values[1] = R_ToplevelExec([](void *) {}, nullptr);
}
}
#endif

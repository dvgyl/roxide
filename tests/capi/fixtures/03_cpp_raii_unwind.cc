/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#include <R.h>
#include <Rinternals.h>
#include <memory>

namespace {
struct ResumeRUnwind {};

int live_buffers = 0;
int released_objects = 0;

class Resources {
public:
    explicit Resources(SEXP object)
        : object_(object), buffer_(std::make_unique<int[]>(32))
    {
        R_PreserveObject(object_);
        ++live_buffers;
    }

    ~Resources()
    {
        buffer_.reset();
        --live_buffers;
        R_ReleaseObject(object_);
        ++released_objects;
    }

    Resources(const Resources &) = delete;
    Resources &operator=(const Resources &) = delete;

private:
    SEXP object_;
    std::unique_ptr<int[]> buffer_;
};

SEXP run_callback(void *data)
{
    SEXP call = PROTECT(Rf_lang1(*static_cast<SEXP *>(data)));
    SEXP result = Rf_eval(call, R_GlobalEnv);
    UNPROTECT(1);
    return result;
}

void cleanup(void *, Rboolean jump)
{
    if (jump)
        throw ResumeRUnwind{};
}

SEXP evaluate(SEXP callback, SEXP object, SEXP continuation)
{
    Resources resources(object);
    return R_UnwindProtect(run_callback, &callback, cleanup, nullptr, continuation);
}
} // namespace

extern "C" SEXP fixture_raii(SEXP callback, SEXP object)
{
    SEXP continuation = PROTECT(R_MakeUnwindCont());
    try {
        SEXP result = evaluate(callback, object, continuation);
        UNPROTECT(1);
        return result;
    } catch (const ResumeRUnwind &) {
        // Exit the C++ handler before resuming the R error.
    }
    R_ContinueUnwind(continuation);
}

extern "C" SEXP fixture_raii_counts()
{
    SEXP counts = PROTECT(Rf_allocVector(INTSXP, 2));
    INTEGER(counts)[0] = live_buffers;
    INTEGER(counts)[1] = released_objects;
    UNPROTECT(1);
    return counts;
}

extern "C" SEXP fixture_raii_weak_ref(SEXP object)
{
    return R_MakeWeakRef(object, R_NilValue, R_NilValue, FALSE);
}

extern "C" SEXP fixture_raii_weak_key(SEXP reference)
{
    return R_WeakRefKey(reference);
}

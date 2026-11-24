/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#include <Rinternals.h>
#include <stdexcept>

extern "C" SEXP fixture_throw()
{
    throw std::runtime_error("Fixture C++ exception.");
}

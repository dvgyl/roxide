/* SPDX-License-Identifier: MIT OR Apache-2.0 */

#define R_NO_REMAP

#include <R_ext/Altrep.h>
#include <R_ext/Rdynload.h>

static R_altrep_class_t integers, reals, strings, lists, no_pointer;

static R_xlen_t length(SEXP x) { return XLENGTH(R_altrep_data1(x)); }
static int int_elt(SEXP x, R_xlen_t i) { return INTEGER_ELT(R_altrep_data1(x), i); }
static double real_elt(SEXP x, R_xlen_t i) { return REAL_ELT(R_altrep_data1(x), i); }
static SEXP str_elt(SEXP x, R_xlen_t i) { return STRING_ELT(R_altrep_data1(x), i); }
static SEXP list_elt(SEXP x, R_xlen_t i) { return VECTOR_ELT(R_altrep_data1(x), i); }
static void str_set(SEXP x, R_xlen_t i, SEXP value) { SET_STRING_ELT(R_altrep_data1(x), i, value); }
static void list_set(SEXP x, R_xlen_t i, SEXP value) { SET_VECTOR_ELT(R_altrep_data1(x), i, value); }

static R_xlen_t int_region(SEXP x, R_xlen_t i, R_xlen_t n, int *out) {
    return INTEGER_GET_REGION(R_altrep_data1(x), i, n, out);
}

static void *data(SEXP x, Rboolean rw) {
    return rw ? DATAPTR_RW(R_altrep_data1(x)) : (void *)DATAPTR_RO(R_altrep_data1(x));
}

static const void *data_or_null(SEXP x) { return DATAPTR_OR_NULL(R_altrep_data1(x)); }
static int yes(SEXP x) { (void)x; return 1; }

static SEXP sum(SEXP x, Rboolean na_rm) {
    (void)na_rm;
    int mode = Rf_asInteger(R_altrep_data2(x));
    if (mode == 1) return R_NilValue;
    if (mode == 2) return Rf_ScalarInteger(123);
    return NULL;
}

static SEXP subset(SEXP x, SEXP index, SEXP call) {
    (void)x;
    (void)call;
    SEXP ans = PROTECT(Rf_allocVector(INTSXP, XLENGTH(index)));
    for (R_xlen_t i = 0; i < XLENGTH(index); ++i) {
        SET_INTEGER_ELT(ans, i, 100 + INTEGER_ELT(index, i));
    }
    UNPROTECT(1);
    return ans;
}

static SEXP duplicate(SEXP x, Rboolean deep) {
    return Rf_ScalarInteger(int_elt(x, 0) + (deep ? 30 : 20));
}

void R_init_f07_altrep(DllInfo *dll) {
    integers = R_make_altinteger_class("fixture_integer", "fixture", dll);
    no_pointer = R_make_altinteger_class("fixture_no_pointer", "fixture", dll);
    reals = R_make_altreal_class("fixture_real", "fixture", dll);
    strings = R_make_altstring_class("fixture_string", "fixture", dll);
    lists = R_make_altlist_class("fixture_list", "fixture", dll);
    R_set_altrep_Length_method(integers, length);
    R_set_altrep_Length_method(no_pointer, length);
    R_set_altrep_Length_method(reals, length);
    R_set_altrep_Length_method(strings, length);
    R_set_altrep_Length_method(lists, length);
    R_set_altinteger_Elt_method(integers, int_elt);
    R_set_altinteger_Get_region_method(integers, int_region);
    R_set_altinteger_Elt_method(no_pointer, int_elt);
    R_set_altinteger_Sum_method(integers, sum);
    R_set_altinteger_Is_sorted_method(integers, yes);
    R_set_altinteger_No_NA_method(integers, yes);
    R_set_altvec_Dataptr_method(integers, data);
    R_set_altvec_Dataptr_or_null_method(integers, data_or_null);
    R_set_altvec_Extract_subset_method(integers, subset);
    R_set_altrep_Duplicate_method(integers, duplicate);
    R_set_altreal_Elt_method(reals, real_elt);
    R_set_altvec_Dataptr_method(reals, data);
    R_set_altstring_Elt_method(strings, str_elt);
    R_set_altstring_Set_elt_method(strings, str_set);
    R_set_altlist_Elt_method(lists, list_elt);
    R_set_altlist_Set_elt_method(lists, list_set);
}

SEXP fixture_alt_new(SEXP data1, SEXP data2) {
    R_altrep_class_t cls;
    switch (TYPEOF(data1)) {
        case INTSXP: cls = Rf_asInteger(data2) == 3 ? no_pointer : integers; break;
        case REALSXP: cls = reals; break;
        case STRSXP: cls = strings; break;
        case VECSXP: cls = lists; break;
        default: Rf_error("Unexpected type.");
    }
    return R_new_altrep(cls, data1, data2);
}

SEXP fixture_alt_integer(SEXP x, SEXP replacement) {
    int out[2] = {0, 0};
    R_set_altrep_data1(x, replacement);
    R_set_altrep_data2(x, replacement);
    if (!R_altrep_inherits(x, integers) || ALTREP_CLASS(x) != R_SEXP(integers) ||
        R_altrep_class_name(x) != Rf_install("fixture_integer") ||
        R_altrep_class_package(x) != Rf_install("fixture") ||
        R_altrep_data1(x) != replacement || R_altrep_data2(x) != replacement ||
        !INTEGER_IS_SORTED(x) || !INTEGER_NO_NA(x) ||
        !DATAPTR_OR_NULL(x) || INTEGER_GET_REGION(x, 1, 2, out) != 2) {
        Rf_error("Unexpected integer ALTREP state.");
    }
    return Rf_ScalarInteger(out[0] + out[1]);
}

SEXP fixture_alt_duplicate(SEXP x) { return Rf_duplicate(x); }

SEXP fixture_alt_no_pointer(SEXP x) {
    if (DATAPTR_OR_NULL(x)) Rf_error("Unexpected data pointer.");
    return Rf_ScalarInteger(INTEGER_RO(x)[0]);
}

SEXP fixture_alt_real(SEXP x) {
    return Rf_ScalarReal(REAL_ELT(x, 1) + REAL_RO(x)[0]);
}

SEXP fixture_alt_mutate(SEXP string, SEXP list) {
    SEXP value = PROTECT(Rf_mkChar("changed"));
    SET_STRING_ELT(string, 1, value);
    if (STRING_ELT(string, 1) != value) Rf_error("String update failed.");
    SET_VECTOR_ELT(list, 0, string);
    if (VECTOR_ELT(list, 0) != string) Rf_error("List update failed.");
    UNPROTECT(1);
    return Rf_ScalarLogical(1);
}

#ifdef ROXIDE_ALTREP_TEST
static R_altrep_class_t restore_class;

static SEXP legacy_restore(SEXP info, SEXP state) {
    if (CAR(info) != Rf_install("restore")) Rf_error("Incorrect serialized class.");
    return R_new_altrep(restore_class, state, R_NilValue);
}

static SEXP extended_restore(SEXP info, SEXP state, SEXP attrs, int objf, int levels) {
    (void)state;
    if (CAR(info) != Rf_install("restore") || attrs != R_NilValue || objf != 1)
        Rf_error("Incorrect extended restore arguments.");
    return Rf_ScalarInteger(levels);
}

static SEXP coerce(SEXP x, int type) {
    return type == REALSXP ? Rf_ScalarReal(int_elt(x, 0) + 0.5) : NULL;
}

static SEXP extreme(SEXP x, Rboolean na_rm) {
    return Rf_ScalarInteger(int_elt(x, 0) + (na_rm ? 70 : 50));
}

static Rboolean inspect(SEXP x, int pre, int deep, int pvec,
                        void (*subtree)(SEXP, int, int, int)) {
    if (subtree) subtree(R_altrep_data1(x), pre, deep, pvec);
    return pre == 2 && deep == 3 && pvec == 4;
}

static SEXP duplicate_ex(SEXP x, Rboolean deep) {
    return Rf_ScalarInteger(int_elt(x, 0) + (deep ? 40 : 10));
}

static void *transient_pointer(SEXP x, Rboolean writeable) {
    int *storage = (int *)R_alloc(3, sizeof(int));
    storage[0] = int_elt(x, 0) + writeable;
    storage[1] = int_elt(x, 1);
    storage[2] = int_elt(x, 2);
    PROTECT(R_altrep_data1(x));
    return storage;
}

static R_xlen_t fail_length(SEXP x) {
    PROTECT(R_altrep_data1(x));
    Rf_error("Foreign Length failed.");
}

SEXP fixture_alt_restore(SEXP state, int extended) {
    restore_class = R_make_altinteger_class("restore", "fixture", NULL);
    R_set_altrep_Length_method(restore_class, length);
    R_set_altinteger_Elt_method(restore_class, int_elt);
    R_set_altrep_Serialized_state_method(restore_class, R_altrep_data1);
    R_set_altrep_Unserialize_method(restore_class, legacy_restore);
    if (extended) R_set_altrep_UnserializeEX_method(restore_class, extended_restore);
    R_set_altrep_Inspect_method(restore_class, inspect);
    R_set_altrep_Duplicate_method(restore_class, duplicate);
    R_set_altrep_DuplicateEX_method(restore_class, duplicate_ex);
    R_set_altrep_Coerce_method(restore_class, coerce);
    R_set_altinteger_Min_method(restore_class, extreme);
    R_set_altinteger_Max_method(restore_class, extreme);
    R_set_altvec_Dataptr_method(restore_class, transient_pointer);
    return R_new_altrep(restore_class, state, R_NilValue);
}

SEXP fixture_alt_fail(SEXP state) {
    R_altrep_class_t cls = R_make_altinteger_class("fail", "fixture", NULL);
    R_set_altrep_Length_method(cls, fail_length);
    return R_new_altrep(cls, state, R_NilValue);
}

static R_xlen_t one_int_region(SEXP x, R_xlen_t start, R_xlen_t len, int *out) {
    return int_region(x, start, len > 1 ? 1 : len, out);
}

static int logical_elt(SEXP x, R_xlen_t i) {
    return LOGICAL_ELT(R_altrep_data1(x), i);
}

static Rbyte raw_elt(SEXP x, R_xlen_t i) { return RAW_ELT(R_altrep_data1(x), i); }

static Rcomplex complex_elt(SEXP x, R_xlen_t i) {
    return COMPLEX_ELT(R_altrep_data1(x), i);
}

SEXP fixture_alt_atomic(SEXP input) {
    R_altrep_class_t cls;
    switch (TYPEOF(input)) {
        case INTSXP:
            cls = R_make_altinteger_class("partial_region", "fixture", NULL);
            R_set_altinteger_Get_region_method(cls, one_int_region);
            break;
        case LGLSXP:
            cls = R_make_altlogical_class("logical", "fixture", NULL);
            R_set_altlogical_Elt_method(cls, logical_elt);
            break;
        case RAWSXP:
            cls = R_make_altraw_class("raw", "fixture", NULL);
            R_set_altraw_Elt_method(cls, raw_elt);
            break;
        case CPLXSXP:
            cls = R_make_altcomplex_class("complex", "fixture", NULL);
            R_set_altcomplex_Elt_method(cls, complex_elt);
            break;
        default: Rf_error("Unexpected atomic type.");
    }
    R_set_altrep_Length_method(cls, length);
    return R_new_altrep(cls, input, R_NilValue);
}
#endif

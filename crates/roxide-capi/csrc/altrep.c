/* SPDX-License-Identifier: MIT OR Apache-2.0 */

#include <R_ext/Altrep.h>

struct RoxAltUnserializeEx {
    R_altrep_UnserializeEX_method_t fun;
    SEXP class_info;
    SEXP state;
    SEXP attributes;
    int object;
    int levels;
    SEXP result;
};

void rox_capi_alt_unserialize_ex(void *data) {
    struct RoxAltUnserializeEx *call = data;
    call->result = call->fun(call->class_info, call->state, call->attributes,
                             call->object, call->levels);
}

struct RoxAltUnserialize {
    R_altrep_Unserialize_method_t fun;
    SEXP class_info;
    SEXP state;
    SEXP result;
};

void rox_capi_alt_unserialize(void *data) {
    struct RoxAltUnserialize *call = data;
    call->result = call->fun(call->class_info, call->state);
}

struct RoxAltInspect {
    R_altrep_Inspect_method_t fun;
    SEXP x;
    int pre;
    int deep;
    int pvec;
    void (*inspect_subtree)(SEXP, int, int, int);
    Rboolean result;
};

void rox_capi_alt_inspect(void *data) {
    struct RoxAltInspect *call = data;
    call->result = call->fun(call->x, call->pre, call->deep, call->pvec,
                             call->inspect_subtree);
}

struct RoxAltExtractSubset {
    R_altvec_Extract_subset_method_t fun;
    SEXP x;
    SEXP index;
    SEXP call;
    SEXP result;
};

void rox_capi_alt_extract_subset(void *data) {
    struct RoxAltExtractSubset *args = data;
    args->result = args->fun(args->x, args->index, args->call);
}

#define ALT_UNARY(name, method, result_type) \
    struct rox_alt_##name { \
        method fun; \
        SEXP x; \
        result_type result; \
    }; \
    void rox_capi_alt_##name(void *data) { \
        struct rox_alt_##name *call = data; \
        call->result = call->fun(call->x); \
    }

#define ALT_BINARY(name, method, arg_type, arg, result_type) \
    struct rox_alt_##name { \
        method fun; \
        SEXP x; \
        arg_type arg; \
        result_type result; \
    }; \
    void rox_capi_alt_##name(void *data) { \
        struct rox_alt_##name *call = data; \
        call->result = call->fun(call->x, call->arg); \
    }

ALT_UNARY(serialized_state, R_altrep_Serialized_state_method_t, SEXP)
ALT_UNARY(length, R_altrep_Length_method_t, R_xlen_t)
ALT_UNARY(dataptr_or_null, R_altvec_Dataptr_or_null_method_t, const void *)

ALT_BINARY(duplicate_ex, R_altrep_DuplicateEX_method_t, Rboolean, deep, SEXP)
ALT_BINARY(duplicate, R_altrep_Duplicate_method_t, Rboolean, deep, SEXP)
ALT_BINARY(coerce, R_altrep_Coerce_method_t, int, type, SEXP)
ALT_BINARY(dataptr, R_altvec_Dataptr_method_t, Rboolean, writable, void *)

ALT_BINARY(int_elt, R_altinteger_Elt_method_t, R_xlen_t, index, int)
ALT_BINARY(real_elt, R_altreal_Elt_method_t, R_xlen_t, index, double)
ALT_BINARY(lgl_elt, R_altlogical_Elt_method_t, R_xlen_t, index, int)
ALT_BINARY(raw_elt, R_altraw_Elt_method_t, R_xlen_t, index, Rbyte)
ALT_BINARY(cplx_elt, R_altcomplex_Elt_method_t, R_xlen_t, index, Rcomplex)
ALT_BINARY(str_elt, R_altstring_Elt_method_t, R_xlen_t, index, SEXP)
ALT_BINARY(list_elt, R_altlist_Elt_method_t, R_xlen_t, index, SEXP)

ALT_UNARY(int_is_sorted, R_altinteger_Is_sorted_method_t, int)
ALT_UNARY(real_is_sorted, R_altreal_Is_sorted_method_t, int)
ALT_UNARY(lgl_is_sorted, R_altlogical_Is_sorted_method_t, int)
ALT_UNARY(str_is_sorted, R_altstring_Is_sorted_method_t, int)
ALT_UNARY(int_no_na, R_altinteger_No_NA_method_t, int)
ALT_UNARY(real_no_na, R_altreal_No_NA_method_t, int)
ALT_UNARY(lgl_no_na, R_altlogical_No_NA_method_t, int)
ALT_UNARY(str_no_na, R_altstring_No_NA_method_t, int)

ALT_BINARY(int_sum, R_altinteger_Sum_method_t, Rboolean, na_rm, SEXP)
ALT_BINARY(real_sum, R_altreal_Sum_method_t, Rboolean, na_rm, SEXP)
ALT_BINARY(lgl_sum, R_altlogical_Sum_method_t, Rboolean, na_rm, SEXP)
ALT_BINARY(int_min, R_altinteger_Min_method_t, Rboolean, na_rm, SEXP)
ALT_BINARY(real_min, R_altreal_Min_method_t, Rboolean, na_rm, SEXP)
ALT_BINARY(int_max, R_altinteger_Max_method_t, Rboolean, na_rm, SEXP)
ALT_BINARY(real_max, R_altreal_Max_method_t, Rboolean, na_rm, SEXP)

#define ALT_REGION(kind, method, element) \
    struct rox_alt_##kind##_get_region { \
        method fun; \
        SEXP x; \
        R_xlen_t start; \
        R_xlen_t len; \
        element *out; \
        R_xlen_t result; \
    }; \
    void rox_capi_alt_##kind##_get_region(void *data) { \
        struct rox_alt_##kind##_get_region *call = data; \
        call->result = call->fun(call->x, call->start, call->len, call->out); \
    }

ALT_REGION(int, R_altinteger_Get_region_method_t, int)
ALT_REGION(real, R_altreal_Get_region_method_t, double)
ALT_REGION(lgl, R_altlogical_Get_region_method_t, int)
ALT_REGION(raw, R_altraw_Get_region_method_t, Rbyte)
ALT_REGION(cplx, R_altcomplex_Get_region_method_t, Rcomplex)

#define ALT_SET_ELT(kind, method) \
    struct rox_alt_##kind##_set_elt { \
        method fun; \
        SEXP x; \
        R_xlen_t index; \
        SEXP value; \
    }; \
    void rox_capi_alt_##kind##_set_elt(void *data) { \
        struct rox_alt_##kind##_set_elt *call = data; \
        call->fun(call->x, call->index, call->value); \
    }

ALT_SET_ELT(str, R_altstring_Set_elt_method_t)
ALT_SET_ELT(list, R_altlist_Set_elt_method_t)

#define ALT_FILL(kind, method, element) \
    struct rox_alt_fill_##kind##_elt { \
        method fun; \
        SEXP x; \
        R_xlen_t len; \
        element *out; \
    }; \
    void rox_capi_alt_fill_##kind##_elt(void *data) { \
        struct rox_alt_fill_##kind##_elt *call = data; \
        for (R_xlen_t i = 0; i < call->len; ++i) { \
            call->out[i] = call->fun(call->x, i); \
        } \
    }

ALT_FILL(int, R_altinteger_Elt_method_t, int)
ALT_FILL(real, R_altreal_Elt_method_t, double)
ALT_FILL(lgl, R_altlogical_Elt_method_t, int)
ALT_FILL(raw, R_altraw_Elt_method_t, Rbyte)
ALT_FILL(cplx, R_altcomplex_Elt_method_t, Rcomplex)

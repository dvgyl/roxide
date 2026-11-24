/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#include <limits.h>
#include <stddef.h>
#include <stdio.h>

#include <R.h>
#include <Rversion.h>
#include <Rinternals.h>
#include <Rmath.h>
#include <R_ext/Altrep.h>
#include <R_ext/Applic.h>
#include <R_ext/Arith.h>
#include <R_ext/Constants.h>
#include <R_ext/Parse.h>
#include <R_ext/Rdynload.h>

#define TYPE_LAYOUT(type) \
    printf("sizeof." #type "=%zu\n", sizeof(type)); \
    printf("alignof." #type "=%zu\n", _Alignof(type))
#define FIELD_OFFSET(type, field) \
    printf("offset." #type "." #field "=%zu\n", offsetof(type, field))
#define INTEGER_VALUE(value) printf("value." #value "=%lld\n", (long long)(value))
#define REAL_VALUE(value) printf("value." #value "=%a\n", (double)(value))

/* Use distinct values to check the NA macros without linking to libR. */
int R_NaInt = INT_MIN;

int main(void)
{
    TYPE_LAYOUT(SEXP);
    TYPE_LAYOUT(SEXPTYPE);
    TYPE_LAYOUT(Rboolean);
    TYPE_LAYOUT(R_len_t);
    TYPE_LAYOUT(R_xlen_t);
    TYPE_LAYOUT(Rbyte);
    TYPE_LAYOUT(Rcomplex);
    TYPE_LAYOUT(PROTECT_INDEX);
    TYPE_LAYOUT(cetype_t);
    TYPE_LAYOUT(ParseStatus);
    TYPE_LAYOUT(R_BindingType_t);
    TYPE_LAYOUT(R_DotType_t);
    TYPE_LAYOUT(R_pstream_format_t);
    TYPE_LAYOUT(struct R_outpstream_st);
    TYPE_LAYOUT(struct R_inpstream_st);
    TYPE_LAYOUT(DL_FUNC);
    TYPE_LAYOUT(R_NativePrimitiveArgType);
    TYPE_LAYOUT(R_CallMethodDef);
    TYPE_LAYOUT(R_CMethodDef);
    TYPE_LAYOUT(R_ExternalMethodDef);
    TYPE_LAYOUT(R_FortranMethodDef);
    TYPE_LAYOUT(R_altrep_class_t);
    TYPE_LAYOUT(optimfn *);
    TYPE_LAYOUT(optimgr *);
    TYPE_LAYOUT(integr_fn *);

    FIELD_OFFSET(Rcomplex, r);
    FIELD_OFFSET(Rcomplex, i);
    FIELD_OFFSET(R_altrep_class_t, ptr);
    FIELD_OFFSET(R_CallMethodDef, name);
    FIELD_OFFSET(R_CallMethodDef, fun);
    FIELD_OFFSET(R_CallMethodDef, numArgs);
    FIELD_OFFSET(R_ExternalMethodDef, name);
    FIELD_OFFSET(R_ExternalMethodDef, fun);
    FIELD_OFFSET(R_ExternalMethodDef, numArgs);
    FIELD_OFFSET(R_CMethodDef, name);
    FIELD_OFFSET(R_CMethodDef, fun);
    FIELD_OFFSET(R_CMethodDef, numArgs);
    FIELD_OFFSET(R_CMethodDef, types);
    FIELD_OFFSET(R_FortranMethodDef, name);
    FIELD_OFFSET(R_FortranMethodDef, fun);
    FIELD_OFFSET(R_FortranMethodDef, numArgs);
    FIELD_OFFSET(R_FortranMethodDef, types);

    INTEGER_VALUE(FALSE);
    INTEGER_VALUE(TRUE);
    INTEGER_VALUE(NILSXP);
    INTEGER_VALUE(SYMSXP);
    INTEGER_VALUE(LISTSXP);
    INTEGER_VALUE(CLOSXP);
    INTEGER_VALUE(ENVSXP);
    INTEGER_VALUE(PROMSXP);
    INTEGER_VALUE(LANGSXP);
    INTEGER_VALUE(SPECIALSXP);
    INTEGER_VALUE(BUILTINSXP);
    INTEGER_VALUE(CHARSXP);
    INTEGER_VALUE(LGLSXP);
    INTEGER_VALUE(INTSXP);
    INTEGER_VALUE(REALSXP);
    INTEGER_VALUE(CPLXSXP);
    INTEGER_VALUE(STRSXP);
    INTEGER_VALUE(DOTSXP);
    INTEGER_VALUE(ANYSXP);
    INTEGER_VALUE(VECSXP);
    INTEGER_VALUE(EXPRSXP);
    INTEGER_VALUE(BCODESXP);
    INTEGER_VALUE(EXTPTRSXP);
    INTEGER_VALUE(WEAKREFSXP);
    INTEGER_VALUE(RAWSXP);
    INTEGER_VALUE(OBJSXP);
    INTEGER_VALUE(S4SXP);
    INTEGER_VALUE(NEWSXP);
    INTEGER_VALUE(FREESXP);
    INTEGER_VALUE(FUNSXP);
    INTEGER_VALUE(SINGLESXP);

    INTEGER_VALUE(CE_NATIVE);
    INTEGER_VALUE(CE_UTF8);
    INTEGER_VALUE(CE_LATIN1);
    INTEGER_VALUE(CE_BYTES);
    INTEGER_VALUE(CE_SYMBOL);
    INTEGER_VALUE(CE_ANY);

    INTEGER_VALUE(SORTED_DECR_NA_1ST);
    INTEGER_VALUE(SORTED_DECR);
    INTEGER_VALUE(KNOWN_UNSORTED);
    INTEGER_VALUE(SORTED_INCR);
    INTEGER_VALUE(SORTED_INCR_NA_1ST);
    INTEGER_VALUE(UNKNOWN_SORTEDNESS);

    INTEGER_VALUE(R_BindingTypeUnbound);
    INTEGER_VALUE(R_BindingTypeValue);
    INTEGER_VALUE(R_BindingTypeMissing);
    INTEGER_VALUE(R_BindingTypeDelayed);
    INTEGER_VALUE(R_BindingTypeForced);
    INTEGER_VALUE(R_BindingTypeActive);
    INTEGER_VALUE(R_DotTypeValue);
    INTEGER_VALUE(R_DotTypeMissing);
    INTEGER_VALUE(R_DotTypeDelayed);
    INTEGER_VALUE(R_DotTypeForced);

    INTEGER_VALUE(R_pstream_any_format);
    INTEGER_VALUE(R_pstream_ascii_format);
    INTEGER_VALUE(R_pstream_binary_format);
    INTEGER_VALUE(R_pstream_xdr_format);
    INTEGER_VALUE(R_pstream_asciihex_format);

    INTEGER_VALUE(PARSE_NULL);
    INTEGER_VALUE(PARSE_OK);
    INTEGER_VALUE(PARSE_INCOMPLETE);
    INTEGER_VALUE(PARSE_ERROR);
    INTEGER_VALUE(PARSE_EOF);

    printf("type.SEXPTYPE.unsigned_int=%d\n", _Generic((SEXPTYPE)0, unsigned int: 1, default: 0));
    printf("type.R_NativePrimitiveArgType.unsigned_int=%d\n",
           _Generic((R_NativePrimitiveArgType)0, unsigned int: 1, default: 0));
    INTEGER_VALUE(R_VERSION);
    INTEGER_VALUE(R_Version(4, 6, 2));
    INTEGER_VALUE(R_XLEN_T_MAX);
    INTEGER_VALUE(R_LEN_T_MAX);
    INTEGER_VALUE(R_SHORT_LEN_MAX);
    INTEGER_VALUE(NA_INTEGER);
    INTEGER_VALUE(NA_LOGICAL);
    R_NaInt = INT_MIN + 1;
    printf("binding.NA_INTEGER=%d\n", NA_INTEGER == R_NaInt);
    printf("binding.NA_LOGICAL=%d\n", NA_LOGICAL == R_NaInt);

    REAL_VALUE(M_E);
    REAL_VALUE(M_LOG2E);
    REAL_VALUE(M_LOG10E);
    REAL_VALUE(M_LN2);
    REAL_VALUE(M_LN10);
    REAL_VALUE(M_PI);
    REAL_VALUE(M_PI_2);
    REAL_VALUE(M_PI_4);
    REAL_VALUE(M_1_PI);
    REAL_VALUE(M_2_PI);
    REAL_VALUE(M_2_SQRTPI);
    REAL_VALUE(M_SQRT2);
    REAL_VALUE(M_SQRT1_2);
    REAL_VALUE(M_SQRT_3);
    REAL_VALUE(M_SQRT_32);
    REAL_VALUE(M_LOG10_2);
    REAL_VALUE(M_2PI);
    REAL_VALUE(M_SQRT_PI);
    REAL_VALUE(M_1_SQRT_2PI);
    REAL_VALUE(M_SQRT_2dPI);
    REAL_VALUE(M_LN_SQRT_PI);
    REAL_VALUE(M_LN_SQRT_2PI);
    REAL_VALUE(M_LN_SQRT_PId2);
    return 0;
}

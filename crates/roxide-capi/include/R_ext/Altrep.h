/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_ALTREP_H
#define ROXIDE_R_EXT_ALTREP_H

#include <Rinternals.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct { SEXP ptr; } R_altrep_class_t;

#define R_SEXP(class_) ((class_).ptr)
#define R_SUBTYPE_INIT(value) { (value) }

SEXP R_new_altrep(R_altrep_class_t klass, SEXP data1, SEXP data2);
Rboolean R_altrep_inherits(SEXP x, R_altrep_class_t klass);
R_xlen_t ALTREP_LENGTH(SEXP x);
SEXP ALTSTRING_ELT(SEXP x, R_xlen_t i);
void *ALTVEC_DATAPTR(SEXP x);
const void *ALTVEC_DATAPTR_RO(SEXP x);
R_altrep_class_t R_make_altstring_class(const char *name, const char *package, DllInfo *info);
R_altrep_class_t R_make_altinteger_class(const char *name, const char *package, DllInfo *info);
R_altrep_class_t R_make_altreal_class(const char *name, const char *package, DllInfo *info);
R_altrep_class_t R_make_altlogical_class(const char *name, const char *package, DllInfo *info);
R_altrep_class_t R_make_altraw_class(const char *name, const char *package, DllInfo *info);
R_altrep_class_t R_make_altcomplex_class(const char *name, const char *package, DllInfo *info);
R_altrep_class_t R_make_altlist_class(const char *name, const char *package, DllInfo *info);

typedef Rcomplex (*R_altcomplex_Elt_method_t)(SEXP, R_xlen_t);
void R_set_altcomplex_Elt_method(R_altrep_class_t klass, R_altcomplex_Elt_method_t method);
typedef R_xlen_t (*R_altcomplex_Get_region_method_t)(SEXP, R_xlen_t, R_xlen_t, Rcomplex *);
void R_set_altcomplex_Get_region_method(R_altrep_class_t klass, R_altcomplex_Get_region_method_t method);
typedef int (*R_altinteger_Elt_method_t)(SEXP, R_xlen_t);
void R_set_altinteger_Elt_method(R_altrep_class_t klass, R_altinteger_Elt_method_t method);
typedef R_xlen_t (*R_altinteger_Get_region_method_t)(SEXP, R_xlen_t, R_xlen_t, int *);
void R_set_altinteger_Get_region_method(R_altrep_class_t klass, R_altinteger_Get_region_method_t method);
typedef int (*R_altinteger_Is_sorted_method_t)(SEXP);
void R_set_altinteger_Is_sorted_method(R_altrep_class_t klass, R_altinteger_Is_sorted_method_t method);
typedef SEXP (*R_altinteger_Max_method_t)(SEXP, Rboolean);
void R_set_altinteger_Max_method(R_altrep_class_t klass, R_altinteger_Max_method_t method);
typedef SEXP (*R_altinteger_Min_method_t)(SEXP, Rboolean);
void R_set_altinteger_Min_method(R_altrep_class_t klass, R_altinteger_Min_method_t method);
typedef int (*R_altinteger_No_NA_method_t)(SEXP);
void R_set_altinteger_No_NA_method(R_altrep_class_t klass, R_altinteger_No_NA_method_t method);
typedef SEXP (*R_altinteger_Sum_method_t)(SEXP, Rboolean);
void R_set_altinteger_Sum_method(R_altrep_class_t klass, R_altinteger_Sum_method_t method);
typedef SEXP (*R_altlist_Elt_method_t)(SEXP, R_xlen_t);
void R_set_altlist_Elt_method(R_altrep_class_t klass, R_altlist_Elt_method_t method);
typedef void (*R_altlist_Set_elt_method_t)(SEXP, R_xlen_t, SEXP);
void R_set_altlist_Set_elt_method(R_altrep_class_t klass, R_altlist_Set_elt_method_t method);
typedef int (*R_altlogical_Elt_method_t)(SEXP, R_xlen_t);
void R_set_altlogical_Elt_method(R_altrep_class_t klass, R_altlogical_Elt_method_t method);
typedef R_xlen_t (*R_altlogical_Get_region_method_t)(SEXP, R_xlen_t, R_xlen_t, int *);
void R_set_altlogical_Get_region_method(R_altrep_class_t klass, R_altlogical_Get_region_method_t method);
typedef int (*R_altlogical_Is_sorted_method_t)(SEXP);
void R_set_altlogical_Is_sorted_method(R_altrep_class_t klass, R_altlogical_Is_sorted_method_t method);
typedef int (*R_altlogical_No_NA_method_t)(SEXP);
void R_set_altlogical_No_NA_method(R_altrep_class_t klass, R_altlogical_No_NA_method_t method);
typedef SEXP (*R_altlogical_Sum_method_t)(SEXP, Rboolean);
void R_set_altlogical_Sum_method(R_altrep_class_t klass, R_altlogical_Sum_method_t method);
typedef Rbyte (*R_altraw_Elt_method_t)(SEXP, R_xlen_t);
void R_set_altraw_Elt_method(R_altrep_class_t klass, R_altraw_Elt_method_t method);
typedef R_xlen_t (*R_altraw_Get_region_method_t)(SEXP, R_xlen_t, R_xlen_t, Rbyte *);
void R_set_altraw_Get_region_method(R_altrep_class_t klass, R_altraw_Get_region_method_t method);
typedef double (*R_altreal_Elt_method_t)(SEXP, R_xlen_t);
void R_set_altreal_Elt_method(R_altrep_class_t klass, R_altreal_Elt_method_t method);
typedef R_xlen_t (*R_altreal_Get_region_method_t)(SEXP, R_xlen_t, R_xlen_t, double *);
void R_set_altreal_Get_region_method(R_altrep_class_t klass, R_altreal_Get_region_method_t method);
typedef int (*R_altreal_Is_sorted_method_t)(SEXP);
void R_set_altreal_Is_sorted_method(R_altrep_class_t klass, R_altreal_Is_sorted_method_t method);
typedef SEXP (*R_altreal_Max_method_t)(SEXP, Rboolean);
void R_set_altreal_Max_method(R_altrep_class_t klass, R_altreal_Max_method_t method);
typedef SEXP (*R_altreal_Min_method_t)(SEXP, Rboolean);
void R_set_altreal_Min_method(R_altrep_class_t klass, R_altreal_Min_method_t method);
typedef int (*R_altreal_No_NA_method_t)(SEXP);
void R_set_altreal_No_NA_method(R_altrep_class_t klass, R_altreal_No_NA_method_t method);
typedef SEXP (*R_altreal_Sum_method_t)(SEXP, Rboolean);
void R_set_altreal_Sum_method(R_altrep_class_t klass, R_altreal_Sum_method_t method);
typedef SEXP (*R_altrep_Coerce_method_t)(SEXP, int);
void R_set_altrep_Coerce_method(R_altrep_class_t klass, R_altrep_Coerce_method_t method);
typedef SEXP (*R_altrep_Duplicate_method_t)(SEXP, Rboolean);
void R_set_altrep_Duplicate_method(R_altrep_class_t klass, R_altrep_Duplicate_method_t method);
typedef SEXP (*R_altrep_DuplicateEX_method_t)(SEXP, Rboolean);
void R_set_altrep_DuplicateEX_method(R_altrep_class_t klass, R_altrep_DuplicateEX_method_t method);
typedef Rboolean (*R_altrep_Inspect_method_t)(SEXP, int, int, int, void (*)(SEXP, int, int, int));
void R_set_altrep_Inspect_method(R_altrep_class_t klass, R_altrep_Inspect_method_t method);
typedef R_xlen_t (*R_altrep_Length_method_t)(SEXP);
void R_set_altrep_Length_method(R_altrep_class_t klass, R_altrep_Length_method_t method);
typedef SEXP (*R_altrep_Serialized_state_method_t)(SEXP);
void R_set_altrep_Serialized_state_method(R_altrep_class_t klass, R_altrep_Serialized_state_method_t method);
typedef SEXP (*R_altrep_Unserialize_method_t)(SEXP, SEXP);
void R_set_altrep_Unserialize_method(R_altrep_class_t klass, R_altrep_Unserialize_method_t method);
typedef SEXP (*R_altrep_UnserializeEX_method_t)(SEXP, SEXP, SEXP, int, int);
void R_set_altrep_UnserializeEX_method(R_altrep_class_t klass, R_altrep_UnserializeEX_method_t method);
typedef SEXP (*R_altstring_Elt_method_t)(SEXP, R_xlen_t);
void R_set_altstring_Elt_method(R_altrep_class_t klass, R_altstring_Elt_method_t method);
typedef int (*R_altstring_Is_sorted_method_t)(SEXP);
void R_set_altstring_Is_sorted_method(R_altrep_class_t klass, R_altstring_Is_sorted_method_t method);
typedef int (*R_altstring_No_NA_method_t)(SEXP);
void R_set_altstring_No_NA_method(R_altrep_class_t klass, R_altstring_No_NA_method_t method);
typedef void (*R_altstring_Set_elt_method_t)(SEXP, R_xlen_t, SEXP);
void R_set_altstring_Set_elt_method(R_altrep_class_t klass, R_altstring_Set_elt_method_t method);
typedef void *(*R_altvec_Dataptr_method_t)(SEXP, Rboolean);
void R_set_altvec_Dataptr_method(R_altrep_class_t klass, R_altvec_Dataptr_method_t method);
typedef const void *(*R_altvec_Dataptr_or_null_method_t)(SEXP);
void R_set_altvec_Dataptr_or_null_method(R_altrep_class_t klass, R_altvec_Dataptr_or_null_method_t method);
typedef SEXP (*R_altvec_Extract_subset_method_t)(SEXP, SEXP, SEXP);
void R_set_altvec_Extract_subset_method(R_altrep_class_t klass, R_altvec_Extract_subset_method_t method);

#ifdef __cplusplus
}
#endif

#endif

/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_RDEFINES_H
#define ROXIDE_RDEFINES_H

#include <Rinternals.h>

#ifdef __cplusplus
extern "C" {
#endif

#define NEW_LOGICAL(n) Rf_allocVector(LGLSXP, (n))
#define NEW_INTEGER(n) Rf_allocVector(INTSXP, (n))
#define NEW_NUMERIC(n) Rf_allocVector(REALSXP, (n))
#define NEW_COMPLEX(n) Rf_allocVector(CPLXSXP, (n))
#define NEW_CHARACTER(n) Rf_allocVector(STRSXP, (n))
#define NEW_RAW(n) Rf_allocVector(RAWSXP, (n))
#define NEW_LIST(n) Rf_allocVector(VECSXP, (n))
#define NEW_ENV(parent, hash, n) R_NewEnv((parent), (hash), (n))
#define NEW_OBJECT(class_) R_do_new_object(class_)
#define MAKE_CLASS(name) R_do_MAKE_CLASS(name)
#define GET_CLASS_DEF(name) R_getClassDef(name)
#define GET_SLOT(x, name) R_do_slot((x), (name))
#define SET_SLOT(x, name, value) R_do_slot_assign((x), (name), (value))
#define HAS_SLOT(x, name) R_has_slot((x), (name))

#define AS_LOGICAL(x) Rf_coerceVector((x), LGLSXP)
#define AS_INTEGER(x) Rf_coerceVector((x), INTSXP)
#define AS_NUMERIC(x) Rf_coerceVector((x), REALSXP)
#define AS_COMPLEX(x) Rf_coerceVector((x), CPLXSXP)
#define AS_CHARACTER(x) Rf_coerceVector((x), STRSXP)
#define AS_RAW(x) Rf_coerceVector((x), RAWSXP)
#define AS_LIST(x) Rf_coerceVector((x), VECSXP)

#define IS_LOGICAL(x) Rf_isLogical(x)
#define IS_INTEGER(x) Rf_isInteger(x)
#define IS_NUMERIC(x) Rf_isReal(x)
#define IS_COMPLEX(x) Rf_isComplex(x)
#define IS_CHARACTER(x) Rf_isString(x)
#define IS_RAW(x) (TYPEOF(x) == RAWSXP)
#define IS_LIST(x) Rf_isNewList(x)

#define LOGICAL_POINTER(x) LOGICAL(x)
#define INTEGER_POINTER(x) INTEGER(x)
#define NUMERIC_POINTER(x) REAL(x)
#define COMPLEX_POINTER(x) COMPLEX(x)
#define RAW_POINTER(x) RAW(x)
#define GET_LENGTH(x) Rf_length(x)
#define GET_NAMES(x) Rf_getAttrib((x), R_NamesSymbol)
#define GET_DIM(x) Rf_getAttrib((x), R_DimSymbol)
#define GET_DIMNAMES(x) Rf_getAttrib((x), R_DimNamesSymbol)
#define GET_CLASS(x) Rf_getAttrib((x), R_ClassSymbol)
#define GET_LEVELS(x) Rf_getAttrib((x), R_LevelsSymbol)
#define GET_ATTR(x, name) Rf_getAttrib((x), (name))
#define SET_LENGTH(x, n) Rf_lengthgets((x), (n))
#define SET_NAMES(x, names) Rf_setAttrib((x), R_NamesSymbol, (names))
#define SET_DIM(x, dims) Rf_setAttrib((x), R_DimSymbol, (dims))
#define SET_DIMNAMES(x, names) Rf_setAttrib((x), R_DimNamesSymbol, (names))
#define SET_CLASS(x, klass) Rf_setAttrib((x), R_ClassSymbol, (klass))
#define SET_LEVELS(x, levels) Rf_setAttrib((x), R_LevelsSymbol, (levels))
#define SET_ATTR(x, name, value) Rf_setAttrib((x), (name), (value))
#define DUPLICATE(x) Rf_duplicate(x)
#define AS_VECTOR(x) Rf_coerceVector((x), VECSXP)
#define COPY_ATTRIBUTES(x, y) Rf_copyMostAttrib((x), (y))
#define GET_STRING_ELT(x, i) CHAR(STRING_ELT((x), (i)))
#define COPY_ATTRIB(x, y) DUPLICATE_ATTRIB((y), (x))
#define R_NaChar R_NaString
#define NA_NUMERIC NA_REAL
#define NA_CHARACTER NA_STRING
#define NA_STRING_P(x) ((x) == NA_STRING)
#define NULL_USER_OBJECT R_NilValue
#define R_EOF (-1)
typedef SEXP USER_OBJECT_;

#ifdef __cplusplus
}
#endif

#endif

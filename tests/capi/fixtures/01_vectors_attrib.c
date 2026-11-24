/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#define R_NO_REMAP
#include <Rinternals.h>
#include <string.h>

static void check(int ok, const char *operation)
{
    if (!ok)
        Rf_error("Vector fixture check failed: %s", operation);
}

SEXP fixture_vectors(void)
{
    SEXP result = Rf_protect(Rf_allocVector(VECSXP, 17));
    SEXP dimensions = Rf_protect(Rf_allocVector(INTSXP, 3));
    INTEGER(dimensions)[0] = 2;
    INTEGER(dimensions)[1] = 1;
    INTEGER(dimensions)[2] = 2;

    SEXP matrix = Rf_protect(Rf_allocMatrix(INTSXP, 2, 2));
    SEXP array = Rf_protect(Rf_allocArray(REALSXP, dimensions));
    for (int i = 0; i < 4; ++i) {
        INTEGER(matrix)[i] = i + 1;
        REAL(array)[i] = i + 0.5;
    }
    SET_VECTOR_ELT(result, 0, matrix);
    SET_VECTOR_ELT(result, 1, array);
    check(Rf_length(matrix) == 4 && Rf_xlength(array) == 4, "length");

    SEXP logical = Rf_protect(Rf_allocVector(LGLSXP, 2));
    LOGICAL(logical)[0] = TRUE;
    LOGICAL(logical)[1] = NA_LOGICAL;
    SET_VECTOR_ELT(result, 2, logical);

    SEXP raw = Rf_protect(Rf_allocVector(RAWSXP, 2));
    RAW(raw)[0] = 0x00;
    RAW(raw)[1] = 0xff;
    SET_VECTOR_ELT(result, 3, raw);

    SEXP complex = Rf_protect(Rf_allocVector(CPLXSXP, 1));
    COMPLEX(complex)[0] = (Rcomplex){2, -3};
    SET_VECTOR_ELT(result, 4, complex);

    SEXP chars = Rf_protect(Rf_allocVector(STRSXP, 2));
    SEXP latin = Rf_protect(Rf_mkCharLenCE("caf\xe9", 4, CE_LATIN1));
    SET_STRING_ELT(chars, 0, latin);
    check(strcmp(CHAR(STRING_ELT(chars, 0)), "caf\xe9") == 0, "Latin-1 bytes");
    const char *utf8 = Rf_translateCharUTF8(latin);
    check(strcmp(utf8, "caf\xc3\xa9") == 0, "UTF-8 translation");
    SET_STRING_ELT(chars, 1, Rf_mkCharLenCE(utf8, 5, CE_UTF8));
    SET_VECTOR_ELT(result, 5, chars);

    SEXP label = Rf_install("fixture_label");
    check(strcmp(CHAR(PRINTNAME(label)), "fixture_label") == 0, "symbol name");
    SEXP attribute = Rf_protect(Rf_ScalarString(STRING_ELT(chars, 1)));
    Rf_setAttrib(result, label, attribute);
    check(Rf_getAttrib(result, label) == attribute, "attribute lookup");

    SEXP source = Rf_protect(Rf_allocVector(VECSXP, 1));
    SET_VECTOR_ELT(source, 0, matrix);
    SEXP deep = Rf_protect(Rf_duplicate(source));
    SEXP shallow = Rf_protect(Rf_shallow_duplicate(source));
    check(deep != source && VECTOR_ELT(deep, 0) != matrix, "deep copy");
    check(shallow != source && VECTOR_ELT(shallow, 0) == matrix, "shallow copy");
    INTEGER(VECTOR_ELT(deep, 0))[0] = 99;
    SET_VECTOR_ELT(result, 6, deep);
    SET_VECTOR_ELT(result, 7, shallow);
    SET_VECTOR_ELT(result, 8, Rf_coerceVector(matrix, REALSXP));

    PROTECT_INDEX index;
    SEXP first = Rf_protect(Rf_ScalarInteger(7));
    R_ProtectWithIndex(Rf_ScalarInteger(8), &index);
    SEXP second = Rf_ScalarReal(9.5);
    R_Reprotect(second, index);

    SEXP tail = Rf_protect(Rf_cons(second, R_NilValue));
    SET_TAG(tail, label);
    SEXP pair = Rf_protect(Rf_cons(first, tail));
    check(CAR(pair) == first && CDR(pair) == tail && TAG(tail) == label,
          "pairlist structure");
    SET_VECTOR_ELT(result, 9, pair);

    SEXP fun = Rf_install("list");
    SET_VECTOR_ELT(result, 10, Rf_lcons(fun, pair));
    SET_VECTOR_ELT(result, 11, Rf_lang1(fun));
    SET_VECTOR_ELT(result, 12, Rf_lang2(fun, first));
    SET_VECTOR_ELT(result, 13, Rf_lang3(fun, first, second));
    SEXP third = Rf_protect(Rf_ScalarLogical(TRUE));
    SET_VECTOR_ELT(result, 14, Rf_lang4(fun, first, second, third));
    SET_VECTOR_ELT(result, 15, Rf_ScalarRaw(0xa5));
    SET_VECTOR_ELT(result, 16, Rf_ScalarComplex((Rcomplex){4, 5}));

    Rf_unprotect_ptr(first);
    Rf_unprotect(17);
    return result;
}

SEXP fixture_imbalanced(void)
{
    return Rf_protect(Rf_ScalarInteger(123));
}

//! Exercise vector, attribute, coercion, pairlist, string, symbol and resize APIs from C.

use std::ffi::{c_char, c_int, c_void};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::ptr::from_mut;
use std::sync::OnceLock;

use libloading::Library;
use roxide_capi::abi::*;
use roxide_capi::api::{
    attrib::*, coerce::*, error::R_CheckUserInterrupt, memory::*, pairlist::*, resizable::*,
    string::*, sym::*, vector::*,
};
use roxide_capi::globals::{R_ClassSymbol, R_NaString, R_NamesSymbol, R_NilValue};
use roxide_capi::jump::{Rf_protect, Rf_unprotect, VoidFn, enter_trampoline};
use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::ValueApi;

// Use the clean-room header to type-check the C half of the table. Passing
// function pointers avoids platform-specific exports from the Rust test binary.
macro_rules! fixture_api {
    ($($name:ident($($arg:ty),*) $(-> $ret:ty)?);+ $(;)?) => {
        #[allow(non_snake_case)]
        #[repr(C)]
        struct Api {
            $($name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)+
        }

        static API: Api = Api { $($name,)+ };

        const C_API: &str = concat!(
            "#define R_NO_REMAP\n#include <Rinternals.h>\n#include <R_ext/Utils.h>\nstruct Api {\n",
            $("__typeof__(&", stringify!($name), ") ", stringify!($name), ";\n",)+
            "};\n",
        );
    }
}

fixture_api! {
    Rf_protect(Sexp) -> Sexp;
    Rf_unprotect(c_int);
    Rf_allocVector(SEXPTYPE, R_xlen_t) -> Sexp;
    Rf_allocMatrix(SEXPTYPE, c_int, c_int) -> Sexp;
    Rf_alloc3DArray(SEXPTYPE, c_int, c_int, c_int) -> Sexp;
    Rf_allocArray(SEXPTYPE, Sexp) -> Sexp;
    Rf_nrows(Sexp) -> c_int;
    Rf_ncols(Sexp) -> c_int;
    TYPEOF(Sexp) -> c_int;
    LENGTH(Sexp) -> c_int;
    XLENGTH(Sexp) -> R_xlen_t;
    INTEGER(Sexp) -> *mut c_int;
    INTEGER_RO(Sexp) -> *const c_int;
    INTEGER_ELT(Sexp, R_xlen_t) -> c_int;
    SET_INTEGER_ELT(Sexp, R_xlen_t, c_int);
    INTEGER_GET_REGION(Sexp, R_xlen_t, R_xlen_t, *mut c_int) -> R_xlen_t;
    INTEGER_IS_SORTED(Sexp) -> c_int;
    INTEGER_NO_NA(Sexp) -> c_int;
    REAL_ELT(Sexp, R_xlen_t) -> f64;
    LOGICAL_ELT(Sexp, R_xlen_t) -> c_int;
    RAW_ELT(Sexp, R_xlen_t) -> Rbyte;
    COMPLEX_ELT(Sexp, R_xlen_t) -> Rcomplex;
    SET_COMPLEX_ELT(Sexp, R_xlen_t, Rcomplex);
    COMPLEX_GET_REGION(Sexp, R_xlen_t, R_xlen_t, *mut Rcomplex) -> R_xlen_t;
    SET_LOGICAL_ELT(Sexp, R_xlen_t, c_int);
    LOGICAL_GET_REGION(Sexp, R_xlen_t, R_xlen_t, *mut c_int) -> R_xlen_t;
    SET_RAW_ELT(Sexp, R_xlen_t, Rbyte);
    RAW_GET_REGION(Sexp, R_xlen_t, R_xlen_t, *mut Rbyte) -> R_xlen_t;
    SET_REAL_ELT(Sexp, R_xlen_t, f64);
    REAL_GET_REGION(Sexp, R_xlen_t, R_xlen_t, *mut f64) -> R_xlen_t;
    SET_VECTOR_ELT(Sexp, R_xlen_t, Sexp) -> Sexp;
    VECTOR_ELT(Sexp, R_xlen_t) -> Sexp;
    DATAPTR_RO(Sexp) -> *const c_void;
    DATAPTR_OR_NULL(Sexp) -> *const c_void;
    Rf_isMatrix(Sexp) -> Rboolean;
    Rf_isArray(Sexp) -> Rboolean;
    Rf_isVectorizable(Sexp) -> Rboolean;
    Rf_isFactor(Sexp) -> Rboolean;
    Rf_type2str(SEXPTYPE) -> Sexp;
    Rf_str2type(*const c_char) -> c_int;
    MARK_NOT_MUTABLE(Sexp);
    MAYBE_SHARED(Sexp) -> c_int;
    Rf_mkCharLenCE(*const c_char, c_int, c_int) -> Sexp;
    Rf_translateChar(Sexp) -> *const c_char;
    vmaxget() -> *mut c_void;
    vmaxset(*const c_void);
    R_alloc(usize, c_int) -> *mut c_char;
    Rf_isBlankString(*const c_char) -> Rboolean;
    Rf_StringBlank(Sexp) -> Rboolean;
    Rf_StringTrue(*const c_char) -> Rboolean;
    Rf_StringFalse(*const c_char) -> Rboolean;
    Rf_pmatch(Sexp, Sexp, Rboolean) -> Rboolean;
    Rf_psmatch(*const c_char, *const c_char, Rboolean) -> Rboolean;
    R_chk_memcpy(*mut c_void, *const c_void, usize) -> *mut c_void;
    R_chk_memset(*mut c_void, c_int, usize) -> *mut c_void;
    R_ExpandFileName(*const c_char) -> *const c_char;
    R_strtod(*const c_char, *mut *mut c_char) -> f64;
    R_atof(*const c_char) -> f64;
    R_tmpnam(*const c_char, *const c_char) -> *mut c_char;
    R_tmpnam2(*const c_char, *const c_char, *const c_char) -> *mut c_char;
    R_free_tmpnam(*mut c_char);
    acopy_string(*const c_char) -> *const c_char;
    R_CheckUserInterrupt();
    Rf_getCharCE(Sexp) -> c_int;
    Rf_mkChar(*const c_char) -> Sexp;
    R_CHAR(Sexp) -> *const c_char;
    Rf_translateCharUTF8(Sexp) -> *const c_char;
    Rf_reEnc(*const c_char, c_int, c_int, c_int) -> *const c_char;
    STRING_ELT(Sexp, R_xlen_t) -> Sexp;
    SET_STRING_ELT(Sexp, R_xlen_t, Sexp);
    STRING_PTR_RO(Sexp) -> *const Sexp;
    Rf_install(*const c_char) -> Sexp;
    Rf_installChar(Sexp) -> Sexp;
    Rf_installTrChar(Sexp) -> Sexp;
    PRINTNAME(Sexp) -> Sexp;
    Rf_cons(Sexp, Sexp) -> Sexp;
    Rf_allocList(c_int) -> Sexp;
    Rf_lang3(Sexp, Sexp, Sexp) -> Sexp;
    CAR(Sexp) -> Sexp;
    CDR(Sexp) -> Sexp;
    TAG(Sexp) -> Sexp;
    SET_TAG(Sexp, Sexp);
    SETCAR(Sexp, Sexp) -> Sexp;
    SETCADDR(Sexp, Sexp) -> Sexp;
    CADDR(Sexp) -> Sexp;
    Rf_elt(Sexp, c_int) -> Sexp;
    Rf_listAppend(Sexp, Sexp) -> Sexp;
    Rf_lastElt(Sexp) -> Sexp;
    Rf_coerceVector(Sexp, SEXPTYPE) -> Sexp;
    Rf_ScalarInteger(c_int) -> Sexp;
    Rf_ScalarReal(f64) -> Sexp;
    Rf_ScalarComplex(Rcomplex) -> Sexp;
    Rf_ScalarLogical(c_int) -> Sexp;
    Rf_ScalarRaw(Rbyte) -> Sexp;
    Rf_ScalarString(Sexp) -> Sexp;
    Rf_asChar(Sexp) -> Sexp;
    Rf_asLogical(Sexp) -> c_int;
    Rf_asInteger(Sexp) -> c_int;
    Rf_asReal(Sexp) -> f64;
    Rf_mkString(*const c_char) -> Sexp;
    Rf_VectorToPairList(Sexp) -> Sexp;
    Rf_PairToVectorList(Sexp) -> Sexp;
    Rf_duplicate(Sexp) -> Sexp;
    Rf_shallow_duplicate(Sexp) -> Sexp;
    Rf_copyMostAttrib(Sexp, Sexp);
    Rf_getAttrib(Sexp, Sexp) -> Sexp;
    Rf_setAttrib(Sexp, Sexp, Sexp) -> Sexp;
    Rf_namesgets(Sexp, Sexp) -> Sexp;
    Rf_classgets(Sexp, Sexp) -> Sexp;
    Rf_inherits(Sexp, *const c_char) -> Rboolean;
    R_getAttribNames(Sexp) -> Sexp;
    R_getAttributes(Sexp) -> Sexp;
    R_getAttribCount(Sexp) -> c_int;
    R_hasAttrib(Sexp, Sexp) -> bool;
    CLEAR_ATTRIB(Sexp);
    ANY_ATTRIB(Sexp) -> c_int;
    Rf_asCharacterFactor(Sexp) -> Sexp;
    Rf_nlevels(Sexp) -> c_int;
    R_allocResizableVector(SEXPTYPE, R_xlen_t) -> Sexp;
    R_resizeVector(Sexp, R_xlen_t);
    R_isResizable(Sexp) -> bool;
    R_maxLength(Sexp) -> R_xlen_t;
    R_duplicateAsResizable(Sexp) -> Sexp;
    Rf_xlengthgets(Sexp, R_xlen_t) -> Sexp;
}

#[repr(C)]
struct State {
    api: *const Api,
    nil: Sexp,
    missing_string: Sexp,
    names: Sexp,
    class: Sexp,
    input: Sexp,
    line: c_int,
}

const C_FIXTURE: &str = r#"
#include <math.h>
#include <string.h>

struct State {
    const struct Api *api;
    SEXP nil, missing_string, names, class, input;
    int line;
};
#define A (s->api)
#define CHECK(e) do { if (!(e)) { s->line = __LINE__; return; } } while (0)
#undef PROTECT
#undef UNPROTECT
#define PROTECT(x) A->Rf_protect(x)
#define UNPROTECT(n) A->Rf_unprotect(n)

void vectors(void *data) {
    struct State *s = data;
    SEXP x = PROTECT(A->Rf_allocVector(INTSXP, 3));
    int *ptr = A->INTEGER(x);
    ptr[0] = 10;
    A->SET_INTEGER_ELT(x, 1, 20);
    int out[3] = {-1, -1, -1};
    CHECK(A->INTEGER_GET_REGION(x, 1, 3, out) == 2);
    CHECK(out[0] == 20 && out[1] == 0 && out[2] == -1);
    CHECK(A->INTEGER_RO(x) == ptr && A->DATAPTR_RO(x) == ptr);
    CHECK(A->LENGTH(x) == 3 && A->XLENGTH(x) == 3);
    CHECK(A->TYPEOF(x) == INTSXP && A->Rf_str2type("integer") == INTSXP);
    CHECK(strcmp(A->R_CHAR(A->Rf_type2str(INTSXP)), "integer") == 0);
    CHECK(!A->MAYBE_SHARED(x));
    A->MARK_NOT_MUTABLE(x);
    CHECK(A->MAYBE_SHARED(x));

    SEXP m = PROTECT(A->Rf_allocMatrix(REALSXP, 2, 3));
    CHECK(A->Rf_isMatrix(m) && A->Rf_isArray(m));
    CHECK(A->Rf_nrows(m) == 2 && A->Rf_ncols(m) == 3 && A->REAL_ELT(m, 5) == 0);
    A->SET_REAL_ELT(m, 5, 1.25);
    double real[2] = {-1.0, -1.0};
    CHECK(A->REAL_GET_REGION(m, 5, 2, real) == 1);
    CHECK(real[0] == 1.25 && real[1] == -1.0);
    SEXP arr = PROTECT(A->Rf_alloc3DArray(LGLSXP, 2, 3, 4));
    CHECK(A->Rf_isArray(arr) && !A->Rf_isMatrix(arr));
    CHECK(A->XLENGTH(arr) == 24 && !A->LOGICAL_ELT(arr, 0));
    A->SET_LOGICAL_ELT(arr, 23, TRUE);
    int logical[2] = {-1, -1};
    CHECK(A->LOGICAL_GET_REGION(arr, 23, 2, logical) == 1);
    CHECK(logical[0] == TRUE && logical[1] == -1);
    SEXP dim = A->Rf_allocVector(INTSXP, 2);
    A->SET_INTEGER_ELT(dim, 0, 3);
    A->SET_INTEGER_ELT(dim, 1, 5);
    // The allocating API must protect the unprotected dimension argument.
    SEXP from_dims = PROTECT(A->Rf_allocArray(RAWSXP, dim));
    CHECK(A->XLENGTH(from_dims) == 15 && A->RAW_ELT(from_dims, 0) == 0);
    A->SET_RAW_ELT(from_dims, 14, 255);
    Rbyte raw[2] = {1, 1};
    CHECK(A->RAW_GET_REGION(from_dims, 14, 2, raw) == 1);
    CHECK(raw[0] == 255 && raw[1] == 1);

    SEXP expr = PROTECT(A->Rf_allocVector(EXPRSXP, 1));
    CHECK(A->SET_VECTOR_ELT(expr, 0, x) == x && A->VECTOR_ELT(expr, 0) == x);
    CHECK(A->SET_VECTOR_ELT(expr, 0, s->nil) == s->nil);

    SEXP temporary = A->Rf_ScalarInteger(123);
    SEXP result = PROTECT(A->Rf_coerceVector(temporary, REALSXP));
    CHECK(A->REAL_ELT(result, 0) == 123);
    UNPROTECT(6);
}

void compact(void *data) {
    struct State *s = data;
    SEXP x = s->input;
    int out[3] = {0, 0, -1};
    CHECK(A->DATAPTR_OR_NULL(x) == NULL);
    CHECK(A->INTEGER_IS_SORTED(x) == SORTED_DECR && A->INTEGER_NO_NA(x));
    CHECK(A->INTEGER_ELT(x, 1) == 5);
    CHECK(A->INTEGER_GET_REGION(x, 2, 3, out) == 2);
    CHECK(out[0] == 4 && out[1] == 3 && out[2] == -1);
    CHECK(A->DATAPTR_OR_NULL(x) == NULL);
    const int *p = A->INTEGER_RO(x);
    CHECK(p[0] == 6 && A->DATAPTR_OR_NULL(x) == p);
    A->INTEGER(x)[0] = 42;
    CHECK(A->INTEGER_ELT(x, 0) == 42 && A->INTEGER_RO(x) == p);
}

void strings_and_symbols(void *data) {
    struct State *s = data;
    SEXP x = PROTECT(A->Rf_allocVector(STRSXP, 2));
    const char latin[] = {'c', 'a', 'f', (char)0xe9};
    SEXP ch = A->Rf_mkCharLenCE(latin, 4, CE_LATIN1);
    A->SET_STRING_ELT(x, 0, ch);
    A->SET_STRING_ELT(x, 1, s->missing_string);
    CHECK(A->STRING_ELT(x, 0) == ch && A->STRING_ELT(x, 1) == s->missing_string);
    CHECK(A->STRING_PTR_RO(x)[0] == ch && A->DATAPTR_RO(x) == A->STRING_PTR_RO(x));
    CHECK(A->Rf_getCharCE(ch) == CE_LATIN1);
    CHECK(memcmp(A->R_CHAR(ch), latin, 4) == 0 && A->R_CHAR(ch)[4] == '\0');
    const char *utf = A->Rf_translateCharUTF8(ch);
    CHECK(strcmp(utf, "caf\xc3\xa9") == 0);
    CHECK(strcmp(A->Rf_reEnc(utf, CE_UTF8, CE_LATIN1, 0), "caf\xe9") == 0);
    SEXP symbol = A->Rf_installTrChar(ch);
    CHECK(symbol == A->Rf_install(utf));
    CHECK(strcmp(A->R_CHAR(A->PRINTNAME(symbol)), utf) == 0);
    SEXP ordinary = A->Rf_mkChar("argument");
    CHECK(A->Rf_installChar(ordinary) == A->Rf_install("argument"));
    UNPROTECT(1);
}

void translation_storage(void *data) {
    struct State *s = data;
    CHECK(A->vmaxget() == NULL);
    SEXP text = PROTECT(A->Rf_mkCharLenCE("\xe9", 1, CE_LATIN1));
    SEXP ascii = PROTECT(A->Rf_mkChar("plain"));
    char *older = A->R_alloc(5, 1);
    strcpy(older, "keep");
    void *mark = A->vmaxget();
    CHECK(A->Rf_translateChar(ascii) == A->R_CHAR(ascii));
    CHECK(A->Rf_translateCharUTF8(ascii) == A->R_CHAR(ascii));
    CHECK(A->vmaxget() == mark);
    const char *native = A->Rf_translateChar(text);
    CHECK(strcmp(native, "\xc3\xa9") == 0 && A->vmaxget() != mark);
    void *translated = A->vmaxget();
    const char *utf8 = A->Rf_translateCharUTF8(text);
    CHECK(A->vmaxget() != translated && strcmp(utf8, native) == 0);
    A->vmaxset(mark);
    CHECK(A->vmaxget() == mark && strcmp(older, "keep") == 0);
    CHECK(strcmp(A->Rf_translateCharUTF8(text), "\xc3\xa9") == 0);
    CHECK(A->vmaxget() != mark);
    UNPROTECT(2);
}

void translation_error(void *data) {
    struct State *s = data;
    CHECK(A->vmaxget() == NULL);
    SEXP text = PROTECT(A->Rf_mkCharLenCE("\xe9", 1, CE_LATIN1));
    CHECK(strcmp(A->Rf_translateCharUTF8(text), "\xc3\xa9") == 0);
    CHECK(A->vmaxget() != NULL);
    SEXP bytes = A->Rf_mkCharLenCE("\xe9", 1, CE_BYTES);
    (void)A->Rf_translateCharUTF8(bytes);
    s->line = __LINE__;
}

void string_utilities(void *data) {
    struct State *s = data;
    CHECK(A->Rf_isBlankString("") && A->Rf_isBlankString(" \t\n\v\f\r"));
    CHECK(!A->Rf_isBlankString(" x "));
    CHECK(A->Rf_StringBlank(s->nil) && A->Rf_StringBlank(A->Rf_mkChar("")));
    CHECK(!A->Rf_StringBlank(s->missing_string) && !A->Rf_StringBlank(A->Rf_mkChar(" ")));
    CHECK(A->Rf_StringBlank(A->Rf_ScalarString(s->missing_string)));
    CHECK(!A->Rf_StringBlank(A->Rf_allocVector(STRSXP, 2)));
    CHECK(A->Rf_StringTrue("T") && A->Rf_StringTrue("true") && !A->Rf_StringTrue("t"));
    CHECK(A->Rf_StringFalse("F") && A->Rf_StringFalse("False") && !A->Rf_StringFalse("FaLsE"));
    CHECK(A->Rf_psmatch("alpha", "al", FALSE) && !A->Rf_psmatch("al", "alpha", FALSE));
    CHECK(!A->Rf_psmatch("alpha", "al", TRUE) && A->Rf_psmatch("alpha", "alpha", TRUE));
    SEXP symbol = A->Rf_install("alpha");
    SEXP pattern = PROTECT(A->Rf_mkString("al"));
    CHECK(A->Rf_pmatch(symbol, pattern, FALSE) && !A->Rf_pmatch(symbol, pattern, TRUE));
    SEXP latin1 = PROTECT(A->Rf_mkCharLenCE("\xe9", 1, CE_LATIN1));
    SEXP utf8 = PROTECT(A->Rf_mkCharLenCE("\xc3\xa9", 2, CE_UTF8));
    CHECK(A->Rf_pmatch(latin1, utf8, TRUE));
    UNPROTECT(3);
}

void c_utilities(void *data) {
    struct State *s = data;
    char target[6] = {0};
    CHECK(A->R_chk_memset(target, 'x', 5) == target);
    CHECK(A->R_chk_memcpy(target + 1, "ab", 2) == target + 1);
    CHECK(strcmp(target, "xabxx") == 0);
    CHECK(strcmp(A->R_ExpandFileName("~/file"), "/work/file") == 0);
    CHECK(strcmp(A->R_ExpandFileName("dir/file"), "dir/file") == 0);
    void *mark = A->vmaxget();
    const char *copy = A->acopy_string(target);
    CHECK(A->vmaxget() != mark && copy != target);
    target[0] = 'y';
    CHECK(strcmp(copy, "xabxx") == 0);
    A->vmaxset(mark);

    const char *number = "  +1.25e2#";
    char *end;
    CHECK(A->R_strtod(number, &end) == 125.0 && *end == '#');
    CHECK(A->R_atof("-0x1.8p+2tail") == -6.0);
    CHECK(A->R_atof("0x1p-1074") == 0x1p-1074);
    CHECK(A->R_atof("0x1p-1075") == 0.0);
    CHECK(A->R_atof("0x1.000000000000001p-1075") == 0x1p-1074);
    CHECK(A->R_atof("0x1.00000000000008p0") == 1.0);
    CHECK(A->R_atof("0x1.0000000000000801p0") == 0x1.0000000000001p0);
    const char *invalid = "1e+";
    CHECK(isnan(A->R_strtod(invalid, &end)) && end == invalid);
    CHECK(isnan(A->R_strtod("nAn(rest)", &end)) && *end == '(');

    char *plain = A->R_tmpnam("check", "/work");
    char *ext = A->R_tmpnam2("check", "/work", ".txt");
    CHECK(strncmp(plain, "/work/check", 11) == 0);
    CHECK(strncmp(ext, "/work/check", 11) == 0);
    CHECK(strlen(ext) >= 15 && strcmp(ext + strlen(ext) - 4, ".txt") == 0);
    A->R_free_tmpnam(plain);
    A->R_free_tmpnam(ext);
    A->R_CheckUserInterrupt();
}

void check_interrupt(void *data) {
    struct State *s = data;
    CHECK(A->vmaxget() == NULL);
    A->R_CheckUserInterrupt();
}

void pairlists(void *data) {
    struct State *s = data;
    SEXP fn = A->Rf_install("sum");
    SEXP a = PROTECT(A->Rf_ScalarInteger(1));
    SEXP b = A->Rf_ScalarInteger(2);
    // b is live on entry, but only Rf_lang3 can protect it before collecting.
    SEXP call = PROTECT(A->Rf_lang3(fn, a, b));
    CHECK(A->TYPEOF(call) == LANGSXP && A->CAR(call) == fn);
    CHECK(A->CADDR(call) == b && A->LENGTH(call) == 3);
    CHECK(A->SETCADDR(call, a) == a && A->CADDR(call) == a);
    SEXP args = A->CDR(call);
    SEXP label = A->Rf_install("first");
    A->SET_TAG(args, label);
    CHECK(A->TAG(args) == label);
    SEXP tags = A->Rf_getAttrib(args, s->names);
    CHECK(strcmp(A->R_CHAR(A->STRING_ELT(tags, 0)), "first") == 0);
    CHECK(A->SETCAR(args, s->nil) == s->nil && A->Rf_elt(args, 0) == s->nil);
    SEXP list = PROTECT(A->Rf_cons(a, s->nil));
    SEXP tail = A->Rf_allocList(1);
    CHECK(A->Rf_listAppend(list, tail) == list);
    CHECK(A->Rf_lastElt(list) == tail && A->LENGTH(list) == 2);
    UNPROTECT(3);
}

void attributes_and_coercion(void *data) {
    struct State *s = data;
    SEXP x = PROTECT(A->Rf_ScalarInteger(1));
    SEXP class = A->Rf_mkString("factor");
    CHECK(A->Rf_classgets(x, class) == x);
    SEXP key = A->Rf_install("levels");
    SEXP levels = A->Rf_mkString("blue");
    CHECK(A->Rf_setAttrib(x, key, levels) == levels);
    CHECK(A->Rf_getAttrib(x, key) == levels && A->Rf_nlevels(x) == 1);
    CHECK(A->Rf_isFactor(x) && A->Rf_inherits(x, "factor"));
    CHECK(A->R_getAttribCount(x) == 2 && A->R_hasAttrib(x, s->class));
    SEXP names = PROTECT(A->R_getAttribNames(x));
    CHECK(strcmp(A->R_CHAR(A->STRING_ELT(names, 0)), "class") == 0);
    CHECK(strcmp(A->R_CHAR(A->STRING_ELT(names, 1)), "levels") == 0);
    SEXP attrs = PROTECT(A->R_getAttributes(x));
    CHECK(A->VECTOR_ELT(attrs, 0) == class && A->VECTOR_ELT(attrs, 1) == levels);
    SEXP converted = PROTECT(A->Rf_asCharacterFactor(x));
    CHECK(strcmp(A->R_CHAR(A->STRING_ELT(converted, 0)), "blue") == 0);
    CHECK(A->Rf_asChar(x) == A->STRING_ELT(converted, 0));
    SEXP duplicate = PROTECT(A->Rf_duplicate(x));
    SEXP shallow = PROTECT(A->Rf_shallow_duplicate(x));
    CHECK(duplicate != x && shallow != x);
    CHECK(A->Rf_getAttrib(duplicate, key) != levels);
    CHECK(A->Rf_getAttrib(shallow, key) == levels);
    A->CLEAR_ATTRIB(duplicate);
    CHECK(!A->ANY_ATTRIB(duplicate) && A->ANY_ATTRIB(x));
    A->Rf_copyMostAttrib(x, duplicate);
    CHECK(A->Rf_inherits(duplicate, "factor"));

    SEXP real = PROTECT(A->Rf_ScalarReal(8.5));
    SEXP label = A->Rf_mkString("amount");
    CHECK(A->Rf_namesgets(real, label) == real);
    SEXP intvec = PROTECT(A->Rf_coerceVector(real, INTSXP));
    CHECK(A->Rf_asInteger(intvec) == 8 && A->Rf_asReal(real) == 8.5);
    CHECK(A->Rf_getAttrib(intvec, s->names) == label);
    SEXP list = PROTECT(A->Rf_VectorToPairList(intvec));
    CHECK(A->Rf_asInteger(A->CAR(list)) == 8);
    SEXP vec = PROTECT(A->Rf_PairToVectorList(list));
    CHECK(A->Rf_isVectorizable(vec) && A->Rf_asInteger(A->VECTOR_ELT(vec, 0)) == 8);
    CHECK(strcmp(A->R_CHAR(A->PRINTNAME(A->TAG(list))), "amount") == 0);
    SEXP cx = A->Rf_ScalarComplex((Rcomplex){1.0, 9.0});
    CHECK(A->COMPLEX_ELT(cx, 0).i == 9.0);
    A->SET_COMPLEX_ELT(cx, 0, (Rcomplex){2.0, -3.0});
    Rcomplex copied;
    CHECK(A->COMPLEX_GET_REGION(cx, 0, 1, &copied) == 1);
    CHECK(copied.r == 2.0 && copied.i == -3.0);
    CHECK(A->Rf_asLogical(A->Rf_ScalarLogical(TRUE)) == 1);
    CHECK(A->RAW_ELT(A->Rf_ScalarRaw(255), 0) == 255);
    SEXP text = A->Rf_ScalarString(A->Rf_mkChar("23"));
    CHECK(A->Rf_asInteger(text) == 23);
    UNPROTECT(10);
}

void resizing(void *data) {
    struct State *s = data;
    SEXP vec = PROTECT(A->R_allocResizableVector(INTSXP, 4));
    CHECK(A->R_isResizable(vec) && A->R_maxLength(vec) == 4 && A->LENGTH(vec) == 4);
    int *ptr = A->INTEGER(vec);
    ptr[0] = 42;
    SEXP names = A->Rf_mkString("first");
    A->Rf_namesgets(vec, names);
    A->R_resizeVector(vec, 1);
    CHECK(A->Rf_getAttrib(vec, s->names) == s->nil);
    CHECK(A->INTEGER(vec) == ptr && A->LENGTH(vec) == 1);
    A->R_resizeVector(vec, 4);
    CHECK(A->INTEGER(vec) == ptr && ptr[0] == 42 && ptr[3] == 0);
    SEXP dup = PROTECT(A->R_duplicateAsResizable(vec));
    CHECK(A->R_isResizable(dup) && A->INTEGER(dup) != ptr && A->INTEGER_ELT(dup, 0) == 42);
    SEXP normal = PROTECT(A->Rf_xlengthgets(dup, 5));
    CHECK(!A->R_isResizable(normal) && A->LENGTH(normal) == 5);
    CHECK(A->INTEGER_ELT(normal, 0) == 42 && A->INTEGER_ELT(normal, 4) == INT_MIN);

    SEXP chars = PROTECT(A->R_allocResizableVector(STRSXP, 2));
    const SEXP *strings = A->STRING_PTR_RO(chars);
    SEXP value = A->Rf_mkChar("keep");
    A->SET_STRING_ELT(chars, 0, value);
    A->SET_STRING_ELT(chars, 1, s->missing_string);
    A->R_resizeVector(chars, 0);
    CHECK(A->STRING_PTR_RO(chars) == strings && A->LENGTH(chars) == 0);
    A->R_resizeVector(chars, 2);
    CHECK(A->STRING_PTR_RO(chars) == strings && A->LENGTH(chars) == 2);
    CHECK(strcmp(A->R_CHAR(A->STRING_ELT(chars, 1)), "") == 0);
    UNPROTECT(4);
}

void wrong_pointer(void *data) {
    struct State *s = data;
    SEXP real = PROTECT(A->Rf_ScalarReal(2));
    (void)A->INTEGER(real);
    s->line = __LINE__;
}

void null_input(void *data) {
    struct State *s = data;
    (void)A->TYPEOF(NULL);
    s->line = __LINE__;
}

void resize_overflow(void *data) {
    struct State *s = data;
    A->R_resizeVector(s->input, 3);
    s->line = __LINE__;
}

void cycle_length(void *data) {
    struct State *s = data;
    (void)A->LENGTH(s->input);
    s->line = __LINE__;
}
"#;

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> &'static Library {
    static FIXTURE: OnceLock<Library> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = TempDir(
            std::env::temp_dir().join(format!("roxide-w1a-{}-{suffix}", std::process::id())),
        );
        std::fs::create_dir(&dir.0).unwrap();
        let library = dir
            .0
            .join(format!("fixture.{}", std::env::consts::DLL_EXTENSION));
        let mut cc = Command::new("cc");
        cc.arg(if cfg!(target_os = "macos") {
            "-dynamiclib"
        } else {
            "-shared"
        });
        let mut cc = cc
            .args([
                "-fPIC",
                "-std=gnu11",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-x",
                "c",
            ])
            .arg("-I")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/include"))
            .arg("-o")
            .arg(&library)
            .arg("-")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = cc.stdin.take().unwrap();
        stdin.write_all(C_API.as_bytes()).unwrap();
        stdin.write_all(C_FIXTURE.as_bytes()).unwrap();
        drop(stdin);
        let output = cc.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // SAFETY: We built this library locally from the fixture above.
        unsafe { Library::new(library) }.unwrap()
    })
}

fn run(ctx: &mut Ctx, name: &[u8], input: Sexp) -> roxide_eval::EvalResult<()> {
    let mut state = State {
        api: &API,
        nil: R_NilValue,
        missing_string: R_NaString,
        names: R_NamesSymbol,
        class: R_ClassSymbol,
        input,
        line: 0,
    };
    let before = ctx.session().heap.protect_depth();
    // SAFETY: Both the C callback and its state stay live throughout the trampoline.
    let result = unsafe {
        let fun = fixture().get::<VoidFn>(name).unwrap();
        enter_trampoline(ctx, *fun, from_mut(&mut state).cast())
    };
    assert_eq!(
        state.line,
        0,
        "C fixture {} failed at line {}",
        String::from_utf8_lossy(name),
        state.line
    );
    assert_eq!(ctx.session().heap.protect_depth(), before);
    result
}

fn each_policy(mut f: impl FnMut(&mut Ctx)) {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        f(&mut Ctx::new(Box::new(PureHost::new()), policy).unwrap());
    }
}

#[test]
fn vectors_and_gc_torture() {
    each_policy(|ctx| run(ctx, b"vectors", Sexp::NULL).unwrap());
}

#[test]
fn compact_altrep_access() {
    each_policy(|ctx| {
        let x = ctx.session_mut().alloc_compact_intseq(6, 4, -1).unwrap();
        run(ctx, b"compact", Sexp::from_id(x)).unwrap();
    });
}

#[test]
fn character_and_symbol_access() {
    each_policy(|ctx| run(ctx, b"strings_and_symbols", Sexp::NULL).unwrap());
}

#[test]
fn translated_storage_is_released_on_reset_and_frame_exit() {
    each_policy(|ctx| {
        run(ctx, b"translation_storage", Sexp::NULL).unwrap();
        assert!(run(ctx, b"translation_error", Sexp::NULL).is_err());
        run(ctx, b"translation_storage", Sexp::NULL).unwrap();
    });
}

#[test]
fn string_and_c_utilities() {
    each_policy(|ctx| {
        run(ctx, b"string_utilities", Sexp::NULL).unwrap();
        run(ctx, b"c_utilities", Sexp::NULL).unwrap();
    });
}

#[test]
fn check_user_interrupt_returns_when_none_is_pending() {
    each_policy(|ctx| run(ctx, b"check_interrupt", Sexp::NULL).unwrap());
}

#[test]
fn pairlist_and_language_construction() {
    each_policy(|ctx| run(ctx, b"pairlists", Sexp::NULL).unwrap());
}

#[test]
fn attributes_duplicates_and_coercion() {
    each_policy(|ctx| run(ctx, b"attributes_and_coercion", Sexp::NULL).unwrap());
}

#[test]
fn resizable_buffers_keep_their_address() {
    each_policy(|ctx| run(ctx, b"resizing", Sexp::NULL).unwrap());
}

#[test]
fn errors_jump_to_the_c_trampoline() {
    each_policy(|ctx| {
        let (_, stderr) = ctx
            .capture_streams(false, true, false, |ctx| {
                assert!(run(ctx, b"wrong_pointer", Sexp::NULL).is_err());
                assert!(run(ctx, b"null_input", Sexp::NULL).is_err());

                let x = ctx
                    .session_mut()
                    .alloc_resizable(roxide_value::SexpType::Int, 2, 2)?;
                ctx.scoped(|ctx| {
                    ctx.protect(x);
                    assert!(run(ctx, b"resize_overflow", Sexp::from_id(x)).is_err());
                    assert_eq!(ctx.session().xlength(x)?, 2);
                    Ok(())
                })?;

                let nil = roxide_value::SEXP_NIL;
                let list = ctx.session_mut().cons(nil, nil, nil)?;
                ctx.session_mut().set_cdr(list, list)?;
                assert!(run(ctx, b"cycle_length", Sexp::from_id(list)).is_err());
                Ok(())
            })
            .unwrap();
        let stderr = String::from_utf8(stderr).unwrap();
        assert_eq!(
            stderr,
            concat!(
                "Error: INTEGER() can only be applied to a 'integer', not a 'double'\n",
                "Error: invalid NULL SEXP passed to TYPEOF\n",
                "Error: subscript out of bounds (3 >= 2)\n",
                "Error: cyclic pairlist\n",
            )
        );
    });
}

#[test]
fn allocating_api_collects_unprotected_objects() {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).unwrap();
    let unprotected = ctx.session_mut().scalar_int(22).unwrap();
    run(&mut ctx, b"vectors", Sexp::NULL).unwrap();
    assert!(ctx.session().heap.get(unprotected).is_err());
}

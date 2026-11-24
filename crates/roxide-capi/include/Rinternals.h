/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_RINTERNALS_H
#define ROXIDE_RINTERNALS_H

#include <limits.h>
#include <stddef.h>
#include <stdio.h>
#include <stdint.h>
#include <R_ext/Arith.h>
#include <R_ext/Boolean.h>
#include <R_ext/Complex.h>
#include <R_ext/Error.h>
#include <R_ext/Memory.h>
#include <R_ext/Rdynload.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct SEXPREC *SEXP;
typedef unsigned int SEXPTYPE;
typedef unsigned char Rbyte;
typedef int R_len_t;
typedef ptrdiff_t R_xlen_t;
typedef int PROTECT_INDEX;

#ifdef __cplusplus
static_assert(sizeof(SEXP) == 8, "Roxide requires 64-bit pointers");
#else
_Static_assert(sizeof(SEXP) == 8, "Roxide requires 64-bit pointers");
#endif

#define R_LEN_T_MAX INT_MAX
#define R_SHORT_LEN_MAX R_LEN_T_MAX
#define R_XLEN_T_MAX 4503599627370496LL
#define LONG_VECTOR_SUPPORT

#define NILSXP 0
#define SYMSXP 1
#define LISTSXP 2
#define CLOSXP 3
#define ENVSXP 4
#define PROMSXP 5
#define LANGSXP 6
#define SPECIALSXP 7
#define BUILTINSXP 8
#define CHARSXP 9
#define LGLSXP 10
#define INTSXP 13
#define REALSXP 14
#define CPLXSXP 15
#define STRSXP 16
#define DOTSXP 17
#define ANYSXP 18
#define VECSXP 19
#define EXPRSXP 20
#define BCODESXP 21
#define EXTPTRSXP 22
#define WEAKREFSXP 23
#define RAWSXP 24
#define OBJSXP 25
#define S4SXP OBJSXP
#define NEWSXP 30
#define FREESXP 31
#define FUNSXP 99

typedef enum {
    CE_NATIVE = 0,
    CE_UTF8 = 1,
    CE_LATIN1 = 2,
    CE_BYTES = 3,
    CE_SYMBOL = 5,
    CE_ANY = 99
} cetype_t;

#if defined(__cplusplus) && !defined(R_NO_REMAP)
#define R_NO_REMAP
#endif

/* Permanent objects and symbols. */
LibExtern SEXP R_NilValue;
LibExtern SEXP R_UnboundValue;
LibExtern SEXP R_MissingArg;
LibExtern SEXP R_GlobalEnv;
LibExtern SEXP R_BaseEnv;
LibExtern SEXP R_BaseNamespace;
LibExtern SEXP R_EmptyEnv;
LibExtern SEXP R_NamespaceRegistry;
LibExtern SEXP R_MethodsNamespace;
LibExtern SEXP R_BlankString;
LibExtern SEXP R_BlankScalarString;
LibExtern SEXP R_NaString;
LibExtern SEXP R_TrueValue;
LibExtern SEXP R_FalseValue;
LibExtern SEXP R_LogicalNAValue;
LibExtern SEXP R_dot_Generic;
LibExtern SEXP R_dot_Method;
LibExtern SEXP R_dot_defined;
LibExtern SEXP R_dot_target;
LibExtern SEXP R_dot_packageName;
LibExtern SEXP R_Srcref;
LibExtern SEXP R_AsCharacterSymbol;
LibExtern SEXP R_AtsignSymbol;
LibExtern SEXP R_BaseSymbol;
LibExtern SEXP R_BraceSymbol;
LibExtern SEXP R_Bracket2Symbol;
LibExtern SEXP R_BracketSymbol;
LibExtern SEXP R_ClassSymbol;
LibExtern SEXP R_DeviceSymbol;
LibExtern SEXP R_DimNamesSymbol;
LibExtern SEXP R_DimSymbol;
LibExtern SEXP R_DollarSymbol;
LibExtern SEXP R_DotsSymbol;
LibExtern SEXP R_DoubleColonSymbol;
LibExtern SEXP R_DropSymbol;
LibExtern SEXP R_EvalSymbol;
LibExtern SEXP R_FunctionSymbol;
LibExtern SEXP R_LastvalueSymbol;
LibExtern SEXP R_LevelsSymbol;
LibExtern SEXP R_ModeSymbol;
LibExtern SEXP R_NaRmSymbol;
LibExtern SEXP R_NameSymbol;
LibExtern SEXP R_NamesSymbol;
LibExtern SEXP R_NamespaceEnvSymbol;
LibExtern SEXP R_PackageSymbol;
LibExtern SEXP R_PreviousSymbol;
LibExtern SEXP R_QuoteSymbol;
LibExtern SEXP R_RowNamesSymbol;
LibExtern SEXP R_SeedsSymbol;
LibExtern SEXP R_SortListSymbol;
LibExtern SEXP R_SourceSymbol;
LibExtern SEXP R_SpecSymbol;
LibExtern SEXP R_SrcfileSymbol;
LibExtern SEXP R_SrcrefSymbol;
LibExtern SEXP R_TripleColonSymbol;
LibExtern SEXP R_TspSymbol;
LibExtern SEXP R_baseSymbol;
#define NA_STRING R_NaString
LibExtern Rboolean R_Interactive;

/* Object types and vector storage. */
int TYPEOF(SEXP x);
int LENGTH(SEXP x);
R_xlen_t XLENGTH(SEXP x);
int IS_LONG_VEC(SEXP x);
int IS_SCALAR(SEXP x, int type);
R_len_t Rf_length(SEXP x);
R_xlen_t Rf_xlength(SEXP x);
SEXP Rf_lengthgets(SEXP x, R_len_t length);
SEXP Rf_xlengthgets(SEXP x, R_xlen_t length);
SEXP R_allocResizableVector(SEXPTYPE type, R_xlen_t maxlen);
void R_resizeVector(SEXP x, R_xlen_t newlen);
bool R_isResizable(SEXP x);
R_xlen_t R_maxLength(SEXP x);
SEXP R_duplicateAsResizable(SEXP x);
void *DATAPTR_RW(SEXP x);
const void *DATAPTR_RO(SEXP x);
const void *DATAPTR_OR_NULL(SEXP x);
const SEXP *STRING_PTR_RO(SEXP x);
const SEXP *VECTOR_PTR_RO(SEXP x);
SEXP STRING_ELT(SEXP x, R_xlen_t i);
SEXP VECTOR_ELT(SEXP x, R_xlen_t i);
void SET_STRING_ELT(SEXP x, R_xlen_t i, SEXP value);
SEXP SET_VECTOR_ELT(SEXP x, R_xlen_t i, SEXP value);
enum {
    SORTED_DECR_NA_1ST = -2,
    SORTED_DECR = -1,
    KNOWN_UNSORTED = 0,
    SORTED_INCR = 1,
    SORTED_INCR_NA_1ST = 2,
    UNKNOWN_SORTEDNESS = INT_MIN
};
int INTEGER_IS_SORTED(SEXP x);
int REAL_IS_SORTED(SEXP x);
int STRING_IS_SORTED(SEXP x);
int INTEGER_NO_NA(SEXP x);
int LOGICAL_NO_NA(SEXP x);
int REAL_NO_NA(SEXP x);
int STRING_NO_NA(SEXP x);

SEXP Rf_allocVector(SEXPTYPE type, R_xlen_t length);
SEXP Rf_allocMatrix(SEXPTYPE type, int rows, int columns);
SEXP Rf_allocArray(SEXPTYPE type, SEXP dimensions);
SEXP Rf_alloc3DArray(SEXPTYPE type, int rows, int columns, int planes);
SEXP Rf_allocList(int length);
SEXP Rf_allocLang(int length);
SEXP Rf_allocS4Object(void);
SEXP Rf_mkNamed(SEXPTYPE type, const char **names);
SEXP Rf_duplicate(SEXP x);
SEXP Rf_shallow_duplicate(SEXP x);
void Rf_copyListMatrix(SEXP target, SEXP source, Rboolean byrow);
void Rf_copyMatrix(SEXP target, SEXP source, Rboolean byrow);
void Rf_copyVector(SEXP target, SEXP source);
int MAYBE_SHARED(SEXP x);
int MAYBE_REFERENCED(SEXP x);
int NOT_SHARED(SEXP x);
int NO_REFERENCES(SEXP x);
void MARK_NOT_MUTABLE(SEXP x);

SEXP Rf_asChar(SEXP x);
int Rf_asInteger(SEXP x);
int Rf_asLogical(SEXP x);
bool Rf_asBool(SEXP x);
Rboolean Rf_asRboolean(SEXP x);
double Rf_asReal(SEXP x);
Rcomplex Rf_asComplex(SEXP x);
SEXP Rf_asS4(SEXP x, Rboolean flag, int complete);
SEXP Rf_asCharacterFactor(SEXP x);
SEXP Rf_coerceVector(SEXP x, SEXPTYPE type);
int Rf_str2type(const char *name);
const char *Rf_type2char(SEXPTYPE type);
SEXP Rf_type2str(SEXPTYPE type);
SEXP Rf_type2str_nowarn(SEXPTYPE type);
SEXP Rf_VectorToPairList(SEXP x);
SEXP Rf_PairToVectorList(SEXP x);
Rboolean Rf_inherits(SEXP x, const char *klass);
Rboolean Rf_isBlankString(const char *x);
Rboolean Rf_StringBlank(SEXP x);
Rboolean Rf_StringFalse(const char *x);
Rboolean Rf_StringTrue(const char *x);
bool R_isTRUE(SEXP x);

/* Pairlists and symbols. */
SEXP Rf_cons(SEXP car, SEXP cdr);
SEXP Rf_lcons(SEXP car, SEXP cdr);
SEXP TAG(SEXP x);
SEXP PRINTNAME(SEXP x);
void SET_TAG(SEXP x, SEXP tag);
SEXP Rf_elt(SEXP x, int i);
SEXP Rf_nthcdr(SEXP x, int n);
SEXP Rf_lastElt(SEXP x);
SEXP Rf_listAppend(SEXP x, SEXP y);
SEXP Rf_install(const char *name);
SEXP Rf_installChar(SEXP name);
SEXP Rf_installTrChar(SEXP name);

const char *R_CHAR(SEXP x);
#define CHAR(x) R_CHAR(x)
SEXP Rf_mkString(const char *text);
SEXP Rf_mkChar(const char *text);
SEXP Rf_mkCharLen(const char *text, int length);
SEXP Rf_mkCharCE(const char *text, cetype_t encoding);
SEXP Rf_mkCharLenCE(const char *text, int length, cetype_t encoding);
cetype_t Rf_getCharCE(SEXP x);
const char *Rf_translateChar(SEXP x);
const char *Rf_translateCharUTF8(SEXP x);
const char *Rf_reEnc(const char *text, cetype_t from, cetype_t to, int substitute);
Rboolean Rf_charIsASCII(SEXP x);
Rboolean Rf_charIsLatin1(SEXP x);
Rboolean Rf_charIsUTF8(SEXP x);

/* Alternate representations. */
int ALTREP(SEXP x);
SEXP ALTREP_CLASS(SEXP x);
SEXP R_altrep_data1(SEXP x);
SEXP R_altrep_data2(SEXP x);
void R_set_altrep_data1(SEXP x, SEXP value);
void R_set_altrep_data2(SEXP x, SEXP value);
SEXP R_altrep_class_name(SEXP x);
SEXP R_altrep_class_package(SEXP x);
SEXP R_tryWrap(SEXP x);

/* Attributes and classes. */
SEXP Rf_getAttrib(SEXP x, SEXP name);
SEXP Rf_setAttrib(SEXP x, SEXP name, SEXP value);
SEXP R_getAttributes(SEXP x);
int R_getAttribCount(SEXP x);
SEXP R_getAttribNames(SEXP x);
bool R_hasAttrib(SEXP x, SEXP name);
SEXP R_mapAttrib(SEXP x, SEXP (*fun)(SEXP, SEXP, void *), void *data);
int ANY_ATTRIB(SEXP x);
void CLEAR_ATTRIB(SEXP x);
void DUPLICATE_ATTRIB(SEXP target, SEXP source);
void SHALLOW_DUPLICATE_ATTRIB(SEXP target, SEXP source);
void Rf_copyMostAttrib(SEXP source, SEXP target);
SEXP Rf_classgets(SEXP x, SEXP value);
SEXP Rf_dimgets(SEXP x, SEXP value);
SEXP Rf_dimnamesgets(SEXP x, SEXP value);
SEXP Rf_namesgets(SEXP x, SEXP value);
SEXP R_class(SEXP x);
int R_nrow(SEXP x);
int R_ncol(SEXP x);
int Rf_nrows(SEXP x);
int Rf_ncols(SEXP x);
int Rf_nlevels(SEXP x);
SEXP Rf_GetArrayDimnames(SEXP x);
SEXP Rf_GetMatrixDimnames(SEXP x);
SEXP Rf_GetRowNames(SEXP x);
SEXP Rf_GetColNames(SEXP x);
SEXP R_do_MAKE_CLASS(const char *name);
SEXP R_getClassDef(const char *name);
SEXP R_do_new_object(SEXP class_definition);
SEXP R_do_slot(SEXP x, SEXP name);
SEXP R_do_slot_assign(SEXP x, SEXP name, SEXP value);
int R_has_slot(SEXP x, SEXP name);
int R_check_class_etc(SEXP x, const char **classes);

/* Environments, closures and evaluation. */
SEXP R_NewEnv(SEXP enclos, int hash, int size);
SEXP R_ParentEnv(SEXP rho);
SEXP R_GetCurrentEnv(void);
SEXP R_getVar(SEXP sym, SEXP rho, Rboolean inherits);
SEXP R_getVarEx(SEXP sym, SEXP rho, Rboolean inherits, SEXP ifnotfound);
Rboolean R_existsVarInFrame(SEXP rho, SEXP sym);
void R_removeVarFromFrame(SEXP sym, SEXP rho);
void Rf_defineVar(SEXP symbol, SEXP value, SEXP rho);
void Rf_setVar(SEXP symbol, SEXP value, SEXP rho);
SEXP Rf_findFun(SEXP symbol, SEXP rho);
SEXP Rf_topenv(SEXP target, SEXP rho);
SEXP R_envSymbols(SEXP rho);
SEXP R_lsInternal3(SEXP rho, Rboolean all, Rboolean sorted);
SEXP R_FindNamespace(SEXP name);
SEXP R_getRegisteredNamespace(const char *name);
Rboolean R_IsPackageEnv(SEXP rho);
SEXP R_PackageEnvName(SEXP rho);
Rboolean R_IsNamespaceEnv(SEXP rho);
SEXP R_NamespaceEnvSpec(SEXP rho);
void R_LockEnvironment(SEXP rho, Rboolean bindings);
Rboolean R_EnvironmentIsLocked(SEXP rho);
void R_LockBinding(SEXP sym, SEXP rho);
void R_unLockBinding(SEXP sym, SEXP rho);
Rboolean R_BindingIsLocked(SEXP sym, SEXP rho);
Rboolean R_BindingIsActive(SEXP sym, SEXP rho);
void R_MakeActiveBinding(SEXP sym, SEXP fun, SEXP rho);
SEXP R_ActiveBindingFunction(SEXP sym, SEXP rho);

typedef enum {
    R_BindingTypeUnbound,
    R_BindingTypeValue,
    R_BindingTypeMissing,
    R_BindingTypeDelayed,
    R_BindingTypeForced,
    R_BindingTypeActive
} R_BindingType_t;
typedef enum {
    R_DotTypeValue,
    R_DotTypeMissing,
    R_DotTypeDelayed,
    R_DotTypeForced
} R_DotType_t;

R_BindingType_t R_GetBindingType(SEXP sym, SEXP env);
SEXP R_ForcedBindingExpression(SEXP sym, SEXP env);
SEXP R_DelayedBindingExpression(SEXP sym, SEXP env);
SEXP R_DelayedBindingEnvironment(SEXP sym, SEXP env);
void R_MakeForcedBinding(SEXP sym, SEXP expr, SEXP value, SEXP env);
void R_MakeDelayedBinding(SEXP sym, SEXP expr, SEXP eval_env, SEXP env);
void R_MakeMissingBinding(SEXP sym, SEXP env);
SEXP R_findDotsEnv(SEXP env);
Rboolean R_DotsExist(SEXP env);
int R_DotsLength(SEXP env);
SEXP R_DotsNames(SEXP env);
R_DotType_t R_GetDotType(int i, SEXP env);
SEXP R_DotsElt(int i, SEXP env);
SEXP R_DotForcedExpression(int i, SEXP env);
SEXP R_DotDelayedExpression(int i, SEXP env);
SEXP R_DotDelayedEnvironment(int i, SEXP env);

SEXP R_mkClosure(SEXP formals, SEXP body, SEXP rho);
SEXP R_ClosureFormals(SEXP closure);
SEXP R_ClosureBody(SEXP closure);
SEXP R_ClosureEnv(SEXP closure);
SEXP R_ClosureExpr(SEXP closure);
SEXP R_BytecodeExpr(SEXP code);
SEXP Rf_eval(SEXP expression, SEXP rho);
SEXP R_tryEval(SEXP expression, SEXP rho, int *error);
SEXP R_tryEvalSilent(SEXP expression, SEXP rho, int *error);
SEXP R_forceAndCall(SEXP call, int count, SEXP rho);
SEXP R_GetCurrentSrcref(int skip);
SEXP R_GetSrcFilename(SEXP srcref);
SEXP Rf_GetOption1(SEXP tag);
int Rf_GetOptionWidth(void);
void Rf_PrintValue(SEXP x);

/* Protection and non-local control flow. */
SEXP Rf_protect(SEXP x);
void Rf_unprotect(int count);
void Rf_unprotect_ptr(SEXP x);
void R_ProtectWithIndex(SEXP x, PROTECT_INDEX *index);
void R_Reprotect(SEXP x, PROTECT_INDEX index);
#define PROTECT(x) Rf_protect(x)
#define UNPROTECT(n) Rf_unprotect(n)
#define UNPROTECT_PTR(x) Rf_unprotect_ptr(x)
#define PROTECT_WITH_INDEX(x, index) R_ProtectWithIndex((x), (index))
#define REPROTECT(x, index) R_Reprotect((x), (index))
void R_PreserveObject(SEXP x);
void R_ReleaseObject(SEXP x);
SEXP R_NewPreciousMSet(int size);
void R_PreserveInMSet(SEXP x, SEXP set);
void R_ReleaseFromMSet(SEXP x, SEXP set);

Rboolean R_ToplevelExec(void (*fun)(void *), void *data);
SEXP R_ExecWithCleanup(SEXP (*fun)(void *), void *data,
                       void (*cleanup)(void *), void *cleanup_data);
SEXP R_tryCatch(SEXP (*fun)(void *), void *data, SEXP classes,
                SEXP (*handler)(SEXP, void *), void *handler_data,
                void (*cleanup)(void *), void *cleanup_data);
SEXP R_tryCatchError(SEXP (*fun)(void *), void *data,
                     SEXP (*handler)(SEXP, void *), void *handler_data);
SEXP R_withCallingErrorHandler(SEXP (*fun)(void *), void *data,
                               SEXP (*handler)(SEXP, void *), void *handler_data);
SEXP R_UnwindProtect(SEXP (*fun)(void *), void *data,
                     void (*cleanup)(void *, Rboolean), void *cleanup_data,
                     SEXP continuation);
SEXP R_MakeUnwindCont(void);
NORET void R_ContinueUnwind(SEXP continuation);
NORET void Rf_errorcall(SEXP call, const char *format, ...) ROXIDE_PRINTF(2, 3);
void Rf_warningcall(SEXP call, const char *format, ...) ROXIDE_PRINTF(2, 3);
void Rf_warningcall_immediate(SEXP call, const char *format, ...) ROXIDE_PRINTF(2, 3);
void Rf_onintr(void);

/* External pointers and finalizers. */
typedef void (*R_CFinalizer_t)(SEXP);
SEXP R_MakeExternalPtr(void *address, SEXP tag, SEXP protect);
typedef DL_FUNC R_DL_FUNC;
SEXP R_MakeExternalPtrFn(R_DL_FUNC address, SEXP tag, SEXP protect);
void *R_ExternalPtrAddr(SEXP x);
DL_FUNC R_ExternalPtrAddrFn(SEXP x);
SEXP R_ExternalPtrTag(SEXP x);
SEXP R_ExternalPtrProtected(SEXP x);
void R_ClearExternalPtr(SEXP x);
void R_SetExternalPtrAddr(SEXP x, void *address);
void R_SetExternalPtrTag(SEXP x, SEXP tag);
void R_SetExternalPtrProtected(SEXP x, SEXP protect);
void R_RegisterFinalizer(SEXP x, SEXP finalizer);
void R_RegisterFinalizerEx(SEXP x, SEXP finalizer, Rboolean onexit);
void R_RegisterCFinalizer(SEXP x, R_CFinalizer_t finalizer);
void R_RegisterCFinalizerEx(SEXP x, R_CFinalizer_t finalizer, Rboolean onexit);
SEXP R_MakeWeakRef(SEXP key, SEXP value, SEXP finalizer, Rboolean onexit);
SEXP R_MakeWeakRefC(SEXP key, SEXP value, R_CFinalizer_t finalizer, Rboolean onexit);
SEXP R_WeakRefKey(SEXP reference);
SEXP R_WeakRefValue(SEXP reference);
void R_RunWeakRefFinalizer(SEXP reference);

/* Serialization streams. */
typedef void *R_pstream_data_t;
typedef enum {
    R_pstream_any_format,
    R_pstream_ascii_format,
    R_pstream_binary_format,
    R_pstream_xdr_format,
    R_pstream_asciihex_format
} R_pstream_format_t;

/* Allocate streams with these types and initialize them with R_Init*Stream. */
typedef struct R_outpstream_st {
    uint64_t storage[6];
} *R_outpstream_t;
typedef struct R_inpstream_st {
    uint64_t storage[16];
} *R_inpstream_t;

void R_InitInPStream(R_inpstream_t stream, R_pstream_data_t data,
                     R_pstream_format_t type,
                     int (*inchar)(R_inpstream_t),
                     void (*inbytes)(R_inpstream_t, void *, int),
                     SEXP (*hook)(SEXP, SEXP), SEXP hook_data);
void R_InitOutPStream(R_outpstream_t stream, R_pstream_data_t data,
                      R_pstream_format_t type, int version,
                      void (*outchar)(R_outpstream_t, int),
                      void (*outbytes)(R_outpstream_t, void *, int),
                      SEXP (*hook)(SEXP, SEXP), SEXP hook_data);
void R_InitFileOutPStream(R_outpstream_t stream, FILE *file,
                          R_pstream_format_t type, int version,
                          SEXP (*hook)(SEXP, SEXP), SEXP hook_data);
void R_InitFileInPStream(R_inpstream_t stream, FILE *file,
                         R_pstream_format_t type,
                         SEXP (*hook)(SEXP, SEXP), SEXP hook_data);
void R_Serialize(SEXP x, R_outpstream_t stream);
SEXP R_Unserialize(R_inpstream_t stream);

int R_compute_identical(SEXP x, SEXP y, int flags);
SEXP Rf_duplicated(SEXP x, Rboolean fromlast);
R_xlen_t Rf_any_duplicated(SEXP x, Rboolean fromlast);
R_xlen_t Rf_any_duplicated3(SEXP x, SEXP incomparables, Rboolean fromlast);
SEXP Rf_match(SEXP table, SEXP x, int nomatch);
Rboolean Rf_pmatch(SEXP x, SEXP table, Rboolean exact);
Rboolean Rf_psmatch(const char *x, const char *table, Rboolean exact);
void R_orderVector(int *index, int n, SEXP args, Rboolean nalast, Rboolean decreasing);
void R_orderVector1(int *index, int n, SEXP x, Rboolean nalast, Rboolean decreasing);
Rboolean Rf_isUnsorted(SEXP x, Rboolean strictly);

/* Typed vector accessors. */
int *LOGICAL(SEXP x);
const int *LOGICAL_RO(SEXP x);
const int *LOGICAL_OR_NULL(SEXP x);
int LOGICAL_ELT(SEXP x, R_xlen_t i);
void SET_LOGICAL_ELT(SEXP x, R_xlen_t i, int value);
R_xlen_t LOGICAL_GET_REGION(SEXP x, R_xlen_t i, R_xlen_t count, int *buffer);
int *INTEGER(SEXP x);
const int *INTEGER_RO(SEXP x);
const int *INTEGER_OR_NULL(SEXP x);
int INTEGER_ELT(SEXP x, R_xlen_t i);
void SET_INTEGER_ELT(SEXP x, R_xlen_t i, int value);
R_xlen_t INTEGER_GET_REGION(SEXP x, R_xlen_t i, R_xlen_t count, int *buffer);
double *REAL(SEXP x);
const double *REAL_RO(SEXP x);
const double *REAL_OR_NULL(SEXP x);
double REAL_ELT(SEXP x, R_xlen_t i);
void SET_REAL_ELT(SEXP x, R_xlen_t i, double value);
R_xlen_t REAL_GET_REGION(SEXP x, R_xlen_t i, R_xlen_t count, double *buffer);
Rcomplex *COMPLEX(SEXP x);
const Rcomplex *COMPLEX_RO(SEXP x);
const Rcomplex *COMPLEX_OR_NULL(SEXP x);
Rcomplex COMPLEX_ELT(SEXP x, R_xlen_t i);
void SET_COMPLEX_ELT(SEXP x, R_xlen_t i, Rcomplex value);
R_xlen_t COMPLEX_GET_REGION(SEXP x, R_xlen_t i, R_xlen_t count, Rcomplex *buffer);
Rbyte *RAW(SEXP x);
const Rbyte *RAW_RO(SEXP x);
const Rbyte *RAW_OR_NULL(SEXP x);
Rbyte RAW_ELT(SEXP x, R_xlen_t i);
void SET_RAW_ELT(SEXP x, R_xlen_t i, Rbyte value);
R_xlen_t RAW_GET_REGION(SEXP x, R_xlen_t i, R_xlen_t count, Rbyte *buffer);
SEXP Rf_ScalarLogical(int value);
SEXP Rf_ScalarInteger(int value);
SEXP Rf_ScalarReal(double value);
SEXP Rf_ScalarComplex(Rcomplex value);
SEXP Rf_ScalarRaw(Rbyte value);
SEXP Rf_ScalarString(SEXP value);

/* Pairlist traversal and construction. */
SEXP CAR(SEXP x);
SEXP CDR(SEXP x);
SEXP CAAR(SEXP x);
SEXP CDAR(SEXP x);
SEXP CADR(SEXP x);
SEXP CDDR(SEXP x);
SEXP CADDR(SEXP x);
SEXP CDDDR(SEXP x);
SEXP CADDDR(SEXP x);
SEXP CAD4R(SEXP x);
SEXP CAD5R(SEXP x);
SEXP SETCAR(SEXP x, SEXP value);
SEXP SETCDR(SEXP x, SEXP value);
SEXP SETCADR(SEXP x, SEXP value);
SEXP SETCADDR(SEXP x, SEXP value);
SEXP SETCADDDR(SEXP x, SEXP value);
SEXP SETCAD4R(SEXP x, SEXP value);
SEXP Rf_list1(SEXP a);
SEXP Rf_list2(SEXP a, SEXP b);
SEXP Rf_list3(SEXP a, SEXP b, SEXP c);
SEXP Rf_list4(SEXP a, SEXP b, SEXP c, SEXP d);
SEXP Rf_list5(SEXP a, SEXP b, SEXP c, SEXP d, SEXP e);
SEXP Rf_list6(SEXP a, SEXP b, SEXP c, SEXP d, SEXP e, SEXP f);
SEXP Rf_lang1(SEXP a);
SEXP Rf_lang2(SEXP a, SEXP b);
SEXP Rf_lang3(SEXP a, SEXP b, SEXP c);
SEXP Rf_lang4(SEXP a, SEXP b, SEXP c, SEXP d);
SEXP Rf_lang5(SEXP a, SEXP b, SEXP c, SEXP d, SEXP e);
SEXP Rf_lang6(SEXP a, SEXP b, SEXP c, SEXP d, SEXP e, SEXP f);

/* Object type predicates. */
Rboolean Rf_isArray(SEXP x);
Rboolean Rf_isComplex(SEXP x);
Rboolean Rf_isDataFrame(SEXP x);
Rboolean Rf_isEnvironment(SEXP x);
Rboolean Rf_isExpression(SEXP x);
Rboolean Rf_isFactor(SEXP x);
Rboolean Rf_isFunction(SEXP x);
Rboolean Rf_isInteger(SEXP x);
Rboolean Rf_isLanguage(SEXP x);
Rboolean Rf_isList(SEXP x);
Rboolean Rf_isLogical(SEXP x);
Rboolean Rf_isMatrix(SEXP x);
Rboolean Rf_isNewList(SEXP x);
Rboolean Rf_isNull(SEXP x);
Rboolean Rf_isNumber(SEXP x);
Rboolean Rf_isNumeric(SEXP x);
Rboolean Rf_isObject(SEXP x);
Rboolean Rf_isOrdered(SEXP x);
Rboolean Rf_isPairList(SEXP x);
Rboolean Rf_isPrimitive(SEXP x);
Rboolean Rf_isReal(SEXP x);
Rboolean Rf_isS4(SEXP x);
Rboolean Rf_isScalarString(SEXP x);
Rboolean Rf_isString(SEXP x);
Rboolean Rf_isSymbol(SEXP x);
Rboolean Rf_isTs(SEXP x);
Rboolean Rf_isUnordered(SEXP x);
Rboolean Rf_isVector(SEXP x);
Rboolean Rf_isVectorAtomic(SEXP x);
Rboolean Rf_isVectorList(SEXP x);
Rboolean Rf_isVectorizable(SEXP x);

#ifndef R_NO_REMAP
#define CONS Rf_cons
#define LCONS Rf_lcons
#define GetArrayDimnames Rf_GetArrayDimnames
#define GetColNames Rf_GetColNames
#define GetMatrixDimnames Rf_GetMatrixDimnames
#define GetOption1 Rf_GetOption1
#define GetOptionWidth Rf_GetOptionWidth
#define GetRowNames Rf_GetRowNames
#define PairToVectorList Rf_PairToVectorList
#define PrintValue Rf_PrintValue
#define ScalarComplex Rf_ScalarComplex
#define ScalarInteger Rf_ScalarInteger
#define ScalarLogical Rf_ScalarLogical
#define ScalarRaw Rf_ScalarRaw
#define ScalarReal Rf_ScalarReal
#define ScalarString Rf_ScalarString
#define StringBlank Rf_StringBlank
#define StringFalse Rf_StringFalse
#define StringTrue Rf_StringTrue
#define VectorToPairList Rf_VectorToPairList
#define alloc3DArray Rf_alloc3DArray
#define allocArray Rf_allocArray
#define allocLang Rf_allocLang
#define allocList Rf_allocList
#define allocMatrix Rf_allocMatrix
#define allocS4Object Rf_allocS4Object
#define allocVector Rf_allocVector
#define any_duplicated Rf_any_duplicated
#define any_duplicated3 Rf_any_duplicated3
#define asBool Rf_asBool
#define asChar Rf_asChar
#define asCharacterFactor Rf_asCharacterFactor
#define asComplex Rf_asComplex
#define asInteger Rf_asInteger
#define asLogical Rf_asLogical
#define asRboolean Rf_asRboolean
#define asReal Rf_asReal
#define asS4 Rf_asS4
#define charIsASCII Rf_charIsASCII
#define charIsLatin1 Rf_charIsLatin1
#define charIsUTF8 Rf_charIsUTF8
#define classgets Rf_classgets
#define coerceVector Rf_coerceVector
#define cons Rf_cons
#define copyListMatrix Rf_copyListMatrix
#define copyMatrix Rf_copyMatrix
#define copyMostAttrib Rf_copyMostAttrib
#define copyVector Rf_copyVector
#define defineVar Rf_defineVar
#define dimgets Rf_dimgets
#define dimnamesgets Rf_dimnamesgets
#define duplicate Rf_duplicate
#define duplicated Rf_duplicated
#define elt Rf_elt
#define errorcall Rf_errorcall
#define eval Rf_eval
#define findFun Rf_findFun
#define getAttrib Rf_getAttrib
#define getCharCE Rf_getCharCE
#define inherits Rf_inherits
#define install Rf_install
#define installChar Rf_installChar
#define installTrChar Rf_installTrChar
#define isArray Rf_isArray
#define isBlankString Rf_isBlankString
#define isComplex Rf_isComplex
#define isDataFrame Rf_isDataFrame
#define isEnvironment Rf_isEnvironment
#define isExpression Rf_isExpression
#define isFactor Rf_isFactor
#define isFunction Rf_isFunction
#define isInteger Rf_isInteger
#define isLanguage Rf_isLanguage
#define isList Rf_isList
#define isLogical Rf_isLogical
#define isMatrix Rf_isMatrix
#define isNewList Rf_isNewList
#define isNull Rf_isNull
#define isNumber Rf_isNumber
#define isNumeric Rf_isNumeric
#define isObject Rf_isObject
#define isOrdered Rf_isOrdered
#define isPairList Rf_isPairList
#define isPrimitive Rf_isPrimitive
#define isReal Rf_isReal
#define isS4 Rf_isS4
#define isScalarString Rf_isScalarString
#define isString Rf_isString
#define isSymbol Rf_isSymbol
#define isTs Rf_isTs
#define isUnordered Rf_isUnordered
#define isUnsorted Rf_isUnsorted
#define isVector Rf_isVector
#define isVectorAtomic Rf_isVectorAtomic
#define isVectorList Rf_isVectorList
#define isVectorizable Rf_isVectorizable
#define lang1 Rf_lang1
#define lang2 Rf_lang2
#define lang3 Rf_lang3
#define lang4 Rf_lang4
#define lang5 Rf_lang5
#define lang6 Rf_lang6
#define lastElt Rf_lastElt
#define lcons Rf_lcons
#define length Rf_length
#define lengthgets Rf_lengthgets
#define list1 Rf_list1
#define list2 Rf_list2
#define list3 Rf_list3
#define list4 Rf_list4
#define list5 Rf_list5
#define list6 Rf_list6
#define listAppend Rf_listAppend
#define match Rf_match
#define mkChar Rf_mkChar
#define mkCharCE Rf_mkCharCE
#define mkCharLen Rf_mkCharLen
#define mkCharLenCE Rf_mkCharLenCE
#define mkNamed Rf_mkNamed
#define mkString Rf_mkString
#define namesgets Rf_namesgets
#define ncols Rf_ncols
#define nlevels Rf_nlevels
#define nrows Rf_nrows
#define nthcdr Rf_nthcdr
#define onintr Rf_onintr
#define pmatch Rf_pmatch
#define protect Rf_protect
#define psmatch Rf_psmatch
#define reEnc Rf_reEnc
#define setAttrib Rf_setAttrib
#define setVar Rf_setVar
#define shallow_duplicate Rf_shallow_duplicate
#define str2type Rf_str2type
#define topenv Rf_topenv
#define translateChar Rf_translateChar
#define translateCharUTF8 Rf_translateCharUTF8
#define type2char Rf_type2char
#define type2str Rf_type2str
#define type2str_nowarn Rf_type2str_nowarn
#define unprotect Rf_unprotect
#define unprotect_ptr Rf_unprotect_ptr
#define warningcall Rf_warningcall
#define warningcall_immediate Rf_warningcall_immediate
#define xlength Rf_xlength
#define xlengthgets Rf_xlengthgets
#endif

#ifdef __cplusplus
}
#endif

#endif

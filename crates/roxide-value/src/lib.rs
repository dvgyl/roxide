#![forbid(unsafe_code)]

//! Core GNU R 4.6.2 data model, exact `NA_real_` (`1954`) semantics, `ALTREP`,
//! AST lowering, number/string formatting, and `deparse()`.
//!
//! `RObject` internals remain `pub(crate)` (ADR-0007); downstream crates access
//! heap objects strictly through [`RSession`], [`ValueApi`], and [`VecView`].

mod attributes;
mod capi_support;
mod coerce;
mod duplicate;

pub mod alt;
pub mod api;
pub mod deparse;
pub mod format;
pub mod identical;
pub mod lower;
pub mod na;
pub mod object;
pub mod session;

pub use alt::{
    AltClass, AltRegistry, AltString, AltVector, AltWrapper, CompactIntSeq, CompactRealSeq,
    Sortedness,
};
pub use api::{CoerceWarn, PairlistIter, ValueApi, ValueError, VecView};
pub use deparse::{DeparseOpts, deparse, dput};
pub use format::{FormatOpts, encode_string, format_int_vec, format_lgl_vec, format_real_vec};
pub use identical::{IdenticalOpts, identical};
pub use lower::{KeepSource, lower};
pub use na::{
    Elem, NA_INTEGER, NA_REAL, NA_REAL_BITS, RComplex, RLogical, int_add, int_mul, int_sub,
    is_na_real, is_nan_not_na, nan_propagate, real_hash_key,
};
pub use object::{Binding, CharId, Encoding, PrimId, PromiseState, RObject, SexpType, Sym};
pub use session::{
    BASE_ENV, BASE_NAMESPACE, BLANK_SCALAR_STRING, EMPTY_ENV, FALSE_SCALAR, GLOBAL_ENV,
    METHODS_NAMESPACE, NA_LGL_SCALAR, NAMESPACE_REGISTRY, RHeap, RSession, SEXP_MISSING, SEXP_NIL,
    SEXP_S4_NULL, SEXP_UNBOUND, SYM_AS_CHARACTER, SYM_ATSIGN, SYM_BASE, SYM_BRACE, SYM_BRACKET,
    SYM_BRACKET2, SYM_CLASS, SYM_DEVICE, SYM_DIM, SYM_DIMNAMES, SYM_DOLLAR, SYM_DOT_DEFINED,
    SYM_DOT_GENERIC, SYM_DOT_METHOD, SYM_DOT_PACKAGE_NAME, SYM_DOT_TARGET, SYM_DOTS,
    SYM_DOUBLE_COLON, SYM_DROP, SYM_EVAL, SYM_FUNCTION, SYM_LASTVALUE, SYM_LEVELS, SYM_MODE,
    SYM_NA_RM, SYM_NAME, SYM_NAMES, SYM_NAMESPACE_ENV, SYM_PACKAGE, SYM_PREVIOUS, SYM_QUOTE,
    SYM_ROW_NAMES, SYM_SEEDS, SYM_SORT_LIST, SYM_SOURCE, SYM_SPEC, SYM_SRCFILE, SYM_SRCREF,
    SYM_TRIPLE_COLON, SYM_TSP, TRUE_SCALAR,
};

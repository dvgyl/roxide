#![forbid(unsafe_code)]

//! Core GNU R 4.6.2 data model, exact `NA_real_` (`1954`) semantics, `ALTREP`,
//! AST lowering, number/string formatting, and `deparse()`.
//!
//! `RObject` internals remain `pub(crate)` (ADR-0007); downstream crates access
//! heap objects strictly through [`RSession`], [`ValueApi`], and [`VecView`].

mod attributes;
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
    BASE_ENV, BASE_NAMESPACE, EMPTY_ENV, FALSE_SCALAR, GLOBAL_ENV, NA_LGL_SCALAR, RHeap, RSession,
    SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, TRUE_SCALAR,
};

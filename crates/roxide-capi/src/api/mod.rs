//! Public GNU R 4.6.2 C API entry points grouped into disjoint implementation families (M4 W1).

// Wave 1-A: core vector, attribute, coercion, pairlist, string, symbol, and resizable vector APIs.
pub mod attrib;
pub mod coerce;
pub mod pairlist;
pub mod resizable;
pub mod string;
pub mod sym;
pub mod vector;

// Wave 1-B: environment, evaluation, error/condition, printing, unwind, and parse APIs.
pub mod env;
pub mod error;
pub mod eval;
pub mod parse;
pub mod print;
pub mod unwind;

// Wave 1-C: memory/transient alloc, external pointers, weakrefs, Rmath, PRNG, arithmetic, utils, S4.
pub mod arith;
pub mod extptr;
pub mod memory;
pub mod random;
pub mod rmath;
pub mod s4;
pub mod utils;
pub mod weakref;

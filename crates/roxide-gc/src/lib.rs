#![forbid(unsafe_code)]

//! Cycle-safe safepoint mark-sweep garbage collector for Roxide.
//!
//! Implements ADR-0002 and ADR-0003:
//! - Single canonical handle type [`SexpId`] (`NonZeroU64`, C `SEXP`-compatible bit layout).
//! - Allocation never triggers collection; collection runs exclusively at explicit safepoints.
//! - Iterative two-phase mark-and-sweep (`Heap::mark` -> `Marked::sweep`) so weak tables
//!   such as `CHARSXP` interning can purge unmarked entries before slots are reclaimed.
//! - Saturating copy-on-modify reference counts (`REFCNT_MAX = 7`, sticky once saturated).

pub mod heap;
pub mod id;

pub use heap::{
    DEFAULT_PROTECT_LIMIT, Finalizer, FinalizerKind, GcPolicy, GcStats, Heap, HeapError, HeapSize,
    Marked, ProtectIndex, ProtectMark, REFCNT_MAX, RootSet, SweepOutcome, Trace, Tracer,
    WeakRefEntry,
};
pub use id::{MAX_GENERATION, SexpId};

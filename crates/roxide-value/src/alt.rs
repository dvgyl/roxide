//! `ALTREP` traits, compact sequence implementations, and R 4.6.2 wrapper duplication guards.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use roxide_gc::{SexpId, Trace, Tracer};

use crate::api::ValueError;
use crate::na::{Elem, RComplex, RLogical};
use crate::object::{CharId, RVec, SexpType};
use crate::session::{RHeap, RSession};

/// Known sortedness metadata exposed by ALTREP vectors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sortedness {
    /// Sortedness unknown.
    Unknown,
    /// Known unsorted.
    Unsorted,
    /// Increasing with NAs last (or no NAs).
    Increasing,
    /// Decreasing with NAs last (or no NAs).
    Decreasing,
    /// Increasing with NAs first.
    IncreasingNaFirst,
    /// Decreasing with NAs first.
    DecreasingNaFirst,
}

/// Base ALTREP class metadata and serialization hooks.
pub trait AltClass: Trace + fmt::Debug + 'static {
    /// ALTREP class name (e.g. `"compact_intseq"`).
    fn class_name(&self) -> &'static str;
    /// ALTREP package name (e.g. `"base"`).
    fn package_name(&self) -> &'static str;
    /// Vector length without materialization.
    fn length(&self, heap: &RHeap) -> usize;
    /// Optional diagnostic summary for `.Internal(inspect(x))`.
    fn inspect(&self, _heap: &RHeap) -> String {
        String::new()
    }
    /// Optional serialized state for XDR v3 (`None` writes materialized contents).
    fn serialized_state(&self, _session: &mut RSession) -> Result<Option<SexpId>, ValueError> {
        Ok(None)
    }
    /// Optional custom duplication (`None` materializes a private copy).
    fn duplicate(
        &self,
        _session: &mut RSession,
        _deep: bool,
    ) -> Result<Option<SexpId>, ValueError> {
        Ok(None)
    }
    /// Optional custom coercion (`None` falls back to standard coercion).
    fn coerce(&self, _session: &mut RSession, _to: SexpType) -> Result<Option<SexpId>, ValueError> {
        Ok(None)
    }
    /// If this object is an ALTREP metadata wrapper, return the wrapped backing handle
    /// so the R 4.6.2 read-only/writable wrapper duplication guard can check its sharing.
    fn wrapped_target(&self) -> Option<SexpId> {
        None
    }
}

/// Typed ALTREP atomic vector interface.
pub trait AltVector<T: Elem>: AltClass {
    /// Read element `i` without materialization.
    fn elt(&self, heap: &RHeap, i: usize) -> T;

    /// Copy up to `out.len()` elements starting at `start`, returning count copied.
    fn get_region(&self, heap: &RHeap, start: usize, out: &mut [T]) -> usize {
        let len = self.length(heap);
        if start >= len {
            return 0;
        }
        let n = (len - start).min(out.len());
        for (offset, slot) in out[..n].iter_mut().enumerate() {
            *slot = self.elt(heap, start + offset);
        }
        n
    }

    /// Optional read-only direct backing slice if already contiguous in memory.
    fn dataptr_or_null<'h>(&'h self, _heap: &'h RHeap) -> Option<&'h [T]> {
        None
    }

    /// Materialize the full vector into an owned boxed slice.
    fn materialize(&self, heap: &RHeap) -> Result<Box<[T]>, ValueError> {
        let len = self.length(heap);
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(self.elt(heap, i));
        }
        Ok(out.into_boxed_slice())
    }

    /// Known sortedness metadata.
    fn is_sorted(&self, _heap: &RHeap) -> Sortedness {
        Sortedness::Unknown
    }

    /// Return true if guaranteed to contain no missing values.
    fn no_na(&self, _heap: &RHeap) -> bool {
        false
    }
}

/// ALTREP character vector interface (methods take `&mut RSession` because `CHARSXP` allocation may occur).
pub trait AltString: AltClass {
    /// Read character element `i`.
    fn elt(&self, session: &mut RSession, i: usize) -> Result<CharId, ValueError>;
    /// Write character element `i`.
    fn set_elt(&self, session: &mut RSession, i: usize, val: CharId) -> Result<(), ValueError>;
    /// Known sortedness metadata.
    fn is_sorted(&self) -> Sortedness {
        Sortedness::Unknown
    }
    /// Return true if guaranteed to contain no `NA_STRING`.
    fn no_na(&self) -> bool {
        false
    }
}

/// Compact integer sequence (`a:b` when endpoints fit `i32`).
#[derive(Clone, Debug)]
pub struct CompactIntSeq {
    /// Starting integer value.
    pub start: i32,
    /// Sequence length.
    pub len: usize,
    /// Step (`1` or `-1`).
    pub incr: i8,
}

impl Trace for CompactIntSeq {
    fn trace(&self, _t: &mut Tracer<'_>) {}
}

impl AltClass for CompactIntSeq {
    fn class_name(&self) -> &'static str {
        "compact_intseq"
    }

    fn package_name(&self) -> &'static str {
        "base"
    }

    fn length(&self, _heap: &RHeap) -> usize {
        self.len
    }
}

impl AltVector<i32> for CompactIntSeq {
    #[inline]
    fn elt(&self, _heap: &RHeap, i: usize) -> i32 {
        (i64::from(self.start) + (i as i64) * i64::from(self.incr)) as i32
    }

    fn is_sorted(&self, _heap: &RHeap) -> Sortedness {
        if self.incr >= 0 {
            Sortedness::Increasing
        } else {
            Sortedness::Decreasing
        }
    }

    fn no_na(&self, _heap: &RHeap) -> bool {
        true
    }
}

/// Compact double sequence (for ranges exceeding `i32::MAX`).
#[derive(Clone, Debug)]
pub struct CompactRealSeq {
    /// Starting value.
    pub start: f64,
    /// Sequence length.
    pub len: usize,
    /// Step (`+1.0` or `-1.0`).
    pub incr: f64,
}

impl Trace for CompactRealSeq {
    fn trace(&self, _t: &mut Tracer<'_>) {}
}

impl AltClass for CompactRealSeq {
    fn class_name(&self) -> &'static str {
        "compact_realseq"
    }

    fn package_name(&self) -> &'static str {
        "base"
    }

    fn length(&self, _heap: &RHeap) -> usize {
        self.len
    }
}

impl AltVector<f64> for CompactRealSeq {
    #[inline]
    fn elt(&self, _heap: &RHeap, i: usize) -> f64 {
        self.start + (i as f64) * self.incr
    }

    fn is_sorted(&self, _heap: &RHeap) -> Sortedness {
        if self.incr >= 0.0 {
            Sortedness::Increasing
        } else {
            Sortedness::Decreasing
        }
    }

    fn no_na(&self, _heap: &RHeap) -> bool {
        true
    }
}

/// Internal typed ALTREP dispatch enum (all base types reserved for M1-M4).
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum AltKind {
    Lgl(Rc<dyn AltVector<RLogical>>),
    Int(Rc<dyn AltVector<i32>>),
    Real(Rc<dyn AltVector<f64>>),
    Cplx(Rc<dyn AltVector<RComplex>>),
    Raw(Rc<dyn AltVector<u8>>),
    Str(Rc<dyn AltString>),
}

/// Materialized backing storage for an [`AltObj`].
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) enum AltMaterialized {
    Lgl(RVec<RLogical>),
    Int(RVec<i32>),
    Real(RVec<f64>),
    Cplx(RVec<RComplex>),
    Raw(RVec<u8>),
    Str(RVec<CharId>),
}

/// Heap body for an ALTREP vector.
#[derive(Debug)]
pub(crate) struct AltObj {
    pub(crate) kind: AltKind,
    pub(crate) materialized: Option<AltMaterialized>,
}

impl AltObj {
    pub(crate) fn base_type(&self) -> SexpType {
        match &self.kind {
            AltKind::Lgl(_) => SexpType::Lgl,
            AltKind::Int(_) => SexpType::Int,
            AltKind::Real(_) => SexpType::Real,
            AltKind::Cplx(_) => SexpType::Cplx,
            AltKind::Raw(_) => SexpType::Raw,
            AltKind::Str(_) => SexpType::Str,
        }
    }

    pub(crate) fn wrapped_target(&self) -> Option<SexpId> {
        match &self.kind {
            AltKind::Lgl(c) => c.wrapped_target(),
            AltKind::Int(c) => c.wrapped_target(),
            AltKind::Real(c) => c.wrapped_target(),
            AltKind::Cplx(c) => c.wrapped_target(),
            AltKind::Raw(c) => c.wrapped_target(),
            AltKind::Str(c) => c.wrapped_target(),
        }
    }

    pub(crate) fn len(&self, heap: &RHeap) -> usize {
        if let Some(mat) = &self.materialized {
            return match mat {
                AltMaterialized::Lgl(v) => v.len(),
                AltMaterialized::Int(v) => v.len(),
                AltMaterialized::Real(v) => v.len(),
                AltMaterialized::Cplx(v) => v.len(),
                AltMaterialized::Raw(v) => v.len(),
                AltMaterialized::Str(v) => v.len(),
            };
        }
        match &self.kind {
            AltKind::Lgl(c) => c.length(heap),
            AltKind::Int(c) => c.length(heap),
            AltKind::Real(c) => c.length(heap),
            AltKind::Cplx(c) => c.length(heap),
            AltKind::Raw(c) => c.length(heap),
            AltKind::Str(c) => c.length(heap),
        }
    }

    pub(crate) fn trace(&self, t: &mut Tracer<'_>) {
        match &self.kind {
            AltKind::Lgl(c) => c.trace(t),
            AltKind::Int(c) => c.trace(t),
            AltKind::Real(c) => c.trace(t),
            AltKind::Cplx(c) => c.trace(t),
            AltKind::Raw(c) => c.trace(t),
            AltKind::Str(c) => c.trace(t),
        }
        if let Some(AltMaterialized::Str(v)) = &self.materialized {
            for c in v.as_slice() {
                t.edge(c.as_sexp());
            }
        }
    }
}

/// Callback type for reconstructing an ALTREP object from XDR v3 serialized state.
pub type AltUnserialize =
    fn(&mut RSession, state: SexpId, attrs: SexpId) -> Result<SexpId, ValueError>;

/// Registry mapping `(class_name, package_name)` to [`AltUnserialize`] constructors.
#[derive(Default)]
pub struct AltRegistry {
    entries: HashMap<(String, String), AltUnserialize>,
}

impl AltRegistry {
    /// Create an empty [`AltRegistry`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an ALTREP unserializer for `(class, package)`.
    pub fn register(
        &mut self,
        class: impl Into<String>,
        package: impl Into<String>,
        func: AltUnserialize,
    ) {
        self.entries.insert((class.into(), package.into()), func);
    }

    /// Look up an ALTREP unserializer by `(class, package)`.
    #[must_use]
    pub fn get(&self, class: &str, package: &str) -> Option<AltUnserialize> {
        self.entries
            .get(&(class.to_owned(), package.to_owned()))
            .copied()
    }
}

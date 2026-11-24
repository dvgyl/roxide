//! `ALTREP` traits, compact sequence implementations, and R 4.6.2 wrapper duplication guards.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use roxide_gc::{HeapError, SexpId, Trace, Tracer};

use crate::api::{ValueApi, ValueError};
use crate::na::{Elem, NA_INTEGER, RComplex, RLogical};
use crate::object::{Body, CharId, RObject, RVec, SexpType};
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
    /// Return true if backed by a foreign C ALTREP class dispatched by `roxide-capi` (ADR-0019).
    fn is_foreign(&self) -> bool {
        false
    }
}

/// Foreign C ALTREP object payload traced by `roxide-gc` (single-authority rule, ADR-0019).
///
/// Method tables and callback dispatch live exclusively in `roxide-capi`.
#[derive(Clone, Debug)]
pub struct ForeignAltObj {
    /// Class identifier allocated by `roxide-capi`'s foreign ALTREP registry.
    pub class_id: u32,
    /// Underlying R vector [`SexpType`] (`Lgl`, `Int`, `Real`, `Cplx`, `Raw`, `Str`, `Vec`).
    pub base_type: SexpType,
    /// Class name registered with `R_make_alt*_class`.
    pub name: String,
    /// Package name registered with `R_make_alt*_class`.
    pub package: String,
    /// First GC-traced state slot (`R_altrep_data1`).
    pub data1: SexpId,
    /// Second GC-traced state slot (`R_altrep_data2`).
    pub data2: SexpId,
}

impl Trace for ForeignAltObj {
    fn trace(&self, t: &mut Tracer<'_>) {
        t.edge(self.data1);
        t.edge(self.data2);
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
        let mut out = Vec::new();
        out.try_reserve_exact(len)
            .map_err(|_| HeapError::OutOfMemory {
                requested: len.saturating_mul(size_of::<T>()),
            })?;
        for i in 0..len {
            out.push(self.elt(heap, i));
        }
        Ok(out.into_boxed_slice())
    }

    /// Return the endpoints of a known unit-step sequence with at least two elements.
    fn unit_sequence_bounds(&self, _heap: &RHeap) -> Option<(T, T)> {
        None
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
        let value = i128::from(self.start) + (i as i128) * i128::from(self.incr);
        i32::try_from(value).unwrap_or(NA_INTEGER)
    }

    fn unit_sequence_bounds(&self, heap: &RHeap) -> Option<(i32, i32)> {
        if self.len < 2 || !matches!(self.incr, -1 | 1) {
            return None;
        }
        let last = self.elt(heap, self.len - 1);
        (self.start != NA_INTEGER && last != NA_INTEGER).then_some((self.start, last))
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
        if i == 0 {
            self.start
        } else {
            self.start + (i as f64) * self.incr
        }
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
    Foreign(ForeignAltObj),
}

/// Materialized backing storage for an [`AltObj`].
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum AltMaterialized {
    Lgl(RVec<RLogical>),
    Int(RVec<i32>),
    Real(RVec<f64>),
    Cplx(RVec<RComplex>),
    Raw(RVec<u8>),
    Str(RVec<CharId>),
    Vec(RVec<SexpId>),
}

/// Heap body for an ALTREP vector.
#[derive(Clone, Debug)]
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
            AltKind::Foreign(f) => f.base_type,
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
            AltKind::Foreign(_) => None,
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
                AltMaterialized::Vec(v) => v.len(),
            };
        }
        match &self.kind {
            AltKind::Lgl(c) => c.length(heap),
            AltKind::Int(c) => c.length(heap),
            AltKind::Real(c) => c.length(heap),
            AltKind::Cplx(c) => c.length(heap),
            AltKind::Raw(c) => c.length(heap),
            AltKind::Str(c) => c.length(heap),
            AltKind::Foreign(_) => 0,
        }
    }

    pub(crate) fn buffer_bytes(&self) -> usize {
        match &self.materialized {
            Some(AltMaterialized::Lgl(v)) => std::mem::size_of_val(&*v.buf),
            Some(AltMaterialized::Int(v)) => std::mem::size_of_val(&*v.buf),
            Some(AltMaterialized::Real(v)) => std::mem::size_of_val(&*v.buf),
            Some(AltMaterialized::Cplx(v)) => std::mem::size_of_val(&*v.buf),
            Some(AltMaterialized::Raw(v)) => v.buf.len(),
            Some(AltMaterialized::Str(v)) => std::mem::size_of_val(&*v.buf),
            Some(AltMaterialized::Vec(v)) => std::mem::size_of_val(&*v.buf),
            None => 0,
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
            AltKind::Foreign(f) => f.trace(t),
        }
        match &self.materialized {
            Some(AltMaterialized::Str(v)) => {
                for c in v.as_slice() {
                    t.edge(c.as_sexp());
                }
            }
            Some(AltMaterialized::Vec(v)) => {
                t.edges(v.as_slice());
            }
            _ => {}
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

/// ALTREP metadata wrapper for an atomic vector.
#[derive(Clone, Debug)]
pub struct AltWrapper {
    target: SexpId,
    len: usize,
}

impl AltWrapper {
    /// Get the handle of the wrapped vector.
    #[must_use]
    pub const fn target(&self) -> SexpId {
        self.target
    }

    /// Allocate a wrapper for an atomic vector.
    pub fn alloc(session: &mut RSession, target: SexpId) -> Result<SexpId, ValueError> {
        let wrapper = Rc::new(Self {
            target,
            len: session.xlength(target)?,
        });
        let (kind, materialized) = match session.type_of(target)? {
            SexpType::Lgl => (AltKind::Lgl(wrapper), None),
            SexpType::Int => (AltKind::Int(wrapper), None),
            SexpType::Real => (AltKind::Real(wrapper), None),
            SexpType::Cplx => (AltKind::Cplx(wrapper), None),
            SexpType::Raw => (AltKind::Raw(wrapper), None),
            SexpType::Str => {
                let data = RVec::from_vec(session.str_slice(target)?.to_vec());
                (AltKind::Str(wrapper), Some(AltMaterialized::Str(data)))
            }
            actual => {
                return Err(ValueError::TypeMismatch {
                    expected: "atomic vector",
                    actual: actual.type_name(false),
                });
            }
        };
        let id = session.alloc_alt(kind, materialized)?;
        session.heap.inc_ref(target);
        Ok(id)
    }
}

impl Trace for AltWrapper {
    fn trace(&self, t: &mut Tracer<'_>) {
        t.edge(self.target);
    }
}

impl AltClass for AltWrapper {
    fn class_name(&self) -> &'static str {
        "wrap"
    }

    fn package_name(&self) -> &'static str {
        "base"
    }

    fn length(&self, _heap: &RHeap) -> usize {
        self.len
    }

    fn wrapped_target(&self) -> Option<SexpId> {
        Some(self.target)
    }
}

impl AltString for AltWrapper {
    fn elt(&self, session: &mut RSession, i: usize) -> Result<CharId, ValueError> {
        let values = session.str_slice(self.target)?;
        values.get(i).copied().ok_or(ValueError::IndexOutOfBounds {
            index: i,
            len: values.len(),
        })
    }

    fn set_elt(&self, _session: &mut RSession, _i: usize, _val: CharId) -> Result<(), ValueError> {
        Err(ValueError::Shared(self.target))
    }
}

macro_rules! atomic_dataptr {
    ($ty:ty, $default:expr, $variant:ident, $prepare:ident, $read:ident, $write:ident) => {
        impl AltVector<$ty> for AltWrapper {
            fn elt(&self, heap: &RHeap, i: usize) -> $ty {
                let mut target = self.target;
                while let Ok(obj) = heap.get(target) {
                    match &obj.body {
                        Body::$variant(v) => {
                            return v.as_slice().get(i).copied().unwrap_or($default);
                        }
                        Body::Alt(alt) => match (&alt.materialized, &alt.kind) {
                            (Some(AltMaterialized::$variant(v)), _) => {
                                return v.as_slice().get(i).copied().unwrap_or($default);
                            }
                            (None, AltKind::$variant(class)) => {
                                if let Some(inner) = class.wrapped_target() {
                                    target = inner;
                                } else {
                                    return class.elt(heap, i);
                                }
                            }
                            _ => break,
                        },
                        _ => break,
                    }
                }
                $default
            }
        }

        impl RSession {
            fn $prepare(&mut self, x: SexpId) -> Result<&mut RVec<$ty>, ValueError> {
                let obj = self.heap.get(x)?;
                let mismatch = ValueError::TypeMismatch {
                    expected: <$ty as Elem>::TYPE.type_name(false),
                    actual: obj.sexp_type().type_name(obj.flags.s4),
                };
                if let Body::Alt(alt) = &obj.body
                    && alt.materialized.is_none()
                {
                    let AltKind::$variant(class) = &alt.kind else {
                        return Err(mismatch);
                    };
                    // A wrapper gets its own buffer before either pointer access.
                    let buf = class.materialize(&self.heap)?;
                    let len = buf.len();
                    if let Body::Alt(alt) = &mut self.heap.get_mut(x)?.body {
                        alt.materialized = Some(AltMaterialized::$variant(RVec {
                            buf,
                            len,
                            resizable: false,
                        }));
                    }
                    self.heap.update_size(x)?;
                }
                match &mut self.heap.get_mut(x)?.body {
                    Body::$variant(v) => Ok(v),
                    Body::Alt(alt) => match &mut alt.materialized {
                        Some(AltMaterialized::$variant(v)) => Ok(v),
                        _ => Err(mismatch),
                    },
                    _ => Err(mismatch),
                }
            }

            /// Get a contiguous read-only slice. Materialize a private buffer for ALTREP.
            pub fn $read(&mut self, x: SexpId) -> Result<&[$ty], ValueError> {
                Ok(self.$prepare(x)?.as_slice())
            }

            /// Get a contiguous writable slice from an unshared vector.
            pub fn $write(&mut self, x: SexpId) -> Result<&mut [$ty], ValueError> {
                if self.heap.maybe_shared(x) {
                    return Err(ValueError::Shared(x));
                }
                Ok(self.$prepare(x)?.as_mut_slice())
            }
        }
    };
}

atomic_dataptr!(
    RLogical,
    RLogical::NA,
    Lgl,
    prepare_lgl,
    lgl_dataptr_ro,
    lgl_dataptr_mut
);
atomic_dataptr!(
    i32,
    NA_INTEGER,
    Int,
    prepare_int,
    int_dataptr_ro,
    int_dataptr_mut
);
atomic_dataptr!(
    f64,
    crate::na::NA_REAL,
    Real,
    prepare_real,
    real_dataptr_ro,
    real_dataptr_mut
);
atomic_dataptr!(
    RComplex,
    RComplex::NA,
    Cplx,
    prepare_cplx,
    cplx_dataptr_ro,
    cplx_dataptr_mut
);
atomic_dataptr!(u8, 0, Raw, prepare_raw, raw_dataptr_ro, raw_dataptr_mut);

impl RSession {
    /// Allocate an ALTREP wrapper for an atomic vector.
    pub fn alloc_alt_wrapper(&mut self, target: SexpId) -> Result<SexpId, ValueError> {
        AltWrapper::alloc(self, target)
    }

    /// Allocate a compact integer sequence with a step of 1 or -1.
    pub fn alloc_compact_intseq(
        &mut self,
        start: i32,
        len: usize,
        incr: i8,
    ) -> Result<SexpId, ValueError> {
        let end = i128::from(start) + (len.saturating_sub(1) as i128) * i128::from(incr);
        if !matches!(incr, 1 | -1)
            || (len > 0
                && (start == NA_INTEGER
                    || !(-i128::from(i32::MAX)..=i128::from(i32::MAX)).contains(&end)))
        {
            return Err(ValueError::InvalidAttribute(
                "invalid compact integer sequence".into(),
            ));
        }
        self.alloc_alt(
            AltKind::Int(Rc::new(CompactIntSeq { start, len, incr })),
            None,
        )
    }

    /// Allocate a compact double sequence with a finite start and a step of 1 or -1.
    pub fn alloc_compact_realseq(
        &mut self,
        start: f64,
        len: usize,
        incr: f64,
    ) -> Result<SexpId, ValueError> {
        if !start.is_finite() || incr.abs() != 1.0 {
            return Err(ValueError::InvalidAttribute(
                "invalid compact double sequence".into(),
            ));
        }
        self.alloc_alt(
            AltKind::Real(Rc::new(CompactRealSeq { start, len, incr })),
            None,
        )
    }

    fn alloc_alt(
        &mut self,
        kind: AltKind,
        materialized: Option<AltMaterialized>,
    ) -> Result<SexpId, ValueError> {
        Ok(self.heap.alloc(RObject::new(Body::Alt(Box::new(AltObj {
            kind,
            materialized,
        }))))?)
    }
}

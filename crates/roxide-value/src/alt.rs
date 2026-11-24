//! `ALTREP` traits, compact sequence implementations, and R 4.6.2 wrapper duplication guards.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use roxide_gc::{HeapError, SexpId, Trace, Tracer};

use crate::api::{AltNeed, AltSlot, ForeignAltInfo, ValueApi, ValueError};
use crate::na::{Elem, NA_INTEGER, RComplex, RLogical};
use crate::object::{Body, CharId, RObject, RVec, SexpType, Sym};
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

/// Base ALTREP class metadata.
pub trait AltClass: Trace + fmt::Debug + 'static {
    /// ALTREP class name (e.g. `"compact_intseq"`).
    fn class_name(&self) -> &'static str;
    /// ALTREP package name (e.g. `"base"`).
    fn package_name(&self) -> &'static str;
    /// Vector length without materialization.
    fn length(&self, heap: &RHeap) -> usize;
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

#[derive(Clone, Debug)]
pub(crate) struct ForeignAlt {
    pub(crate) class: SexpId,
    pub(crate) ty: SexpType,
    pub(crate) data1: SexpId,
    pub(crate) data2: SexpId,
    pub(crate) len: Option<usize>,
    pub(crate) dirty: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum AltKind {
    Lgl(Rc<dyn AltVector<RLogical>>),
    Int(Rc<dyn AltVector<i32>>),
    Real(Rc<dyn AltVector<f64>>),
    Cplx(Rc<dyn AltVector<RComplex>>),
    Raw(Rc<dyn AltVector<u8>>),
    Str(Rc<dyn AltString>),
    Foreign(ForeignAlt),
}

impl AltKind {
    fn builtin_class(&self) -> Option<&dyn AltClass> {
        Some(match self {
            Self::Lgl(c) => c.as_ref(),
            Self::Int(c) => c.as_ref(),
            Self::Real(c) => c.as_ref(),
            Self::Cplx(c) => c.as_ref(),
            Self::Raw(c) => c.as_ref(),
            Self::Str(c) => c.as_ref(),
            Self::Foreign(_) => return None,
        })
    }
}

/// Materialized backing storage for an [`AltObj`].
#[derive(Clone, Debug)]
pub(crate) enum AltMaterialized {
    Lgl(RVec<RLogical>),
    Int(RVec<i32>),
    Real(RVec<f64>),
    Cplx(RVec<RComplex>),
    Raw(RVec<u8>),
    Str(RVec<CharId>),
    List(RVec<SexpId>),
}

impl AltMaterialized {
    fn len(&self) -> usize {
        match self {
            Self::Lgl(v) => v.len(),
            Self::Int(v) => v.len(),
            Self::Real(v) => v.len(),
            Self::Cplx(v) => v.len(),
            Self::Raw(v) => v.len(),
            Self::Str(v) => v.len(),
            Self::List(v) => v.len(),
        }
    }

    fn take_dense(body: &mut Body) -> Option<Self> {
        fn take<T>(v: &mut RVec<T>) -> RVec<T> {
            std::mem::replace(v, RVec::from_vec(Vec::new()))
        }
        Some(match body {
            Body::Lgl(v) => Self::Lgl(take(v)),
            Body::Int(v) => Self::Int(take(v)),
            Body::Real(v) => Self::Real(take(v)),
            Body::Cplx(v) => Self::Cplx(take(v)),
            Body::Raw(v) => Self::Raw(take(v)),
            Body::Str(v) => Self::Str(take(v)),
            Body::Vec(v) => Self::List(take(v)),
            _ => return None,
        })
    }

    fn dense_copy(&self) -> Body {
        match self {
            Self::Lgl(v) => Body::Lgl(v.clone()),
            Self::Int(v) => Body::Int(v.clone()),
            Self::Real(v) => Body::Real(v.clone()),
            Self::Cplx(v) => Body::Cplx(v.clone()),
            Self::Raw(v) => Body::Raw(v.clone()),
            Self::Str(v) => Body::Str(v.clone()),
            Self::List(v) => Body::Vec(v.clone()),
        }
    }

    fn for_each_edge(&self, visit: impl FnMut(SexpId)) {
        match self {
            Self::Str(v) => v.as_slice().iter().map(|c| c.as_sexp()).for_each(visit),
            Self::List(v) => v.as_slice().iter().copied().for_each(visit),
            _ => {}
        }
    }
}

pub(crate) type AltDrops = Rc<RefCell<Vec<SexpId>>>;

/// Heap body for an ALTREP vector.
#[derive(Debug)]
pub(crate) struct AltObj {
    pub(crate) kind: AltKind,
    pub(crate) materialized: Option<AltMaterialized>,
    dropped_edges: Option<AltDrops>,
}

impl AltObj {
    fn new(kind: AltKind, materialized: Option<AltMaterialized>) -> Self {
        Self {
            kind,
            materialized,
            dropped_edges: None,
        }
    }

    pub(crate) fn duplicate_body(&self, x: SexpId) -> Result<Body, ValueError> {
        if matches!(self.kind, AltKind::Foreign(_)) {
            return self
                .materialized
                .as_ref()
                .map(AltMaterialized::dense_copy)
                .ok_or(ValueError::ForeignAltrep {
                    id: x,
                    need: AltNeed::Payload,
                });
        }
        Ok(Body::Alt(Box::new(Self::new(
            self.kind.clone(),
            self.materialized.clone(),
        ))))
    }

    pub(crate) fn require_payload(&self, x: SexpId) -> Result<(), ValueError> {
        if self.materialized.is_none() && matches!(self.kind, AltKind::Foreign(_)) {
            return Err(ValueError::ForeignAltrep {
                id: x,
                need: AltNeed::Payload,
            });
        }
        Ok(())
    }

    pub(crate) fn base_type(&self) -> SexpType {
        match &self.kind {
            AltKind::Lgl(_) => SexpType::Lgl,
            AltKind::Int(_) => SexpType::Int,
            AltKind::Real(_) => SexpType::Real,
            AltKind::Cplx(_) => SexpType::Cplx,
            AltKind::Raw(_) => SexpType::Raw,
            AltKind::Str(_) => SexpType::Str,
            AltKind::Foreign(foreign) => foreign.ty,
        }
    }

    pub(crate) fn wrapped_target(&self) -> Option<SexpId> {
        self.kind.builtin_class().and_then(AltClass::wrapped_target)
    }

    pub(crate) fn len(&self, heap: &RHeap, x: SexpId) -> Result<usize, ValueError> {
        if let Some(mat) = &self.materialized {
            return Ok(mat.len());
        }
        let len = match &self.kind {
            AltKind::Foreign(foreign) => foreign.len,
            kind => kind.builtin_class().map(|class| class.length(heap)),
        };
        len.ok_or(ValueError::ForeignAltrep {
            id: x,
            need: AltNeed::Length,
        })
    }

    pub(crate) fn buffer_bytes(&self) -> usize {
        match &self.materialized {
            Some(AltMaterialized::Lgl(v)) => std::mem::size_of_val(&*v.buf),
            Some(AltMaterialized::Int(v)) => std::mem::size_of_val(&*v.buf),
            Some(AltMaterialized::Real(v)) => std::mem::size_of_val(&*v.buf),
            Some(AltMaterialized::Cplx(v)) => std::mem::size_of_val(&*v.buf),
            Some(AltMaterialized::Raw(v)) => v.buf.len(),
            Some(AltMaterialized::Str(v)) => std::mem::size_of_val(&*v.buf),
            Some(AltMaterialized::List(v)) => std::mem::size_of_val(&*v.buf),
            None => 0,
        }
    }

    pub(crate) fn trace(&self, t: &mut Tracer<'_>) {
        if let Some(class) = self.kind.builtin_class() {
            class.trace(t);
        }
        self.for_each_edge(|id| t.edge(id));
    }

    fn for_each_edge(&self, mut visit: impl FnMut(SexpId)) {
        if let AltKind::Foreign(foreign) = &self.kind {
            [foreign.class, foreign.data1, foreign.data2]
                .into_iter()
                .for_each(&mut visit);
        } else if let Some(target) = self.wrapped_target() {
            visit(target);
        }
        if let Some(mat) = &self.materialized {
            mat.for_each_edge(visit);
        }
    }
}

impl Drop for AltObj {
    fn drop(&mut self) {
        if let Some(queue) = &self.dropped_edges {
            let mut queue = queue.borrow_mut();
            self.for_each_edge(|id| queue.push(id));
        }
    }
}

/// ALTREP metadata wrapper for an atomic vector.
#[derive(Clone, Debug)]
pub struct AltWrapper {
    target: SexpId,
    len: usize,
    class_name: &'static str,
}

impl AltWrapper {
    /// Get the handle of the wrapped vector.
    #[must_use]
    pub const fn target(&self) -> SexpId {
        self.target
    }

    /// Allocate a wrapper for an atomic vector.
    pub fn alloc(session: &mut RSession, target: SexpId) -> Result<SexpId, ValueError> {
        let obj = session.heap.get(target)?;
        if let Body::Alt(alt) = &obj.body {
            alt.require_payload(target)?;
        }
        let len = session.xlength(target)?;
        let wrapper = |class_name| {
            Rc::new(Self {
                target,
                len,
                class_name,
            })
        };
        match obj.sexp_type() {
            SexpType::Lgl => session.alloc_alt_lgl(wrapper("wrap_logical")),
            SexpType::Int => session.alloc_alt_int(wrapper("wrap_integer")),
            SexpType::Real => session.alloc_alt_real(wrapper("wrap_real")),
            SexpType::Cplx => session.alloc_alt_cplx(wrapper("wrap_complex")),
            SexpType::Raw => session.alloc_alt_raw(wrapper("wrap_raw")),
            SexpType::Str => session.alloc_alt_str(wrapper("wrap_string")),
            _ => Err(obj.type_mismatch("atomic vector")),
        }
    }
}

impl Trace for AltWrapper {
    fn trace(&self, t: &mut Tracer<'_>) {
        t.edge(self.target);
    }
}

impl AltClass for AltWrapper {
    fn class_name(&self) -> &'static str {
        self.class_name
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
                let mismatch = obj.type_mismatch(<$ty as Elem>::TYPE.type_name(false));
                if let Body::Alt(alt) = &obj.body
                    && alt.materialized.is_none()
                {
                    alt.require_payload(x)?;
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
                self.require_alt_payload(x)?;
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
        self.alloc_alt_int(Rc::new(CompactIntSeq { start, len, incr }))
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
        self.alloc_alt_real(Rc::new(CompactRealSeq { start, len, incr }))
    }

    pub(crate) fn alloc_alt_lgl(
        &mut self,
        class: Rc<dyn AltVector<RLogical>>,
    ) -> Result<SexpId, ValueError> {
        self.alloc_alt(AltKind::Lgl(class), None)
    }

    pub(crate) fn alloc_alt_int(
        &mut self,
        class: Rc<dyn AltVector<i32>>,
    ) -> Result<SexpId, ValueError> {
        self.alloc_alt(AltKind::Int(class), None)
    }

    pub(crate) fn alloc_alt_real(
        &mut self,
        class: Rc<dyn AltVector<f64>>,
    ) -> Result<SexpId, ValueError> {
        self.alloc_alt(AltKind::Real(class), None)
    }

    pub(crate) fn alloc_alt_cplx(
        &mut self,
        class: Rc<dyn AltVector<RComplex>>,
    ) -> Result<SexpId, ValueError> {
        self.alloc_alt(AltKind::Cplx(class), None)
    }

    pub(crate) fn alloc_alt_raw(
        &mut self,
        class: Rc<dyn AltVector<u8>>,
    ) -> Result<SexpId, ValueError> {
        self.alloc_alt(AltKind::Raw(class), None)
    }

    pub(crate) fn alloc_alt_str(&mut self, class: Rc<dyn AltString>) -> Result<SexpId, ValueError> {
        let materialized = if let Some(target) = class.wrapped_target() {
            Some(AltMaterialized::Str(RVec::from_vec(
                self.str_slice(target)?.to_vec(),
            )))
        } else {
            None
        };
        self.alloc_alt(AltKind::Str(class), materialized)
    }

    /// Return true if `x` is an ALTREP vector (`ALTREP(x)`).
    pub fn is_altrep(&self, x: SexpId) -> Result<bool, ValueError> {
        Ok(matches!(self.heap.get(x)?.body, Body::Alt(_)))
    }

    /// Allocate a class descriptor with a nonzero address that encodes the class index.
    pub fn alloc_alt_class(
        &mut self,
        name: Sym,
        package: Sym,
        index: usize,
    ) -> Result<SexpId, ValueError> {
        let addr = index.checked_add(1).ok_or_else(|| {
            ValueError::InvalidAttribute("ALTREP class index is too large".into())
        })?;
        self.sexp_as_sym(name.as_sexp())?;
        self.sexp_as_sym(package.as_sexp())?;
        let id = self.alloc_extptr(addr, name.as_sexp(), package.as_sexp())?;
        self.heap.get_mut(id)?.flags.alt_class = true;
        Ok(id)
    }

    /// Return the class and package symbols for a descriptor or ALTREP vector.
    pub fn alt_class_names(&self, x: SexpId) -> Result<Option<(Sym, Sym)>, ValueError> {
        let descriptor = match &self.heap.get(x)?.body {
            Body::Alt(alt) => match &alt.kind {
                AltKind::Foreign(foreign) => foreign.class,
                kind => {
                    return Ok(kind.builtin_class().and_then(|class| {
                        let name = self.interned_symbol(class.class_name())?;
                        let package = self.interned_symbol(class.package_name())?;
                        Some((name, package))
                    }));
                }
            },
            _ => x,
        };
        let obj = self.heap.get(descriptor)?;
        if let Body::ExtPtr(ptr) = &obj.body
            && obj.flags.alt_class
        {
            return Ok(Some((
                self.sexp_as_sym(ptr.tag)?,
                self.sexp_as_sym(ptr.prot)?,
            )));
        }
        Ok(None)
    }

    /// Decode the index stored in a class descriptor. Return `None` for a null address.
    pub fn alt_class_index(&self, descriptor: SexpId) -> Result<Option<usize>, ValueError> {
        let obj = self.heap.get(descriptor)?;
        Ok(match &obj.body {
            Body::ExtPtr(ptr) if obj.flags.alt_class => ptr.addr.checked_sub(1),
            _ => None,
        })
    }

    /// Allocate a lazy foreign ALTREP vector with an unknown length.
    pub fn alloc_foreign_alt(
        &mut self,
        class: SexpId,
        ty: SexpType,
        data1: SexpId,
        data2: SexpId,
    ) -> Result<SexpId, ValueError> {
        if !matches!(
            ty,
            SexpType::Lgl
                | SexpType::Int
                | SexpType::Real
                | SexpType::Cplx
                | SexpType::Raw
                | SexpType::Str
                | SexpType::Vec
        ) {
            return Err(ValueError::TypeMismatch {
                expected: "ALTREP vector type",
                actual: ty.type_name(false),
            });
        }
        self.extptr_addr(class)?;
        self.heap.get(data1)?;
        self.heap.get(data2)?;
        let id = self.alloc_alt(
            AltKind::Foreign(ForeignAlt {
                class,
                ty,
                data1,
                data2,
                len: None,
                dirty: false,
            }),
            None,
        )?;
        self.foreign_alt_seen = true;
        Ok(id)
    }

    /// Return the current data slots, cached length, and storage state of a foreign ALTREP vector.
    pub fn foreign_alt(&self, x: SexpId) -> Result<Option<ForeignAltInfo>, ValueError> {
        if let Body::Alt(alt) = &self.heap.get(x)?.body
            && let AltKind::Foreign(foreign) = &alt.kind
        {
            return Ok(Some(ForeignAltInfo {
                class: foreign.class,
                ty: foreign.ty,
                data1: foreign.data1,
                data2: foreign.data2,
                len: foreign.len,
                owned: alt.materialized.is_some(),
            }));
        }
        Ok(None)
    }

    /// Return whether native code can have changed the owned foreign ALTREP elements.
    pub fn foreign_alt_is_dirty(&self, x: SexpId) -> Result<bool, ValueError> {
        Ok(matches!(
            &self.heap.get(x)?.body,
            Body::Alt(alt) if matches!(&alt.kind, AltKind::Foreign(foreign) if foreign.dirty)
        ))
    }

    /// Record a write to owned foreign ALTREP elements or the issue of a writable pointer.
    pub fn mark_foreign_alt_dirty(&mut self, x: SexpId) -> Result<(), ValueError> {
        if let Body::Alt(alt) = &mut self.heap.get_mut(x)?.body
            && alt.materialized.is_some()
            && let AltKind::Foreign(foreign) = &mut alt.kind
        {
            foreign.dirty = true;
        }
        Ok(())
    }

    /// Replace a data slot and update the reference counts.
    pub fn set_foreign_alt_data(
        &mut self,
        x: SexpId,
        slot: AltSlot,
        v: SexpId,
    ) -> Result<(), ValueError> {
        self.heap.get(v)?;
        self.replace_edge(x, v, "foreign ALTREP", |body| {
            let Body::Alt(alt) = body else { return None };
            let AltKind::Foreign(foreign) = &mut alt.kind else {
                return None;
            };
            Some(match slot {
                AltSlot::Data1 => &mut foreign.data1,
                AltSlot::Data2 => &mut foreign.data2,
            })
        })
    }

    /// Cache the first length supplied for a foreign ALTREP vector.
    pub fn seal_foreign_alt_len(&mut self, x: SexpId, len: usize) -> Result<(), ValueError> {
        let obj = self.heap.get_mut(x)?;
        let mismatch = obj.type_mismatch("foreign ALTREP");
        if let Body::Alt(alt) = &mut obj.body
            && let AltKind::Foreign(foreign) = &mut alt.kind
        {
            foreign.len.get_or_insert(len);
            return Ok(());
        }
        Err(mismatch)
    }

    /// Move a dense vector's elements into a lazy foreign ALTREP vector and empty the source.
    /// Return false if the ALTREP vector already owns its elements.
    pub fn adopt_foreign_alt_payload(
        &mut self,
        x: SexpId,
        dense: SexpId,
    ) -> Result<bool, ValueError> {
        let info = self.foreign_alt(x)?.ok_or_else(|| {
            ValueError::InvalidAttribute("expected a foreign ALTREP vector".into())
        })?;
        if info.owned {
            return Ok(false);
        }
        let src = self.heap.get_mut(dense)?;
        if src.sexp_type() != info.ty {
            return Err(src.type_mismatch(info.ty.type_name(false)));
        }
        let mismatch = src.type_mismatch("dense vector");
        let mat = AltMaterialized::take_dense(&mut src.body).ok_or(mismatch)?;
        let dst = self.heap.get_mut(x)?;
        let mismatch = dst.type_mismatch("foreign ALTREP");
        let Body::Alt(alt) = &mut dst.body else {
            return Err(mismatch);
        };
        let AltKind::Foreign(foreign) = &mut alt.kind else {
            return Err(mismatch);
        };
        foreign.len = Some(mat.len());
        alt.materialized = Some(mat);
        self.heap.update_size(dense)?;
        self.heap.update_size(x)?;
        Ok(true)
    }

    /// Return the class and package names for a built-in ALTREP vector.
    pub fn builtin_alt_class(
        &self,
        x: SexpId,
    ) -> Result<Option<(&'static str, &'static str)>, ValueError> {
        let Body::Alt(alt) = &self.heap.get(x)?.body else {
            return Ok(None);
        };
        Ok(alt
            .kind
            .builtin_class()
            .map(|class| (class.class_name(), class.package_name())))
    }

    /// Return true if this session has allocated a foreign ALTREP vector.
    #[must_use]
    pub fn foreign_alt_seen(&self) -> bool {
        self.foreign_alt_seen
    }

    pub(crate) fn require_alt_payload(&self, x: SexpId) -> Result<(), ValueError> {
        if let Body::Alt(alt) = &self.heap.get(x)?.body {
            alt.require_payload(x)?;
        }
        Ok(())
    }

    pub(crate) fn register_alt_edges(&mut self, x: SexpId) -> Result<(), ValueError> {
        let Body::Alt(alt) = &mut self.heap.get_mut(x)?.body else {
            return Ok(());
        };
        let mut edges = Vec::new();
        if matches!(alt.kind, AltKind::Foreign(_) | AltKind::Str(_))
            || alt.wrapped_target().is_some()
        {
            alt.dropped_edges = Some(Rc::clone(&self.alt_drops));
            alt.for_each_edge(|edge| edges.push(edge));
        }
        for edge in edges {
            self.heap.inc_ref(edge);
        }
        Ok(())
    }

    fn alloc_alt(
        &mut self,
        kind: AltKind,
        materialized: Option<AltMaterialized>,
    ) -> Result<SexpId, ValueError> {
        if let Some(class) = kind.builtin_class() {
            self.intern(class.class_name())?;
            self.intern(class.package_name())?;
        }
        let id = self
            .heap
            .alloc(RObject::new(Body::Alt(Box::new(AltObj::new(
                kind,
                materialized,
            )))))?;
        self.register_alt_edges(id)?;
        Ok(id)
    }
}

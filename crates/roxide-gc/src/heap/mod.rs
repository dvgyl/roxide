//! Generational-slot mark-sweep GC arena with `ProtectStack` and saturating refcounts.

use std::collections::HashMap;
use std::fmt;

use crate::id::{MAX_GENERATION, SexpId};

mod ephemeron;
mod protect;

use ephemeron::Ephemeron;
pub use ephemeron::{Finalizer, FinalizerKind, SweepOutcome, WeakRefEntry};
pub use protect::{DEFAULT_PROTECT_LIMIT, ProtectIndex, ProtectMark};

/// Saturated copy-on-modify reference count ceiling (sticky once reached).
pub const REFCNT_MAX: u8 = 7;

/// Errors returned by [`Heap`] operations (never panics on stale handles or limits).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HeapError {
    /// Handle refers to a freed or mismatched-generation slot.
    Stale(SexpId),
    /// Requested allocation would exceed heap capacity or integer bounds.
    OutOfMemory {
        /// Requested size in bytes.
        requested: usize,
    },
    /// Allocation exceeds configured memory limit.
    LimitExceeded {
        /// Requested size in bytes.
        requested: usize,
    },
    /// Protect stack reached its configured depth limit (`R_PPSSIZE`).
    ProtectStackOverflow {
        /// Configured stack depth limit.
        limit: usize,
    },
    /// Attempted to unprotect more items than currently on the protect stack.
    ProtectStackUnderflow {
        /// Requested pop count.
        requested: usize,
        /// Available stack depth.
        available: usize,
    },
    /// `UNPROTECT_PTR` could not find the requested handle on the protect stack.
    ProtectPtrNotFound(SexpId),
    /// Attempted to mutably borrow the same slot twice via `get_pair_mut`.
    AliasedSlot(SexpId),
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stale(id) => write!(f, "stale SEXP handle ({:#x})", id.to_bits()),
            Self::OutOfMemory { requested } => {
                write!(f, "cannot allocate memory of size {requested} bytes")
            }
            Self::LimitExceeded { requested } => {
                write!(f, "memory limit exceeded requesting {requested} bytes")
            }
            Self::ProtectStackOverflow { .. } => {
                f.write_str("protect(): protection stack overflow")
            }
            Self::ProtectStackUnderflow {
                requested,
                available,
            } => write!(
                f,
                "unprotect stack imbalance: tried to pop {requested} with depth {available}"
            ),
            Self::ProtectPtrNotFound(_) => f.write_str("unprotect_ptr: pointer not found"),
            Self::AliasedSlot(id) => write!(f, "aliased mutable slot ({:#x})", id.to_bits()),
        }
    }
}

impl std::error::Error for HeapError {}

/// Collection scheduling policy.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum GcPolicy {
    /// Trigger collection when live bytes exceed `next_gc_bytes`.
    Adaptive {
        /// Minimum heap threshold in bytes before adaptive collection triggers.
        min_bytes: usize,
        /// Growth multiplier applied to surviving bytes after each sweep.
        growth: f32,
    },
    /// Collect at every safepoint (enabled by `ROXIDE_GC_TORTURE=1`).
    Torture,
}

impl Default for GcPolicy {
    fn default() -> Self {
        Self::Adaptive {
            min_bytes: 4 * 1024 * 1024,
            growth: 2.0,
        }
    }
}

/// Statistics returned from a completed GC cycle.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct GcStats {
    /// Objects surviving this collection.
    pub live_objects: usize,
    /// Objects freed during this sweep.
    pub freed_objects: usize,
    /// Estimated heap bytes remaining live.
    pub live_bytes: usize,
    /// Total collections completed on this heap.
    pub collections: u64,
}

/// Implemented by GC-managed objects to report outgoing [`SexpId`] edges.
pub trait Trace {
    /// Push all directly referenced [`SexpId`] handles into `tracer`.
    fn trace(&self, tracer: &mut Tracer<'_>);
}

/// Implemented by root providers (evaluator frames, globals, `CtxExt` tables).
pub trait RootSet {
    /// Push all root [`SexpId`] handles into `tracer`.
    fn trace_roots(&self, tracer: &mut Tracer<'_>);
}

/// Reports approximate heap footprint in bytes for adaptive GC scheduling.
pub trait HeapSize {
    /// Approximate heap bytes owned by this object.
    fn heap_bytes(&self) -> usize;
}

/// Worklist sink passed to [`Trace::trace`] and [`RootSet::trace_roots`].
pub struct Tracer<'a> {
    marked: &'a mut [bool],
    worklist: &'a mut Vec<u32>,
    generations: &'a [u32],
}

impl<'a> Tracer<'a> {
    /// Mark a single referenced handle and enqueue it if not yet visited.
    #[inline]
    pub fn edge(&mut self, id: SexpId) {
        let idx = id.index() as usize;
        if let Some(&slot_gen) = self.generations.get(idx)
            && slot_gen == id.generation()
            && !self.marked[idx]
        {
            self.marked[idx] = true;
            self.worklist.push(idx as u32);
        }
    }

    fn trace_slots<T: Trace>(&mut self, slots: &[Slot<T>]) {
        while let Some(idx) = self.worklist.pop() {
            if let Some(obj) = slots[idx as usize].value.as_ref() {
                obj.trace(self);
            }
        }
    }

    /// Mark a slice of referenced handles.
    #[inline]
    pub fn edges(&mut self, ids: &[SexpId]) {
        for &id in ids {
            self.edge(id);
        }
    }
}

struct Slot<T> {
    generation: u32,
    refcnt: u8,
    permanent: bool,
    bytes: usize,
    value: Option<T>,
}

impl<T> Slot<T> {
    fn retire_slot(&mut self) -> bool {
        self.value = None;
        self.bytes = 0;
        self.refcnt = 0;
        if self.generation == MAX_GENERATION {
            return false;
        }
        self.generation += 1;
        true
    }
}

/// Generational-slot mark-sweep GC heap.
pub struct Heap<T: Trace + HeapSize> {
    slots: Vec<Slot<T>>,
    /// Parallel copy of slot generations for fast marking without borrowing `slots`.
    generations: Vec<u32>,
    free_list: Vec<u32>,
    protect_stack: Vec<SexpId>,
    protect_limit: usize,
    precious: HashMap<SexpId, u32>,
    ephemerons: Vec<Ephemeron>,
    pending_finalizers: Vec<Finalizer>,
    policy: GcPolicy,
    live_bytes: usize,
    live_objects: usize,
    next_gc_bytes: usize,
    max_bytes: Option<usize>,
    collections: u64,
}

impl<T: Trace + HeapSize> Default for Heap<T> {
    fn default() -> Self {
        Self::new(GcPolicy::default())
    }
}

impl<T: Trace + HeapSize> Heap<T> {
    /// Create a new empty [`Heap`] with the given [`GcPolicy`].
    #[must_use]
    pub fn new(policy: GcPolicy) -> Self {
        let next_gc_bytes = match policy {
            GcPolicy::Adaptive { min_bytes, .. } => min_bytes,
            GcPolicy::Torture => 0,
        };
        Self {
            slots: Vec::new(),
            generations: Vec::new(),
            free_list: Vec::new(),
            protect_stack: Vec::new(),
            protect_limit: DEFAULT_PROTECT_LIMIT,
            precious: HashMap::new(),
            ephemerons: Vec::new(),
            pending_finalizers: Vec::new(),
            policy,
            live_bytes: 0,
            live_objects: 0,
            next_gc_bytes,
            max_bytes: None,
            collections: 0,
        }
    }

    /// Configure an optional hard memory ceiling in bytes.
    pub fn set_max_bytes(&mut self, max_bytes: Option<usize>) {
        self.max_bytes = max_bytes;
    }

    /// Return the active GC policy.
    #[must_use]
    pub fn policy(&self) -> GcPolicy {
        self.policy
    }

    /// Update the active GC policy.
    pub fn set_policy(&mut self, policy: GcPolicy) {
        self.policy = policy;
        if let GcPolicy::Adaptive { min_bytes, .. } = policy
            && self.next_gc_bytes < min_bytes
        {
            self.next_gc_bytes = min_bytes;
        }
    }

    /// Allocate a normal GC-managed object. Never triggers collection.
    pub fn alloc(&mut self, value: T) -> Result<SexpId, HeapError> {
        let bytes = value.heap_bytes();
        self.check_limit(bytes)?;
        if let Some(idx) = self.free_list.pop() {
            let slot = &mut self.slots[idx as usize];
            let generation = slot.generation;
            let id =
                SexpId::new(idx, generation).ok_or(HeapError::OutOfMemory { requested: bytes })?;
            slot.refcnt = 0;
            slot.permanent = false;
            slot.bytes = bytes;
            slot.value = Some(value);
            self.live_bytes = self.live_bytes.saturating_add(bytes);
            self.live_objects += 1;
            return Ok(id);
        }
        let idx = u32::try_from(self.slots.len())
            .map_err(|_| HeapError::OutOfMemory { requested: bytes })?;
        let generation = 1u32;
        let id = SexpId::new(idx, generation).ok_or(HeapError::OutOfMemory { requested: bytes })?;
        self.slots.push(Slot {
            generation,
            refcnt: 0,
            permanent: false,
            bytes,
            value: Some(value),
        });
        self.generations.push(generation);
        self.live_bytes = self.live_bytes.saturating_add(bytes);
        self.live_objects += 1;
        Ok(id)
    }

    /// Allocate a permanent generation-0 singleton at the next sequential slot index.
    ///
    /// Permanent objects are always marked live and always have saturated `refcnt`.
    pub fn alloc_permanent(&mut self, value: T) -> Result<SexpId, HeapError> {
        let bytes = value.heap_bytes();
        let idx = u32::try_from(self.slots.len())
            .map_err(|_| HeapError::OutOfMemory { requested: bytes })?;
        let id = SexpId::new(idx, 0).ok_or(HeapError::OutOfMemory { requested: bytes })?;
        self.slots.push(Slot {
            generation: 0,
            refcnt: REFCNT_MAX,
            permanent: true,
            bytes,
            value: Some(value),
        });
        self.generations.push(0);
        self.live_bytes = self.live_bytes.saturating_add(bytes);
        self.live_objects += 1;
        Ok(id)
    }

    /// Refresh the cached byte-size accounting of an existing object after in-place mutation.
    pub fn update_size(&mut self, id: SexpId) -> Result<(), HeapError> {
        let (old_bytes, new_bytes) = {
            let slot = self.slot_mut(id)?;
            let old_bytes = slot.bytes;
            let new_bytes = slot.value.as_ref().map_or(0, HeapSize::heap_bytes);
            slot.bytes = new_bytes;
            (old_bytes, new_bytes)
        };
        self.live_bytes = self
            .live_bytes
            .saturating_sub(old_bytes)
            .saturating_add(new_bytes);
        Ok(())
    }

    /// Borrow a live object immutably.
    pub fn get(&self, id: SexpId) -> Result<&T, HeapError> {
        let slot = self.slot(id)?;
        slot.value.as_ref().ok_or(HeapError::Stale(id))
    }

    /// Borrow a live object mutably.
    pub fn get_mut(&mut self, id: SexpId) -> Result<&mut T, HeapError> {
        let slot = self.slot_mut(id)?;
        slot.value.as_mut().ok_or(HeapError::Stale(id))
    }

    /// Borrow two distinct live slots simultaneously (`dst` mutable, `src` immutable).
    pub fn get_pair_mut(&mut self, dst: SexpId, src: SexpId) -> Result<(&mut T, &T), HeapError> {
        if dst.index() == src.index() {
            return Err(HeapError::AliasedSlot(dst));
        }
        let d_idx = dst.index() as usize;
        let s_idx = src.index() as usize;
        if d_idx >= self.slots.len() || self.slots[d_idx].generation != dst.generation() {
            return Err(HeapError::Stale(dst));
        }
        if s_idx >= self.slots.len() || self.slots[s_idx].generation != src.generation() {
            return Err(HeapError::Stale(src));
        }
        if d_idx < s_idx {
            let (left, right) = self.slots.split_at_mut(s_idx);
            let d = left[d_idx].value.as_mut().ok_or(HeapError::Stale(dst))?;
            let s = right[0].value.as_ref().ok_or(HeapError::Stale(src))?;
            Ok((d, s))
        } else {
            let (left, right) = self.slots.split_at_mut(d_idx);
            let s = left[s_idx].value.as_ref().ok_or(HeapError::Stale(src))?;
            let d = right[0].value.as_mut().ok_or(HeapError::Stale(dst))?;
            Ok((d, s))
        }
    }

    /// Return the current saturating reference count (`0..=REFCNT_MAX`).
    #[must_use]
    pub fn refcnt(&self, id: SexpId) -> u8 {
        self.slot(id).map_or(REFCNT_MAX, |s| s.refcnt)
    }

    /// Increment the reference count, saturating stickily at [`REFCNT_MAX`].
    pub fn inc_ref(&mut self, id: SexpId) {
        if let Ok(slot) = self.slot_mut(id)
            && !slot.permanent
            && slot.refcnt < REFCNT_MAX
        {
            slot.refcnt += 1;
        }
    }

    /// Decrement the reference count unless saturated or permanent.
    pub fn dec_ref(&mut self, id: SexpId) {
        if let Ok(slot) = self.slot_mut(id)
            && !slot.permanent
            && slot.refcnt > 0
            && slot.refcnt < REFCNT_MAX
        {
            slot.refcnt -= 1;
        }
    }

    /// Return true if more than one heap reference may observe this object (`refcnt > 1`).
    #[must_use]
    pub fn maybe_shared(&self, id: SexpId) -> bool {
        self.refcnt(id) > 1
    }

    /// Return true if at least one heap reference exists (`refcnt > 0`).
    #[must_use]
    pub fn maybe_referenced(&self, id: SexpId) -> bool {
        self.refcnt(id) > 0
    }

    /// Permanently saturate the reference count (`MARK_NOT_MUTABLE`).
    pub fn mark_immutable(&mut self, id: SexpId) {
        if let Ok(slot) = self.slot_mut(id) {
            slot.refcnt = REFCNT_MAX;
        }
    }

    /// Return whether a safepoint should trigger collection under the active [`GcPolicy`].
    #[must_use]
    pub fn wants_collection(&self) -> bool {
        match self.policy {
            GcPolicy::Torture => true,
            GcPolicy::Adaptive { .. } => self.live_bytes >= self.next_gc_bytes,
        }
    }

    /// Live byte estimate.
    #[must_use]
    pub fn live_bytes(&self) -> usize {
        self.live_bytes
    }

    /// Live object count.
    #[must_use]
    pub fn live_objects(&self) -> usize {
        self.live_objects
    }

    /// Phase 1 of collection: iteratively mark all objects reachable from permanent slots,
    /// the protect stack, the precious set, and `roots`.
    pub fn mark<'h>(&'h mut self, roots: &dyn RootSet) -> Marked<'h, T> {
        let mut marked = vec![false; self.slots.len()];
        let mut worklist = Vec::new();

        let mut tracer = Tracer {
            marked: &mut marked,
            worklist: &mut worklist,
            generations: &self.generations,
        };
        self.seed_roots(roots, &mut tracer);
        tracer.trace_slots(&self.slots);

        Marked {
            heap: self,
            marked,
            ephemerons_resolved: false,
            cleared_weakrefs: Vec::new(),
        }
    }

    fn seed_roots(&self, roots: &dyn RootSet, tracer: &mut Tracer<'_>) {
        for (idx, slot) in self.slots.iter().enumerate() {
            if slot.permanent && slot.value.is_some() {
                tracer.marked[idx] = true;
                tracer.worklist.push(idx as u32);
            }
        }
        tracer.edges(&self.protect_stack);
        for &id in self.precious.keys() {
            tracer.edge(id);
        }
        for fin in &self.pending_finalizers {
            tracer.edge(fin.target);
            if let Some(fun) = fin.kind.closure() {
                tracer.edge(fun);
            }
        }
        roots.trace_roots(tracer);
    }

    fn check_limit(&self, additional: usize) -> Result<(), HeapError> {
        if let Some(limit) = self.max_bytes
            && self.live_bytes.saturating_add(additional) > limit
        {
            return Err(HeapError::LimitExceeded {
                requested: additional,
            });
        }
        Ok(())
    }

    fn slot(&self, id: SexpId) -> Result<&Slot<T>, HeapError> {
        let idx = id.index() as usize;
        let slot = self.slots.get(idx).ok_or(HeapError::Stale(id))?;
        if slot.generation != id.generation() || slot.value.is_none() {
            return Err(HeapError::Stale(id));
        }
        Ok(slot)
    }

    fn slot_mut(&mut self, id: SexpId) -> Result<&mut Slot<T>, HeapError> {
        let idx = id.index() as usize;
        let slot = self.slots.get_mut(idx).ok_or(HeapError::Stale(id))?;
        if slot.generation != id.generation() || slot.value.is_none() {
            return Err(HeapError::Stale(id));
        }
        Ok(slot)
    }
}

/// Phase-2 handle returned by [`Heap::mark`], allowing weak tables to inspect reachability
/// before [`Marked::sweep`] frees unmarked objects.
pub struct Marked<'h, T: Trace + HeapSize> {
    heap: &'h mut Heap<T>,
    marked: Vec<bool>,
    ephemerons_resolved: bool,
    cleared_weakrefs: Vec<SexpId>,
}

impl<T: Trace + HeapSize> Marked<'_, T> {
    /// Return true if `id` is live and was marked reachable during Phase 1.
    #[must_use]
    pub fn is_marked(&self, id: SexpId) -> bool {
        let idx = id.index() as usize;
        self.heap
            .generations
            .get(idx)
            .is_some_and(|&g| g == id.generation())
            && self.marked.get(idx).copied().unwrap_or(false)
    }

    /// Transitively mark `id` and every object reachable from it before [`Marked::sweep`].
    ///
    /// Used for ephemeron value marking and keeping objects queued for GC finalization alive
    /// until their finalizer callback runs.
    pub fn mark_transitive(&mut self, id: SexpId) {
        let mut worklist = Vec::new();
        let mut tracer = Tracer {
            marked: &mut self.marked,
            worklist: &mut worklist,
            generations: &self.heap.generations,
        };
        tracer.edge(id);
        tracer.trace_slots(&self.heap.slots);
    }

    /// Sweep all unmarked non-permanent slots, bump generations, and return [`GcStats`].
    pub fn sweep(self) -> GcStats {
        self.sweep_outcome().stats
    }

    /// Sweep all unmarked non-permanent slots and return [`SweepOutcome`] (stats + cleared weakrefs).
    ///
    /// Newly queued finalizers remain rooted on the heap until drained via
    /// [`Heap::drain_pending_finalizers`], preserving single-consumer finalizer execution.
    pub fn sweep_outcome(mut self) -> SweepOutcome {
        self.resolve_ephemerons();
        let cleared_weakrefs = std::mem::take(&mut self.cleared_weakrefs);
        let mut freed = 0usize;
        let mut live_objects = 0usize;
        let mut live_bytes = 0usize;

        for (idx, slot) in self.heap.slots.iter_mut().enumerate() {
            if slot.value.is_none() {
                continue;
            }
            if slot.permanent || self.marked[idx] {
                live_objects += 1;
                live_bytes = live_bytes.saturating_add(slot.bytes);
            } else {
                if slot.retire_slot() {
                    self.heap.free_list.push(idx as u32);
                }
                self.heap.generations[idx] = slot.generation;
                freed += 1;
            }
        }

        self.heap.live_objects = live_objects;
        self.heap.live_bytes = live_bytes;
        self.heap.collections += 1;

        if let GcPolicy::Adaptive { min_bytes, growth } = self.heap.policy {
            let scaled = (live_bytes as f64 * f64::from(growth)) as usize;
            self.heap.next_gc_bytes = scaled.max(min_bytes);
        }

        SweepOutcome {
            stats: GcStats {
                live_objects,
                freed_objects: freed,
                live_bytes,
                collections: self.heap.collections,
            },
            cleared_weakrefs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Node {
        edges: Vec<SexpId>,
    }

    impl Trace for Node {
        fn trace(&self, tracer: &mut Tracer<'_>) {
            tracer.edges(&self.edges);
        }
    }

    impl HeapSize for Node {
        fn heap_bytes(&self) -> usize {
            64 + self.edges.len() * 8
        }
    }

    struct EmptyRoots;
    impl RootSet for EmptyRoots {
        fn trace_roots(&self, _tracer: &mut Tracer<'_>) {}
    }

    #[test]
    fn collects_cycles_and_detects_stale_handles() {
        let mut heap = Heap::new(GcPolicy::Torture);
        let perm = heap.alloc_permanent(Node { edges: vec![] }).unwrap();
        let a = heap.alloc(Node { edges: vec![] }).unwrap();
        let b = heap.alloc(Node { edges: vec![a] }).unwrap();
        heap.get_mut(a).unwrap().edges.push(b);

        let mark = heap.protect_mark();
        heap.protect(a);
        let stats1 = heap.mark(&EmptyRoots).sweep();
        assert_eq!(stats1.freed_objects, 0);
        assert_eq!(stats1.live_objects, 3);

        heap.unprotect_to(mark);
        let stats2 = heap.mark(&EmptyRoots).sweep();
        assert_eq!(stats2.freed_objects, 2);
        assert_eq!(stats2.live_objects, 1);
        assert!(heap.get(perm).is_ok());
        assert_eq!(heap.get(a).err(), Some(HeapError::Stale(a)));
        assert_eq!(heap.get(b).err(), Some(HeapError::Stale(b)));
    }

    #[test]
    fn refcount_saturates_stickily() {
        let mut heap = Heap::new(GcPolicy::default());
        let id = heap.alloc(Node { edges: vec![] }).unwrap();
        assert!(!heap.maybe_referenced(id));
        heap.inc_ref(id);
        assert!(heap.maybe_referenced(id));
        assert!(!heap.maybe_shared(id));
        heap.inc_ref(id);
        assert!(heap.maybe_shared(id));
        heap.dec_ref(id);
        assert!(!heap.maybe_shared(id));
        for _ in 0..20 {
            heap.inc_ref(id);
        }
        assert_eq!(heap.refcnt(id), REFCNT_MAX);
        heap.dec_ref(id);
        assert_eq!(heap.refcnt(id), REFCNT_MAX);
    }

    #[test]
    fn protect_limit_and_unprotect_ptr() {
        let mut heap = Heap::new(GcPolicy::default());
        heap.set_protect_limit(2);
        let a = heap.alloc(Node { edges: vec![] }).unwrap();
        let b = heap.alloc(Node { edges: vec![] }).unwrap();
        let c = heap.alloc(Node { edges: vec![] }).unwrap();
        heap.try_protect(a).unwrap();
        heap.try_protect(b).unwrap();
        assert_eq!(
            heap.try_protect(c).unwrap_err(),
            HeapError::ProtectStackOverflow { limit: 2 }
        );
        heap.unprotect_ptr(a).unwrap();
        assert_eq!(heap.protect_depth(), 1);
        assert_eq!(
            heap.unprotect_ptr(a).unwrap_err(),
            HeapError::ProtectPtrNotFound(a)
        );
        heap.unprotect_ptr(b).unwrap();
        assert_eq!(heap.protect_depth(), 0);
    }

    #[test]
    fn ephemeron_cyclic_finalizer_and_weakref() {
        let mut heap = Heap::new(GcPolicy::Torture);
        let target = heap.alloc(Node { edges: vec![] }).unwrap();
        let fin_closure = heap
            .alloc(Node {
                edges: vec![target],
            })
            .unwrap();
        heap.register_finalizer(target, FinalizerKind::RClosure(fin_closure), false);

        let mark = heap.protect_mark();
        heap.protect(target);
        let out1 = heap.mark(&EmptyRoots).sweep_outcome();
        assert_eq!(out1.stats.freed_objects, 0);
        assert!(!heap.has_pending_finalizers());
        assert!(heap.get(fin_closure).is_ok());

        heap.unprotect_to(mark);
        let out2 = heap.mark(&EmptyRoots).sweep_outcome();
        assert_eq!(out2.stats.freed_objects, 0);
        assert!(heap.has_pending_finalizers());
        assert!(heap.get(target).is_ok());
        assert!(heap.get(fin_closure).is_ok());

        let drained = heap.drain_pending_finalizers();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].target, target);
        let out3 = heap.mark(&EmptyRoots).sweep_outcome();
        assert!(!heap.has_pending_finalizers());
        assert_eq!(out3.stats.freed_objects, 2);
        assert_eq!(heap.get(target).err(), Some(HeapError::Stale(target)));

        let t2 = heap.alloc(Node { edges: vec![] }).unwrap();
        heap.register_finalizer(
            t2,
            FinalizerKind::Native {
                fun: 0x1000,
                dll: 7,
            },
            true,
        );
        heap.retain_finalizers(|k| !matches!(k, FinalizerKind::Native { dll: 7, .. }));
        assert!(heap.take_exit_finalizers().is_empty());
    }
    #[test]
    fn preserves_weakref_flags_and_finalizer_order() {
        let mut heap = Heap::new(GcPolicy::Torture);
        let key = heap.alloc(Node { edges: vec![] }).unwrap();
        let weakref = heap.alloc(Node { edges: vec![] }).unwrap();
        let weak_callback = FinalizerKind::Native { fun: 1, dll: 0 };
        let standalone = FinalizerKind::Native { fun: 2, dll: 0 };
        let mut entry = WeakRefEntry {
            weakref,
            key,
            value: key,
            finalizer: None,
            onexit: true,
        };

        heap.register_weakref(entry);
        assert_eq!(heap.remove_weakref(weakref), Some(entry));

        entry.finalizer = Some(weak_callback);
        heap.register_weakref(entry);
        heap.retain_finalizers(|kind| kind != weak_callback);
        assert_eq!(
            heap.remove_weakref(weakref),
            Some(WeakRefEntry {
                finalizer: None,
                ..entry
            })
        );

        heap.register_weakref(entry);
        heap.register_finalizer(key, standalone, true);
        assert_eq!(
            heap.take_exit_finalizers()
                .into_iter()
                .map(|fin| fin.kind)
                .collect::<Vec<_>>(),
            [standalone, weak_callback]
        );

        heap.register_weakref(entry);
        heap.register_finalizer(key, standalone, false);
        assert_eq!(
            heap.mark(&EmptyRoots).sweep_outcome().cleared_weakrefs,
            [weakref]
        );
        assert_eq!(
            heap.drain_pending_finalizers()
                .into_iter()
                .map(|fin| fin.kind)
                .collect::<Vec<_>>(),
            [standalone, weak_callback]
        );
    }
}

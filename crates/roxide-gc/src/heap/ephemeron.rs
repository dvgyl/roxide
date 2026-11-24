use super::{GcStats, Heap, HeapSize, Marked, SexpId, Trace};

/// Callback kind attached to a GC finalizer or weak reference (ADR-0019).
///
/// Native C finalizer pointers are stored as `(fun, dll)` addresses/ids so `roxide-gc` keeps
/// `#![forbid(unsafe_code)]` while allowing `dyn.unload` to purge finalizers targeting an
/// unloading DLL before `dlclose`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalizerKind {
    /// R closure finalizer (`reg.finalizer`, `R_RegisterFinalizerEx`, `R_MakeWeakRef`).
    RClosure(SexpId),
    /// Native C finalizer function pointer (`R_RegisterCFinalizerEx`, `R_MakeWeakRefC`).
    Native {
        /// Function pointer address (`R_CFinalizer_t`).
        fun: usize,
        /// Owning DLL handle identifier (`0` if built-in / non-unloadable).
        dll: u32,
    },
}

impl FinalizerKind {
    /// Return the callback handle for an R closure.
    #[must_use]
    pub fn closure(&self) -> Option<SexpId> {
        match *self {
            Self::RClosure(id) => Some(id),
            Self::Native { .. } => None,
        }
    }
}

/// Result returned by [`Marked::sweep_outcome`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SweepOutcome {
    /// Collection statistics.
    pub stats: GcStats,
    /// `WEAKREFSXP` handles whose keys became unreachable during this cycle and must be cleared.
    pub cleared_weakrefs: Vec<SexpId>,
}

/// Registered ephemeron finalizer entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Finalizer {
    /// Finalized object handle (`ENVSXP` or `EXTPTRSXP`).
    pub target: SexpId,
    /// Callback to invoke after `target` becomes unreachable.
    pub kind: FinalizerKind,
    /// Whether this finalizer should also run at session exit.
    pub onexit: bool,
}

/// Registered ephemeron weak-reference entry (`WEAKREFSXP`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WeakRefEntry {
    /// Handle of the `WEAKREFSXP` object itself.
    pub weakref: SexpId,
    /// Weakly held key (`ENVSXP` or `EXTPTRSXP`).
    pub key: SexpId,
    /// Value kept alive while `key` is reachable.
    pub value: SexpId,
    /// Optional finalizer triggered when `key` becomes unreachable.
    pub finalizer: Option<FinalizerKind>,
    /// Whether `finalizer` should also run at session exit.
    pub onexit: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Ephemeron {
    key: SexpId,
    value: Option<SexpId>,
    weakref: Option<SexpId>,
    callback: Option<(FinalizerKind, bool)>,
    // remove_weakref returns the original flag even after callback removal.
    onexit_without_callback: bool,
}

impl Ephemeron {
    fn finalizer(self) -> Option<Finalizer> {
        let (kind, onexit) = self.callback?;
        Some(Finalizer {
            target: self.key,
            kind,
            onexit,
        })
    }
}

impl<T: Trace + HeapSize> Heap<T> {
    /// Register a finalizer for the weakly held `target`.
    pub fn register_finalizer(&mut self, target: SexpId, kind: FinalizerKind, onexit: bool) {
        let pos = self
            .ephemerons
            .partition_point(|entry| entry.weakref.is_none());
        self.ephemerons.insert(
            pos,
            Ephemeron {
                key: target,
                value: None,
                weakref: None,
                callback: Some((kind, onexit)),
                onexit_without_callback: false,
            },
        );
    }

    /// Register a `WEAKREFSXP` ephemeron.
    pub fn register_weakref(&mut self, entry: WeakRefEntry) {
        self.ephemerons.push(Ephemeron {
            key: entry.key,
            value: Some(entry.value),
            weakref: Some(entry.weakref),
            callback: entry.finalizer.map(|kind| (kind, entry.onexit)),
            onexit_without_callback: entry.finalizer.is_none() && entry.onexit,
        });
    }

    /// Remove and return the first registered entry for `weakref`.
    pub fn remove_weakref(&mut self, weakref: SexpId) -> Option<WeakRefEntry> {
        let pos = self
            .ephemerons
            .iter()
            .position(|entry| entry.weakref == Some(weakref))?;
        let entry = self.ephemerons.remove(pos);
        Some(WeakRefEntry {
            weakref,
            key: entry.key,
            value: entry.value?,
            finalizer: entry.callback.map(|(kind, _)| kind),
            onexit: entry
                .callback
                .map_or(entry.onexit_without_callback, |(_, onexit)| onexit),
        })
    }

    /// Retain only callbacks that satisfy `keep` in the registry and pending queue.
    pub fn retain_finalizers(&mut self, keep: impl Fn(FinalizerKind) -> bool) {
        self.ephemerons.retain(|entry| {
            entry.weakref.is_some() || entry.callback.is_some_and(|(kind, _)| keep(kind))
        });
        self.pending_finalizers.retain(|fin| keep(fin.kind));
        for entry in self
            .ephemerons
            .iter_mut()
            .filter(|entry| entry.weakref.is_some())
        {
            if let Some((kind, onexit)) = entry.callback
                && !keep(kind)
            {
                entry.callback = None;
                entry.onexit_without_callback = onexit;
            }
        }
    }

    /// Return true if the pending queue contains finalizers.
    #[must_use]
    pub fn has_pending_finalizers(&self) -> bool {
        !self.pending_finalizers.is_empty()
    }

    /// Drain all pending finalizers in queue order.
    pub fn drain_pending_finalizers(&mut self) -> Vec<Finalizer> {
        std::mem::take(&mut self.pending_finalizers)
    }

    /// Take pending finalizers and registered finalizers that run at session exit.
    pub fn take_exit_finalizers(&mut self) -> Vec<Finalizer> {
        let mut out = self.drain_pending_finalizers();
        out.extend(
            self.ephemerons
                .extract_if(.., |entry| entry.callback.is_some_and(|(_, onexit)| onexit))
                .filter_map(Ephemeron::finalizer),
        );
        out
    }
}

impl<T: Trace + HeapSize> Marked<'_, T> {
    /// Resolve live ephemeron edges and queue callbacks for dead keys.
    ///
    /// Call this method at most once per collection to change reachability.
    /// Subsequent calls have no effect.
    pub fn resolve_ephemerons(&mut self) {
        if self.ephemerons_resolved {
            return;
        }
        self.ephemerons_resolved = true;
        self.ephemeron_fixpoint();
        let dead = self.partition_dead();
        self.resurrect(dead);
    }

    fn ephemeron_fixpoint(&mut self) {
        loop {
            let mut changed = false;
            for i in 0..self.heap.ephemerons.len() {
                let entry = self.heap.ephemerons[i];
                if self.is_marked(entry.key) {
                    for edge in entry
                        .value
                        .into_iter()
                        .chain(entry.callback.and_then(|(kind, _)| kind.closure()))
                    {
                        if !self.is_marked(edge) {
                            self.mark_transitive(edge);
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn partition_dead(&mut self) -> Vec<Finalizer> {
        let mut entries = std::mem::take(&mut self.heap.ephemerons);
        let mut finalizers = Vec::new();
        let mut cleared = Vec::new();
        for entry in entries.extract_if(.., |entry| !self.is_marked(entry.key)) {
            cleared.extend(entry.weakref);
            finalizers.extend(entry.finalizer());
        }
        self.heap.ephemerons = entries;
        self.cleared_weakrefs = cleared;
        finalizers
    }

    fn resurrect(&mut self, finalizers: Vec<Finalizer>) {
        for fin in &finalizers {
            self.mark_transitive(fin.target);
            if let Some(closure) = fin.kind.closure() {
                self.mark_transitive(closure);
            }
        }
        self.heap.pending_finalizers.extend(finalizers);
    }
}

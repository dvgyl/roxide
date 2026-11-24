use super::{Heap, HeapError, HeapSize, SexpId, Trace};

/// Default maximum protect-stack depth matching GNU R 4.6.2 (`R_PPSSIZE = 50000`, ADR-0019).
pub const DEFAULT_PROTECT_LIMIT: usize = 50_000;

/// Index into the protect stack returned by [`Heap::protect`] for [`Heap::reprotect`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProtectIndex(usize);

impl ProtectIndex {
    /// Construct a [`ProtectIndex`] from a raw stack offset (for C `PROTECT_INDEX` interop).
    #[inline]
    #[must_use]
    pub const fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    /// Return the raw protect-stack offset (for C `PROTECT_INDEX` interop).
    #[inline]
    #[must_use]
    pub const fn as_raw(self) -> usize {
        self.0
    }
}

/// Saved depth of the protect stack for scoped restoration via [`Heap::unprotect_to`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProtectMark(usize);

impl<T: Trace + HeapSize> Heap<T> {
    /// Push a handle onto the protect stack.
    pub fn protect(&mut self, id: SexpId) -> ProtectIndex {
        let idx = self.protect_stack.len();
        self.protect_stack.push(id);
        ProtectIndex(idx)
    }

    /// Push a handle onto the protect stack, enforcing [`Heap::protect_limit`] (`Rf_protect`).
    pub fn try_protect(&mut self, id: SexpId) -> Result<ProtectIndex, HeapError> {
        if self.protect_stack.len() >= self.protect_limit {
            return Err(HeapError::ProtectStackOverflow {
                limit: self.protect_limit,
            });
        }
        Ok(self.protect(id))
    }

    /// Configured protect-stack depth ceiling (`R_PPSSIZE`).
    #[must_use]
    pub fn protect_limit(&self) -> usize {
        self.protect_limit
    }

    /// Update the configured protect-stack depth ceiling.
    pub fn set_protect_limit(&mut self, limit: usize) {
        self.protect_limit = limit;
    }

    /// Replace an existing protect stack entry (`REPROTECT`).
    pub fn reprotect(&mut self, at: ProtectIndex, id: SexpId) {
        if let Some(entry) = self.protect_stack.get_mut(at.0) {
            *entry = id;
        }
    }

    /// Pop `n` entries from the protect stack.
    pub fn unprotect(&mut self, n: usize) -> Result<(), HeapError> {
        let available = self.protect_stack.len();
        if n > available {
            return Err(HeapError::ProtectStackUnderflow {
                requested: n,
                available,
            });
        }
        self.protect_stack.truncate(available - n);
        Ok(())
    }

    /// Remove the topmost occurrence of `id` from the protect stack (`UNPROTECT_PTR`).
    pub fn unprotect_ptr(&mut self, id: SexpId) -> Result<(), HeapError> {
        self.unprotect_ptr_from(id, 0)
    }

    /// Remove the topmost occurrence of `id` at or above `min_depth` on the protect stack.
    ///
    /// Prevents nested C frames from unprotecting roots owned by enclosing trampoline frames.
    pub fn unprotect_ptr_from(&mut self, id: SexpId, min_depth: usize) -> Result<(), HeapError> {
        let start = min_depth.min(self.protect_stack.len());
        let Some(rel) = self.protect_stack[start..]
            .iter()
            .rposition(|&entry| entry == id)
        else {
            return Err(HeapError::ProtectPtrNotFound(id));
        };
        self.protect_stack.remove(start + rel);
        Ok(())
    }

    /// Record the current protect stack depth.
    #[must_use]
    pub fn protect_mark(&self) -> ProtectMark {
        ProtectMark(self.protect_stack.len())
    }

    /// Restore the protect stack depth to a previous [`ProtectMark`].
    pub fn unprotect_to(&mut self, mark: ProtectMark) {
        if mark.0 <= self.protect_stack.len() {
            self.protect_stack.truncate(mark.0);
        }
    }

    /// Current protect stack depth.
    #[must_use]
    pub fn protect_depth(&self) -> usize {
        self.protect_stack.len()
    }

    /// Add one reference in the precious multiset (`R_PreserveObject`).
    pub fn preserve(&mut self, id: SexpId) {
        let count = self.precious.entry(id).or_insert(0);
        *count = count.saturating_add(1);
    }

    /// Remove one reference from the precious multiset (`R_ReleaseObject`).
    pub fn release(&mut self, id: SexpId) {
        if let Some(count) = self.precious.get_mut(&id) {
            if *count <= 1 {
                self.precious.remove(&id);
            } else {
                *count -= 1;
            }
        }
    }
}

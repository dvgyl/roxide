//! GC-traced graphics display list (M3 W1-D).

use roxide_gc::{SexpId, Tracer};

/// Recorded graphics display list entries for one device.
#[derive(Default)]
pub struct DisplayList {
    entries: Vec<SexpId>,
    enabled: bool,
}

impl DisplayList {
    /// Append a recorded graphics call `SEXP` when recording is enabled.
    pub fn push(&mut self, entry: SexpId) {
        if self.enabled {
            self.entries.push(entry);
        }
    }

    /// Clear all recorded entries (e.g. on `new_page()`).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Enable or disable display-list recording.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Borrow all recorded `SexpId` entries.
    #[must_use]
    pub fn entries(&self) -> &[SexpId] {
        &self.entries
    }

    /// Trace all recorded `SexpId` roots during GC.
    pub fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.edges(&self.entries);
    }
}

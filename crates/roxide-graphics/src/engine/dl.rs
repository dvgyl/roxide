//! GC-traced graphics display list (M3 W1-D).

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

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

    /// Return a snapshot of the recorded calls as an R list.
    ///
    /// Protect the returned list before a garbage collection can occur.
    pub fn snapshot(&self, ctx: &mut Ctx) -> EvalResult {
        Self::snapshot_entries(ctx, &self.entries)
    }

    /// Copy call IDs into an R list after releasing a borrow of the device table.
    pub fn snapshot_entries(ctx: &mut Ctx, entries: &[SexpId]) -> EvalResult {
        ctx.scoped(|ctx| {
            for &entry in entries {
                ctx.protect(entry);
            }
            let snapshot = ctx.session_mut().alloc_list(entries.len())?;
            ctx.protect(snapshot);
            for (i, &entry) in entries.iter().enumerate() {
                ctx.session_mut().list_set(snapshot, i, entry)?;
            }
            Ok(snapshot)
        })
    }

    /// Evaluate the recorded calls in order in `env`. Stop at the first error.
    pub fn replay(ctx: &mut Ctx, snapshot: SexpId, env: SexpId) -> EvalResult<()> {
        ctx.scoped(|ctx| {
            ctx.protect(snapshot);
            ctx.protect(env);
            if ctx.session().type_of(snapshot)? != SexpType::Vec {
                return Err(ctx.stop(SEXP_NIL, "invalid graphics display list"));
            }
            for index in 0..ctx.session().xlength(snapshot)? {
                let entry = ctx.session().list_get(snapshot, index)?;
                ctx.eval(entry, env)?;
            }
            Ok(())
        })
    }

    /// Trace all recorded `SexpId` roots during GC.
    pub fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.edges(&self.entries);
    }
}

impl Trace for DisplayList {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        DisplayList::trace(self, tracer);
    }
}

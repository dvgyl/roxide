//! Thread-local C API context and borrow checks.

use std::cell::RefCell;
use std::ffi::c_int;
use std::thread::{self, ThreadId};

use roxide_eval::{Ctx, Unwind};
use roxide_gc::ProtectMark;
use roxide_value::{RHeap, RSession};

/// Current C API access mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// An API call has borrowed the evaluator.
    Full,
    /// An API call has borrowed the session.
    Session,
    /// An API call has borrowed the heap for reads.
    Heap,
    /// Foreign code can enter the C API.
    Released,
}

#[derive(Clone, Copy)]
struct Published {
    ctx: *mut Ctx,
    owner: ThreadId,
    mode: Mode,
}

struct Frame {
    mark: ProtectMark,
    floor: usize,
    pending: Option<Unwind>,
}

#[derive(Default)]
struct ApiCx {
    published: Option<Published>,
    frames: Vec<Frame>,
}

thread_local! {
    static API: RefCell<ApiCx> = RefCell::new(ApiCx::default());
}

fn abort(message: &str) -> ! {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "{message}");
    std::process::abort()
}

struct Restore(Option<Published>);

impl Drop for Restore {
    fn drop(&mut self) {
        API.with_borrow_mut(|api| api.published = self.0);
    }
}

/// Publish a context on the current R thread for the duration of `f`.
///
/// Reentry must use the same context. This restores the previous borrow
/// mode when `f` returns or unwinds in Rust.
pub fn with_ctx_entered<R>(ctx: &mut Ctx, f: impl FnOnce() -> R) -> R {
    let owner = thread::current().id();
    let ctx = std::ptr::from_mut(ctx);
    let previous = API.with_borrow_mut(|api| {
        if api.published.is_some_and(|active| active.ctx != ctx) {
            abort("Roxide C API cannot replace an active R context");
        }
        api.published.replace(Published {
            ctx,
            owner,
            mode: Mode::Released,
        })
    });
    let _restore = Restore(previous);
    f()
}

/// Run foreign dispatch with a fresh context pointer for reentry.
///
/// The caller must sit above the foreign trampoline so that control
/// returns here after each foreign jump.
pub(crate) fn with_released<R>(ctx: &mut Ctx, f: impl FnOnce() -> R) -> R {
    with_ctx_entered(ctx, f)
}

fn borrow(mode: Mode) -> (*mut Ctx, Restore) {
    API.with_borrow_mut(|api| {
        let Some(published) = api.published else {
            abort("R API called from a non-R thread (no active context)");
        };
        if published.owner != thread::current().id() {
            abort("R API called from a non-R thread");
        }
        if published.mode != Mode::Released {
            abort("Roxide C API attempted to borrow an already borrowed context");
        }
        api.published = Some(Published { mode, ..published });
        (published.ctx, Restore(Some(published)))
    })
}

/// Borrow the evaluator for one C API call.
pub fn with_full<R>(f: impl FnOnce(&mut Ctx) -> R) -> R {
    let (ctx, _restore) = borrow(Mode::Full);
    // SAFETY: The active publication owns the context for this scope.
    // The borrow guard rejects another borrow until this one ends.
    f(unsafe { &mut *ctx })
}

/// Borrow the session for one C API call.
pub fn with_session<R>(f: impl FnOnce(&mut RSession) -> R) -> R {
    let (ctx, _restore) = borrow(Mode::Session);
    // SAFETY: The borrow guard gives this call exclusive access.
    f(unsafe { (&mut *ctx).session_mut() })
}

/// Borrow the heap for one read-only C API call on the R thread.
pub fn with_heap<R>(f: impl FnOnce(&RHeap) -> R) -> R {
    let (ctx, _restore) = borrow(Mode::Heap);
    // SAFETY: The published context is live and cannot be mutably borrowed.
    f(unsafe { &(&*ctx).session().heap })
}

pub(crate) const MAX_DEPTH: usize = 256;

pub(crate) struct ActiveFrame;

impl ActiveFrame {
    pub(crate) fn push(heap: &RHeap) -> Option<Self> {
        API.with_borrow_mut(|api| {
            if api.frames.len() >= MAX_DEPTH {
                return None;
            }
            api.frames.push(Frame {
                mark: heap.protect_mark(),
                floor: heap.protect_depth(),
                pending: None,
            });
            Some(Self)
        })
    }

    pub(crate) fn saved_mark(&self) -> (ProtectMark, usize) {
        with_frame(|frame| (frame.mark, frame.floor))
    }

    pub(crate) fn take_unwind(&self) -> Option<Unwind> {
        with_frame(|frame| frame.pending.take())
    }
}

impl Drop for ActiveFrame {
    fn drop(&mut self) {
        API.with_borrow_mut(|api| api.frames.pop());
    }
}

fn with_frame<R>(f: impl FnOnce(&mut Frame) -> R) -> R {
    API.with_borrow_mut(|api| match api.frames.last_mut() {
        Some(frame) => f(frame),
        None => abort("Roxide C API called without an active trampoline"),
    })
}

pub(crate) fn depth() -> usize {
    API.with_borrow(|api| api.frames.len())
}

pub(crate) fn protect_floor() -> usize {
    with_frame(|frame| frame.floor)
}

pub(crate) fn pend(unwind: Unwind) -> c_int {
    with_frame(|frame| frame.pending = Some(unwind));
    // MAX_DEPTH is small enough for a C int.
    (depth() - 1) as c_int
}

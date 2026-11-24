//! Thread-local C API context, reentry ceilings, and worker-thread heap read synchronization.

use std::cell::RefCell;
use std::ffi::c_int;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

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

fn mode_rank(mode: Mode) -> u8 {
    match mode {
        Mode::Heap => 0,
        Mode::Session => 1,
        Mode::Full => 2,
        Mode::Released => 3,
    }
}

#[derive(Clone, Copy)]
struct Published {
    ctx: *mut Ctx,
    mode: Mode,
    ceiling: Mode,
    via_trampoline: bool,
}

struct Frame {
    mark: ProtectMark,
    entry_depth: usize,
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

/// Process-wide published read-only heap pointer for worker threads (ADR-0019).
///
/// Protocol on `HEAP_STATE`:
/// - `0`: unpublished or mutably borrowed by the R main thread (workers must abort).
/// - `1`: released for read-only access, 0 active worker readers.
/// - `1 + k`: released for read-only access, `k` active worker readers.
///
/// `ACTIVE_HEAP` is only written by the R main thread while `HEAP_STATE == 0`,
/// published via `Release` store `0 -> 1`, and reclaimed via `Acquire` CAS `1 -> 0`.
static ACTIVE_HEAP: AtomicPtr<RHeap> = AtomicPtr::new(null_mut());
static HEAP_STATE: AtomicUsize = AtomicUsize::new(0);
/// Process-wide owner lock enforcing one active C API session at a time (ADR-0019).
/// Acquired only at outermost publication; reentrant calls on the same R thread reuse it.
static SESSION_LOCK: Mutex<()> = Mutex::new(());

fn is_readable(published: Option<Published>) -> bool {
    matches!(
        published,
        Some(Published {
            mode: Mode::Released | Mode::Heap,
            ..
        })
    )
}

fn sync_worker_heap(old: Option<Published>, new: Option<Published>) {
    match (is_readable(old), is_readable(new)) {
        (false, true) => {
            if let Some(pub_new) = new {
                // SAFETY: `pub_new.ctx` is exclusively owned by the R thread while
                // `HEAP_STATE == 0` and stays pinned for the duration of the publication.
                let heap_ptr =
                    unsafe { std::ptr::from_ref(&(&*pub_new.ctx).session().heap) }.cast_mut();
                ACTIVE_HEAP.store(heap_ptr, Ordering::Relaxed);
                HEAP_STATE.store(1, Ordering::Release);
            }
        }
        (true, false) => {
            while HEAP_STATE
                .compare_exchange_weak(1, 0, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                std::hint::spin_loop();
            }
            if new.is_none() {
                ACTIVE_HEAP.store(null_mut(), Ordering::Relaxed);
            }
        }
        _ => {}
    }
}

fn abort(message: &str) -> ! {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "{message}");
    std::process::abort()
}

struct Restore {
    previous: Option<Published>,
    _session_guard: Option<MutexGuard<'static, ()>>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        API.with_borrow_mut(|api| {
            let old = api.published;
            api.published = self.previous;
            sync_worker_heap(old, self.previous);
        });
    }
}

struct WorkerReadGuard;

impl Drop for WorkerReadGuard {
    fn drop(&mut self) {
        HEAP_STATE.fetch_sub(1, Ordering::Release);
    }
}

fn publish_inner<R>(
    ctx: &mut Ctx,
    ceiling: Mode,
    via_trampoline: bool,
    f: impl FnOnce() -> R,
) -> R {
    let ctx_ptr = std::ptr::from_mut(ctx);
    let already_active = API.with_borrow(|api| api.published.is_some());
    let session_guard = (!already_active).then(|| {
        SESSION_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    });
    let previous = API.with_borrow_mut(|api| {
        if api.published.is_some_and(|active| active.ctx != ctx_ptr) {
            abort("Roxide C API cannot replace an active R context");
        }
        let effective_ceiling = match api.published {
            Some(active) if mode_rank(active.ceiling) < mode_rank(ceiling) => active.ceiling,
            _ => ceiling,
        };
        let next = Some(Published {
            ctx: ctx_ptr,
            mode: Mode::Released,
            ceiling: effective_ceiling,
            via_trampoline,
        });
        let prev = api.published;
        api.published = next;
        sync_worker_heap(prev, next);
        prev
    });
    let _restore = Restore {
        previous,
        _session_guard: session_guard,
    };
    f()
}

/// Publish a context on the current R thread for non-jumping read/session calls during `f`.
///
/// Reentry must use the same context. Because no C trampoline sits directly beneath `f`,
/// any error jump through `boundary` aborts cleanly rather than unwinding past Rust locals.
pub fn with_ctx_entered<R>(ctx: &mut Ctx, f: impl FnOnce() -> R) -> R {
    publish_inner(ctx, Mode::Full, false, f)
}

/// Run foreign dispatch with a fresh context pointer for reentry inside an active trampoline.
///
/// The caller must sit above the foreign trampoline so that control returns here after
/// each foreign jump.
pub(crate) fn with_released<R>(ctx: &mut Ctx, ceiling: Mode, f: impl FnOnce() -> R) -> R {
    publish_inner(ctx, ceiling, true, f)
}

fn borrow(mode: Mode) -> (*mut Ctx, Restore) {
    API.with_borrow_mut(|api| {
        let Some(published) = api.published else {
            abort("R API called from a non-R thread (no active context)");
        };
        if published.mode != Mode::Released {
            abort("Roxide C API attempted to borrow an already borrowed context");
        }
        if mode_rank(mode) > mode_rank(published.ceiling) {
            abort("Roxide C API borrow exceeds active reentry ceiling");
        }
        let next = Some(Published { mode, ..published });
        api.published = next;
        sync_worker_heap(Some(published), next);
        (
            published.ctx,
            Restore {
                previous: Some(published),
                _session_guard: None,
            },
        )
    })
}

/// Borrow the evaluator for one C API call on the R main thread.
pub fn with_full<R>(f: impl FnOnce(&mut Ctx) -> R) -> R {
    let (ctx, _restore) = borrow(Mode::Full);
    // SAFETY: The active publication owns the context for this scope and
    // `sync_worker_heap` drained all worker readers before setting `HEAP_STATE = 0`.
    f(unsafe { &mut *ctx })
}

/// Borrow the session for one C API call on the R main thread.
pub fn with_session<R>(f: impl FnOnce(&mut RSession) -> R) -> R {
    let (ctx, _restore) = borrow(Mode::Session);
    // SAFETY: The borrow guard gives this call exclusive access and
    // `sync_worker_heap` drained all worker readers before setting `HEAP_STATE = 0`.
    f(unsafe { (&mut *ctx).session_mut() })
}

/// Borrow the heap for one read-only C API call on the R main thread or a worker thread
/// while the main thread is in `Mode::Released`.
pub fn with_heap<R>(f: impl FnOnce(&RHeap) -> R) -> R {
    let on_r_thread = API.with_borrow(|api| api.published.is_some());
    if on_r_thread {
        let (ctx, _restore) = borrow(Mode::Heap);
        // SAFETY: The published context is live and cannot be mutably borrowed while in `Heap`.
        return f(unsafe { &(&*ctx).session().heap });
    }
    if HEAP_STATE
        .try_update(Ordering::Acquire, Ordering::Relaxed, |state| {
            (state >= 1).then_some(state + 1)
        })
        .is_err()
    {
        abort("R API called from a non-R thread while R heap is not released");
    }
    let _guard = WorkerReadGuard;
    let heap = ACTIVE_HEAP.load(Ordering::Acquire);
    if heap.is_null() {
        abort("R API called from a non-R thread (no active heap)");
    }
    // SAFETY: `HEAP_STATE >= 2` pins the R main thread out of mutable borrows until
    // `WorkerReadGuard` drops, and `&RHeap` performs no interior mutation.
    f(unsafe { &*heap })
}

pub(crate) fn can_jump() -> bool {
    API.with_borrow(|api| {
        api.published
            .is_some_and(|published| published.via_trampoline)
            && !api.frames.is_empty()
    })
}

pub(crate) const MAX_DEPTH: usize = 256;

pub(crate) struct ActiveFrame;

impl ActiveFrame {
    pub(crate) fn push(heap: &RHeap, inherit_floor: bool) -> Option<Self> {
        API.with_borrow_mut(|api| {
            if api.frames.len() >= MAX_DEPTH {
                return None;
            }
            let entry_depth = heap.protect_depth();
            let floor = if inherit_floor {
                api.frames.last().map_or(entry_depth, |parent| parent.floor)
            } else {
                entry_depth
            };
            api.frames.push(Frame {
                mark: heap.protect_mark(),
                entry_depth,
                floor,
                pending: None,
            });
            Some(Self)
        })
    }

    pub(crate) fn saved_mark(&self) -> (ProtectMark, usize) {
        with_frame(|frame| (frame.mark, frame.entry_depth))
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
    if !can_jump() {
        abort("Roxide C API jump attempted without an active foreign trampoline");
    }
    with_frame(|frame| frame.pending = Some(unwind));
    // MAX_DEPTH is small enough for a C int.
    (depth() - 1) as c_int
}

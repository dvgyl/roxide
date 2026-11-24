//! Evaluator context, call arguments, and evaluation frames.

mod altrep;
mod gc;
mod host;
mod registration;
mod script;

use std::any::TypeId;
use std::collections::{HashMap, HashSet};

use roxide_diag::SourceMap;
use roxide_gc::SexpId;
use roxide_value::{PrimId, RSession, SEXP_NIL, Sym};

use crate::api::{NamespaceLoader, NativeHooks, OptionsCache, PrimDef, S4Hooks};
use crate::namespace::{DeferredS3, NsRecord};

pub(crate) use gc::ExtEntry;
pub(crate) use host::HostStack;

/// Monotonically increasing frame identifier for targeted non-local transfers.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FrameId(pub u64);

/// Evaluated or unevaluated call arguments passed to a [`crate::PrimFn`].
#[derive(Clone, Default, Debug)]
pub struct Args {
    values: Vec<SexpId>,
    names: Vec<Option<Sym>>,
}

impl Args {
    /// Create an [`Args`] list from paired `(Option<Sym>, SexpId)` entries.
    #[must_use]
    pub fn from_pairs(pairs: Vec<(Option<Sym>, SexpId)>) -> Self {
        let mut values = Vec::with_capacity(pairs.len());
        let mut names = Vec::with_capacity(pairs.len());
        for (n, v) in pairs {
            names.push(n);
            values.push(v);
        }
        Self { values, names }
    }

    /// Number of arguments.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// True if zero arguments were supplied.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Positional argument `i` (`SEXP_NIL` if out of range).
    #[inline]
    #[must_use]
    pub fn get(&self, i: usize) -> SexpId {
        self.values.get(i).copied().unwrap_or(SEXP_NIL)
    }

    /// Argument tag at index `i`.
    #[inline]
    #[must_use]
    pub fn name(&self, i: usize) -> Option<Sym> {
        self.names.get(i).copied().flatten()
    }

    /// Borrow all argument values.
    #[inline]
    #[must_use]
    pub fn values(&self) -> &[SexpId] {
        &self.values
    }

    /// Look up the first argument matching exact tag name `target`.
    #[must_use]
    pub fn named(&self, session: &RSession, target: &str) -> Option<SexpId> {
        for (i, tag) in self.names.iter().enumerate() {
            if let Some(sym) = tag
                && session.sym_name(*sym).ok() == Some(target)
            {
                return Some(self.values[i]);
            }
        }
        None
    }

    /// Iterate over `(Option<Sym>, SexpId)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (Option<Sym>, SexpId)> + '_ {
        self.names.iter().copied().zip(self.values.iter().copied())
    }
}

/// Active evaluation frame metadata.
#[derive(Clone)]
pub(crate) struct Frame {
    pub(crate) id: FrameId,
    pub(crate) call: SexpId,
    pub(crate) cloenv: SexpId,
    pub(crate) on_exit: Vec<SexpId>,
    pub(crate) fun: SexpId,
    pub(crate) caller: SexpId,
    pub(crate) actuals: Args,
    pub(crate) supplied: Vec<bool>,
    pub(crate) dispatch: Option<crate::dispatch::MethodFrame>,
}

/// Core Roxide execution context owning the [`RSession`], [`Host`](roxide_host::Host), frames, and primitive table.
pub struct Ctx {
    pub(crate) session: RSession,
    box_muller_spare: Option<BoxMullerSpare>,
    pub(crate) host: HostStack,
    pub(crate) sources: SourceMap,
    pub(crate) opts: OptionsCache,
    pub(crate) prims: Vec<&'static PrimDef>,
    pub(crate) internals: HashMap<&'static str, PrimId>,
    pub(crate) registered_prims: HashSet<(SexpId, &'static str)>,
    pub(crate) overridable_prims: HashSet<(SexpId, &'static str)>,
    pub(crate) overridable_internals: HashSet<&'static str>,
    pub(crate) namespaces: Vec<NsRecord>,
    pub(crate) pending_s3: Vec<DeferredS3>,
    pub(crate) namespace_loader: Option<&'static dyn NamespaceLoader>,
    pub(crate) autoloads: SexpId,
    pub(crate) frames: Vec<Frame>,
    pub(crate) loops: Vec<(FrameId, SexpId)>,
    pub(crate) conditions: crate::conditions::Conditions,
    pub(crate) show_error_messages: bool,
    pub(crate) options: Vec<(Sym, SexpId)>,
    pub(crate) next_frame_id: u64,
    pub(crate) expr_depth: usize,
    pub(crate) overflow_headroom: usize,
    pub(crate) stack_origin: Option<usize>,
    pub(crate) stack_budget: usize,
    pub(crate) calls: Vec<(SexpId, SexpId)>,
    pub(crate) replacement_call: Option<SexpId>,
    pub(crate) eval_env: SexpId,
    pub(crate) visible: bool,
    pub(crate) pending_warnings: Vec<(Option<Vec<String>>, String)>,
    pub(crate) vm_stack: crate::vm::VmStack,
    pub(crate) extensions: HashMap<TypeId, ExtEntry>,
    pub(crate) running_finalizers: bool,
    pub(crate) s4_hooks: Option<&'static dyn S4Hooks>,
    pub(crate) native_hooks: Option<&'static dyn NativeHooks>,
    altrep_aware: HashSet<&'static str>,
    pub(crate) jit_level: u8,
    pub(crate) interactive: bool,
}

struct BoxMullerSpare {
    seed: SexpId,
    words: Vec<i32>,
    value: f64,
}

impl Ctx {
    /// Return a spare normal if the seed binding and its contents still match.
    pub fn box_muller_spare(&mut self, seed: SexpId, words: &[i32]) -> Option<f64> {
        if let Some(spare) = &self.box_muller_spare
            && spare.seed == seed
            && spare.words == words
        {
            return Some(spare.value);
        }
        self.box_muller_spare = None;
        None
    }

    /// Replace the spare normal after saving the random stream.
    pub fn save_box_muller_spare(&mut self, seed: SexpId, words: Vec<i32>, spare: Option<f64>) {
        self.box_muller_spare = spare.map(|value| BoxMullerSpare { seed, words, value });
    }

    /// Discard the spare normal when the seed binding is absent or invalid.
    pub fn clear_box_muller_spare(&mut self) {
        self.box_muller_spare = None;
    }

    /// Allocate the next evaluation frame identifier.
    pub(crate) fn next_frame(&mut self) -> FrameId {
        let id = FrameId(self.next_frame_id);
        self.next_frame_id += 1;
        id
    }

    /// Borrow the underlying [`RSession`].
    #[inline]
    #[must_use]
    pub fn session(&self) -> &RSession {
        &self.session
    }

    /// Mutably borrow the underlying [`RSession`].
    #[inline]
    #[must_use]
    pub fn session_mut(&mut self) -> &mut RSession {
        &mut self.session
    }

    /// Borrow cached options.
    #[inline]
    #[must_use]
    pub fn opts(&self) -> &OptionsCache {
        &self.opts
    }

    /// Mutably borrow cached options.
    #[inline]
    #[must_use]
    pub fn opts_mut(&mut self) -> &mut OptionsCache {
        &mut self.opts
    }

    /// Set visibility flag for the current evaluation result.
    #[inline]
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Current visibility flag.
    #[inline]
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    /// Return whether this session is running interactively.
    #[inline]
    #[must_use]
    pub const fn is_interactive(&self) -> bool {
        self.interactive
    }

    /// Configure whether this session is interactive.
    #[inline]
    pub fn set_interactive(&mut self, interactive: bool) {
        self.interactive = interactive;
    }

    /// Return whether this replacement call can update its target in place.
    pub fn can_overwrite(&self, call: &crate::Call) -> bool {
        self.replacement_call == Some(call.expr)
    }

    /// Set the evaluator stack budget in bytes.
    ///
    /// The caller must leave space outside this budget for entry frames and unwinding.
    pub fn set_stack_budget(&mut self, bytes: usize) {
        self.stack_budget = bytes;
    }

    /// Return the configured evaluator stack budget in bytes.
    #[must_use]
    pub fn stack_budget(&self) -> usize {
        self.stack_budget
    }

    /// Return the identifier of the innermost active closure frame, if any.
    #[must_use]
    pub fn current_frame_id(&self) -> Option<FrameId> {
        self.frames.last().map(|frame| frame.id)
    }

    /// Return the calling environment of the current closure frame.
    pub fn closure_caller(&self, env: SexpId) -> SexpId {
        self.frame(env).map_or(env, |frame| frame.caller)
    }

    /// Inspect `(call, cloenv, fun, caller)` of an active closure frame by [`FrameId`].
    #[must_use]
    pub fn frame_info(&self, id: FrameId) -> Option<(SexpId, SexpId, SexpId, SexpId)> {
        self.frames
            .iter()
            .rfind(|frame| frame.id == id)
            .map(|frame| (frame.call, frame.cloenv, frame.fun, frame.caller))
    }

    /// Return the parent active closure frame [`FrameId`] of `id`, if one exists.
    #[must_use]
    pub fn parent_frame_id(&self, id: FrameId) -> Option<FrameId> {
        let idx = self.frames.iter().rposition(|frame| frame.id == id)?;
        if idx == 0 {
            return None;
        }
        let caller = self.frames[idx].caller;
        self.frames[..idx]
            .iter()
            .rposition(|frame| frame.cloenv == caller)
            .map(|p| self.frames[p].id)
            .or_else(|| Some(self.frames[idx - 1].id))
    }

    /// Install the native C-API & dynamic-library hooks (owned by `roxide-capi`, ADRs 0017–0020).
    pub fn set_native_hooks(&mut self, hooks: &'static dyn NativeHooks) {
        self.native_hooks = Some(hooks);
    }

    /// Borrow the installed [`NativeHooks`], if `roxide-capi` has been registered.
    #[must_use]
    pub fn native_hooks(&self) -> Option<&'static dyn NativeHooks> {
        self.native_hooks
    }

    /// Current bytecode JIT compilation level (`0..=3`, default `3`; ADR-0020).
    #[must_use]
    pub fn jit_level(&self) -> u8 {
        self.jit_level
    }

    /// Update the bytecode JIT compilation level (`0..=3`), returning the previous level.
    pub fn set_jit_level(&mut self, level: u8) -> u8 {
        let prev = self.jit_level;
        self.jit_level = level.min(3);
        prev
    }
}

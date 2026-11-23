//! [`Ctx`] evaluator context, argument bundle [`Args`], frame stack, and GC root wiring.

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};

use roxide_diag::{SourceMap, SrcFileId, format as diag_format};
use roxide_gc::{GcPolicy, RootSet, SexpId, Tracer};
use roxide_host::Host;
use roxide_syntax::{ParseStep, Parser};
use roxide_value::{
    BASE_ENV, DeparseOpts, GLOBAL_ENV, KeepSource, PrimId, RSession, SEXP_NIL, Sym, ValueApi,
    deparse, lower,
};

use crate::api::{
    ArgEval, CtxExt, EvalResult, InternalError, Module, OptionsCache, Package, PrimDef, PrimKind,
    S4Hooks, Unwind,
};
use crate::builtins;

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
    pub(crate) dispatch: Option<crate::dispatch::MethodFrame>,
}

pub(crate) struct ExtEntry {
    boxed: Box<dyn Any>,
    trace_fn: fn(&dyn Any, &mut Tracer<'_>),
}

/// Registered non-base package namespace and optional attached search-path environment.
#[derive(Clone, Copy)]
pub(crate) struct RegisteredPackage {
    pub(crate) name: &'static str,
    pub(crate) namespace: SexpId,
    pub(crate) attached: Option<SexpId>,
}

/// Core Roxide execution context owning the [`RSession`], [`Host`], frames, and primitive table.
pub struct Ctx {
    pub(crate) session: RSession,
    pub(crate) host: Box<dyn Host>,
    pub(crate) sources: SourceMap,
    pub(crate) opts: OptionsCache,
    pub(crate) prims: Vec<&'static PrimDef>,
    pub(crate) internals: HashMap<&'static str, PrimId>,
    pub(crate) registered_prims: HashSet<(SexpId, &'static str)>,
    pub(crate) overridable_prims: HashSet<(SexpId, &'static str)>,
    pub(crate) overridable_internals: HashSet<&'static str>,
    pub(crate) packages: Vec<RegisteredPackage>,
    pub(crate) frames: Vec<Frame>,
    pub(crate) loops: Vec<(FrameId, SexpId)>,
    pub(crate) conditions: crate::conditions::Conditions,
    pub(crate) options: Vec<(Sym, SexpId)>,
    pub(crate) utils: SexpId,
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
    pub(crate) extensions: HashMap<TypeId, ExtEntry>,
    pub(crate) s4_hooks: Option<&'static dyn S4Hooks>,
    pub(crate) interactive: bool,
}

struct CtxRoots<'a> {
    frames: &'a [Frame],
    calls: &'a [(SexpId, SexpId)],
    loops: &'a [(FrameId, SexpId)],
    eval_env: SexpId,
    conditions: &'a crate::conditions::Conditions,
    options: &'a [(Sym, SexpId)],
    utils: SexpId,
    packages: &'a [RegisteredPackage],
    extensions: &'a HashMap<TypeId, ExtEntry>,
}

impl RootSet for CtxRoots<'_> {
    fn trace_roots(&self, tracer: &mut Tracer<'_>) {
        for &(call, env) in self.calls {
            tracer.edge(call);
            tracer.edge(env);
        }
        for frame in self.frames {
            tracer.edge(frame.call);
            tracer.edge(frame.cloenv);
            tracer.edge(frame.fun);
            tracer.edge(frame.caller);
            tracer.edges(frame.actuals.values());
            if let Some(dispatch) = &frame.dispatch {
                dispatch.trace(tracer);
            }
            tracer.edges(&frame.on_exit);
        }
        for &(_, env) in self.loops {
            tracer.edge(env);
        }
        tracer.edge(self.eval_env);
        self.conditions.trace(tracer);
        tracer.edge(self.utils);
        for pkg in self.packages {
            tracer.edge(pkg.namespace);
            if let Some(attached) = pkg.attached {
                tracer.edge(attached);
            }
        }
        for &(_, value) in self.options {
            tracer.edge(value);
        }
        for entry in self.extensions.values() {
            (entry.trace_fn)(entry.boxed.as_ref(), tracer);
        }
    }
}

impl Ctx {
    /// Construct a new [`Ctx`] with the given [`Host`] and [`GcPolicy`], registering core M0 primitives.
    pub fn new(host: Box<dyn Host>, policy: GcPolicy) -> Result<Self, InternalError> {
        let mut session = RSession::new(policy)?;
        let utils = session.alloc_env(BASE_ENV)?;
        let max_expressions = host.limits().max_expressions;
        let opts = OptionsCache {
            expressions: max_expressions,
            ..OptionsCache::default()
        };
        let mut ctx = Self {
            session,
            host,
            sources: SourceMap::new(),
            opts,
            prims: Vec::new(),
            internals: HashMap::new(),
            registered_prims: HashSet::new(),
            overridable_prims: HashSet::new(),
            overridable_internals: HashSet::new(),
            packages: Vec::new(),
            frames: Vec::new(),
            loops: Vec::new(),
            conditions: Default::default(),
            options: Vec::new(),
            utils,
            next_frame_id: 1,
            expr_depth: 0,
            overflow_headroom: 0,
            stack_origin: None,
            stack_budget: 512 * 1024,
            calls: Vec::new(),
            replacement_call: None,
            eval_env: GLOBAL_ENV,
            visible: true,
            pending_warnings: Vec::new(),
            extensions: HashMap::new(),
            s4_hooks: None,
            interactive: false,
        };
        let global = ctx.session.intern(".GlobalEnv")?;
        ctx.session.env_define(BASE_ENV, global, GLOBAL_ENV)?;
        ctx.register_module_in(&builtins::CORE_MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::control::MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::language::MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::environment::MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::fallback::MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::conditions::MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::dispatch::MODULE, BASE_ENV, true)?;
        Ok(ctx)
    }

    pub(crate) fn next_frame(&mut self) -> FrameId {
        let id = FrameId(self.next_frame_id);
        self.next_frame_id += 1;
        id
    }

    /// Register a [`Module`]'s primitives and `.Internal` entries in `BASE_ENV`.
    ///
    /// Rejects duplicate non-overridable primitive or `.Internal` registrations (ADR-0011).
    pub fn register_module(&mut self, module: &Module) -> Result<(), InternalError> {
        self.register_module_in(module, BASE_ENV, false)
    }

    /// Register a non-base R [`Package`] (`stats`, `utils`, `tools`, ...) (ADR-0011).
    ///
    /// Creates `namespace:<name>` (parent `BASE_ENV`), registers all package modules and preludes
    /// into that namespace, and when `pkg.attach` is true attaches `package:<name>` onto the
    /// search path immediately below `.GlobalEnv`.
    pub fn register_package(&mut self, pkg: &Package) -> Result<(), InternalError> {
        if pkg.name == "base" || self.packages.iter().any(|p| p.name == pkg.name) {
            return Err(InternalError {
                message: format!("package '{}' is already registered", pkg.name),
            });
        }
        let ns = self.session.alloc_env(BASE_ENV)?;
        let mark = self.session.heap.protect_mark();
        self.session.heap.protect(ns);
        let result = (|| -> Result<(), InternalError> {
            for module in pkg.modules {
                self.register_module_in(module, ns, false)?;
            }
            let attached = if pkg.attach {
                let parent = self.session.env_enclos(GLOBAL_ENV)?;
                let pkg_env = self.session.alloc_env(parent)?;
                self.session.heap.protect(pkg_env);
                let name_sym = self.session.intern("name")?;
                let label = self.session.scalar_str(&format!("package:{}", pkg.name))?;
                self.session.set_attrib(pkg_env, name_sym, label)?;
                for (sym, binding) in self.session.env_entries(ns)? {
                    let sym_name = self.session.sym_name(sym)?;
                    if !sym_name.starts_with('.') {
                        self.session.env_define_binding(pkg_env, sym, binding)?;
                    }
                }
                self.session.env_set_enclos(GLOBAL_ENV, pkg_env)?;
                Some(pkg_env)
            } else {
                None
            };
            self.packages.push(RegisteredPackage {
                name: pkg.name,
                namespace: ns,
                attached,
            });
            Ok(())
        })();
        self.session.heap.unprotect_to(mark);
        result
    }

    fn register_module_in(
        &mut self,
        module: &Module,
        target_env: SexpId,
        overridable: bool,
    ) -> Result<(), InternalError> {
        for prim in module.prims {
            let id = PrimId(u16::try_from(self.prims.len()).map_err(|_| InternalError {
                message: String::from("primitive table is full"),
            })?);
            match prim.kind {
                PrimKind::Primitive => {
                    let key = (target_env, prim.name);
                    if self.registered_prims.contains(&key) && !self.overridable_prims.remove(&key)
                    {
                        return Err(InternalError {
                            message: format!("duplicate primitive registration '{}'", prim.name),
                        });
                    }
                    self.prims.push(prim);
                    self.registered_prims.insert(key);
                    if overridable {
                        self.overridable_prims.insert(key);
                    }
                    let sym = self.session.intern(prim.name)?;
                    let special = matches!(prim.args, ArgEval::Unevaluated | ArgEval::EvalDollar);
                    let pval = self.session.alloc_prim(id, special)?;
                    self.session.env_define(target_env, sym, pval)?;
                }
                PrimKind::Internal => {
                    if self.internals.contains_key(prim.name)
                        && !self.overridable_internals.remove(prim.name)
                    {
                        return Err(InternalError {
                            message: format!("duplicate .Internal registration '{}'", prim.name),
                        });
                    }
                    self.prims.push(prim);
                    if overridable {
                        self.overridable_internals.insert(prim.name);
                    }
                    self.internals.insert(prim.name, id);
                }
            }
        }
        if !module.prelude.is_empty() {
            self.run_script("<prelude>", module.prelude, target_env, false)
                .map_err(|_| InternalError {
                    message: String::from("error evaluating module prelude"),
                })?;
        }
        Ok(())
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

    /// Mutably borrow the underlying [`Host`].
    #[inline]
    #[must_use]
    pub fn host(&mut self) -> &mut dyn Host {
        self.host.as_mut()
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

    /// Emit a diagnostic message (signaling `simpleMessage` when handlers are installed).
    pub fn message(&mut self, call: SexpId, text: impl Into<String>) -> EvalResult<()> {
        self.emit_condition("message", call, text.into(), false)
    }

    /// Emit a warning that participates in condition handlers and restarts.
    pub fn warn_now(&mut self, call: SexpId, msg: impl Into<String>) -> EvalResult<()> {
        self.emit_condition("warning", call, msg.into(), false)
    }

    /// Print a value using Roxide's core default vector printer.
    pub fn print_value_default(&mut self, val: SexpId, digits: usize) -> EvalResult<()> {
        builtins::print_value(self, val, digits)
    }

    /// Return the default `dput` / `deparse` line output for a value.
    pub fn deparse_lines(&self, val: SexpId, opts: DeparseOpts) -> EvalResult<Vec<String>> {
        Ok(deparse(&self.session, val, opts)?)
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

    /// Install the stateless S4 dispatch hook.
    pub fn set_s4_hooks(&mut self, hooks: &'static dyn S4Hooks) {
        self.s4_hooks = Some(hooks);
    }

    /// Protect a [`SexpId`] on the heap's protect stack.
    pub fn protect(&mut self, id: SexpId) {
        self.session.heap.protect(id);
    }

    /// Execute `f` inside a protect-mark scope that restores the protect stack on every exit path.
    pub fn scoped<R>(&mut self, f: impl FnOnce(&mut Self) -> EvalResult<R>) -> EvalResult<R> {
        let mark = self.session.heap.protect_mark();
        let res = f(self);
        self.session.heap.unprotect_to(mark);
        res
    }

    /// Access or lazily initialize per-module GC-traced extension state (ADR-0007).
    pub fn ext<T: CtxExt + Default>(&mut self) -> &mut T {
        let tid = TypeId::of::<T>();
        let entry = self.extensions.entry(tid).or_insert_with(|| ExtEntry {
            boxed: Box::<T>::default(),
            trace_fn: |any, tracer| {
                if let Some(val) = any.downcast_ref::<T>() {
                    val.trace(tracer);
                }
            },
        });
        entry
            .boxed
            .downcast_mut::<T>()
            .unwrap_or_else(|| unreachable!("TypeId key guarantees exact type match"))
    }

    /// Explicit GC safepoint (ADR-0002): runs collection only if policy requests it.
    pub fn safepoint(&mut self) {
        if self.session.heap.wants_collection() {
            let roots = CtxRoots {
                frames: &self.frames,
                calls: &self.calls,
                loops: &self.loops,
                eval_env: self.eval_env,
                conditions: &self.conditions,
                options: &self.options,
                utils: self.utils,
                packages: &self.packages,
                extensions: &self.extensions,
            };
            let _ = self.session.collect(&roots);
        }
    }

    /// Record a deferred warning (not a safepoint; flushed at expression/error boundary).
    pub fn warn(&mut self, call: SexpId, msg: impl Into<String>) {
        let call_lines = if call == SEXP_NIL {
            None
        } else {
            deparse(&self.session, call, DeparseOpts::default()).ok()
        };
        self.pending_warnings.push((call_lines, msg.into()));
    }

    /// Flush accumulated deferred warnings to `stderr`.
    pub fn flush_warnings(&mut self) {
        if self.pending_warnings.is_empty() {
            return;
        }
        let total = self.pending_warnings.len();
        let text = diag_format::warnings(&self.pending_warnings, total);
        self.pending_warnings.clear();
        let _ = self.host.write_err(text.as_bytes());
    }

    /// Signal an R error and return the resulting unwind.
    pub fn stop(&mut self, call: SexpId, msg: impl Into<String>) -> Unwind {
        self.error_condition(call, msg.into())
    }

    /// Parse and execute a complete R script string in `env`.
    ///
    /// When `auto_print` is true, visible top-level expression results are printed.
    pub fn run_script(
        &mut self,
        name: &str,
        src: &str,
        env: SexpId,
        auto_print: bool,
    ) -> EvalResult<()> {
        let fid: SrcFileId = self.sources.add_file(name, src);
        let mut parser = Parser::new(src, fid);
        loop {
            match parser.parse_next() {
                ParseStep::Eof => {
                    self.flush_warnings();
                    return Ok(());
                }
                ParseStep::Incomplete => {
                    return Err(self.stop(SEXP_NIL, "unexpected end of input"));
                }
                ParseStep::Error(err) => {
                    let call = parser.error_call().map_or(Ok(SEXP_NIL), |node| {
                        lower(parser.ast(), node, &mut self.session, KeepSource::No)
                    });
                    let call = self.condition_result(SEXP_NIL, call.map_err(Into::into))?;
                    return Err(self.stop(call, err.message));
                }
                ParseStep::Expr(node) => {
                    let expr = lower(parser.ast(), node, &mut self.session, KeepSource::No);
                    let expr = self.condition_result(SEXP_NIL, expr.map_err(Into::into))?;
                    let val = self.scoped(|ctx| {
                        ctx.protect(expr);
                        ctx.visible = true;
                        let v = ctx.eval(expr, env)?;
                        if auto_print && ctx.visible {
                            ctx.protect(v);
                            builtins::print_value(ctx, v, ctx.opts.digits)?;
                        }
                        Ok(v)
                    })?;
                    let _ = val;
                    self.flush_warnings();
                }
            }
        }
    }

    /// Convenience entrypoint for running top-level Rscript code in `.GlobalEnv`.
    pub fn run_global(&mut self, name: &str, src: &str) -> EvalResult<()> {
        self.run_script(name, src, GLOBAL_ENV, true)
    }
}

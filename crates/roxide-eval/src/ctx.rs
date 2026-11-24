//! [`Ctx`] evaluator context, argument bundle [`Args`], frame stack, and GC root wiring.

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use roxide_diag::{SourceMap, SrcFileId, format as diag_format};
use roxide_gc::{GcPolicy, RootSet, SexpId, Trace, Tracer};
use roxide_host::{
    CommandSpec, CpuTimes, DirEntry, FileMeta, Host, HostError, HostErrorKind, HostFile,
    HostResult, Limits, LocaleInfo, OpenMode, ProcessOutput, StdStream, SysInfo, WallTime,
};
use roxide_syntax::{ParseStep, Parser};
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, DeparseOpts, EMPTY_ENV, GLOBAL_ENV, KeepSource, PrimId, RSession,
    SEXP_NIL, SexpType, Sortedness, Sym, ValueApi, deparse, lower,
};

use crate::api::{
    AltOp, AltReply, ArgEval, Call, CtxExt, EvalResult, InternalError, Module, NamespaceLoader,
    NativeHooks, OptionsCache, Package, PrimDef, PrimKind, S4Hooks, ScriptOpts, Unwind,
    altrep_hook_missing,
};
use crate::builtins;
use crate::namespace::{DeferredS3, NsRecord};

fn altrep_reply_error() -> Unwind {
    Unwind::Internal(InternalError {
        message: "unexpected foreign ALTREP hook reply".into(),
    })
}

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

pub(crate) struct ExtEntry {
    boxed: Box<dyn Any>,
    trace_fn: fn(&dyn Any, &mut Tracer<'_>),
}

/// Core Roxide execution context owning the [`RSession`], [`Host`], frames, and primitive table.
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

type StreamSink = Box<dyn FnMut(&mut Ctx, &[u8]) -> EvalResult<()>>;

struct StreamCapture {
    id: u64,
    capture_out: bool,
    capture_err: bool,
    split: bool,
    persistent: bool,
    exceeded: bool,
    received: usize,
    bytes: Vec<u8>,
    sink: Option<StreamSink>,
    flushing: bool,
}

pub(crate) struct HostStack {
    base: Box<dyn Host>,
    captures: Vec<StreamCapture>,
    next_capture_id: u64,
}

impl HostStack {
    pub(crate) fn write_out(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.write(StdStream::Stdout, bytes)
    }

    pub(crate) fn write_err(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.write(StdStream::Stderr, bytes)
    }

    pub(crate) fn limits(&self) -> &Limits {
        self.base.limits()
    }

    pub(crate) fn poll_interrupt(&mut self) -> bool {
        self.base.poll_interrupt()
    }

    fn write(&mut self, stream: StdStream, bytes: &[u8]) -> HostResult<()> {
        let limit = self.base.limits().max_vector_bytes;
        for capture in self.captures.iter_mut().rev() {
            let selected = match stream {
                StdStream::Stdout => capture.capture_out,
                StdStream::Stderr => capture.capture_err,
                StdStream::Stdin => false,
            };
            if !selected || capture.flushing {
                continue;
            }
            if capture.exceeded || bytes.len() > limit.saturating_sub(capture.received) {
                capture.exceeded = true;
                return Err(HostError::new(
                    HostErrorKind::Other,
                    "output capture exceeds the vector size limit",
                ));
            }
            capture.received += bytes.len();
            capture.bytes.extend_from_slice(bytes);
            if !capture.split {
                return Ok(());
            }
        }
        match stream {
            StdStream::Stdout => self.base.write_out(bytes),
            StdStream::Stderr => self.base.write_err(bytes),
            StdStream::Stdin => Ok(()),
        }
    }
}

impl Host for HostStack {
    fn write_out(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.write_out(bytes)
    }
    fn write_err(&mut self, bytes: &[u8]) -> HostResult<()> {
        self.write_err(bytes)
    }
    fn flush(&mut self, stream: StdStream) -> HostResult<()> {
        self.base.flush(stream)
    }
    fn read_line(&mut self, buf: &mut Vec<u8>) -> HostResult<usize> {
        self.base.read_line(buf)
    }
    fn is_tty(&self, stream: StdStream) -> bool {
        self.base.is_tty(stream)
    }
    fn open(&mut self, path: &Path, mode: OpenMode) -> HostResult<Box<dyn HostFile>> {
        self.base.open(path, mode)
    }
    fn metadata(&self, path: &Path, follow: bool) -> HostResult<FileMeta> {
        self.base.metadata(path, follow)
    }
    fn read_dir(&self, path: &Path) -> HostResult<Vec<DirEntry>> {
        self.base.read_dir(path)
    }
    fn create_dir(&mut self, path: &Path, recursive: bool, mode: u32) -> HostResult<()> {
        self.base.create_dir(path, recursive, mode)
    }
    fn remove(&mut self, path: &Path, recursive: bool) -> HostResult<()> {
        self.base.remove(path, recursive)
    }
    fn rename(&mut self, from: &Path, to: &Path) -> HostResult<()> {
        self.base.rename(from, to)
    }
    fn canonicalize(&self, path: &Path) -> HostResult<PathBuf> {
        self.base.canonicalize(path)
    }
    fn cwd(&self) -> HostResult<PathBuf> {
        self.base.cwd()
    }
    fn set_cwd(&mut self, path: &Path) -> HostResult<()> {
        self.base.set_cwd(path)
    }
    fn session_temp_dir(&self) -> &Path {
        self.base.session_temp_dir()
    }
    fn home_dir(&self) -> Option<PathBuf> {
        self.base.home_dir()
    }
    fn env_var(&self, key: &str) -> Option<Vec<u8>> {
        self.base.env_var(key)
    }
    fn set_env_var(&mut self, key: &str, value: Option<&[u8]>) -> HostResult<()> {
        self.base.set_env_var(key, value)
    }
    fn env_vars(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.base.env_vars()
    }
    fn locale(&self) -> LocaleInfo {
        self.base.locale()
    }
    fn tzif(&self, zone: &str) -> HostResult<Vec<u8>> {
        self.base.tzif(zone)
    }
    fn now(&self) -> WallTime {
        self.base.now()
    }
    fn monotonic(&self) -> Duration {
        self.base.monotonic()
    }
    fn cpu_times(&self) -> CpuTimes {
        self.base.cpu_times()
    }
    fn sleep(&mut self, duration: Duration) -> HostResult<()> {
        self.base.sleep(duration)
    }
    fn pid(&self) -> u32 {
        self.base.pid()
    }
    fn fill_entropy(&mut self, buf: &mut [u8]) {
        self.base.fill_entropy(buf);
    }
    fn sys_info(&self) -> SysInfo {
        self.base.sys_info()
    }
    fn spawn(&mut self, spec: &CommandSpec) -> HostResult<ProcessOutput> {
        self.base.spawn(spec)
    }
    fn poll_interrupt(&mut self) -> bool {
        self.base.poll_interrupt()
    }
    fn limits(&self) -> &Limits {
        self.base.limits()
    }
}

struct CtxRoots<'a> {
    frames: &'a [Frame],
    calls: &'a [(SexpId, SexpId)],
    loops: &'a [(FrameId, SexpId)],
    eval_env: SexpId,
    conditions: &'a crate::conditions::Conditions,
    options: &'a [(Sym, SexpId)],
    autoloads: SexpId,
    namespaces: &'a [NsRecord],
    pending_s3: &'a [DeferredS3],
    vm_stack: &'a crate::vm::VmStack,
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
        tracer.edge(self.autoloads);
        for rec in self.namespaces {
            rec.trace(tracer);
        }
        for entry in self.pending_s3 {
            entry.trace(tracer);
        }
        for &(_, value) in self.options {
            tracer.edge(value);
        }
        self.vm_stack.trace(tracer);
        for entry in self.extensions.values() {
            (entry.trace_fn)(entry.boxed.as_ref(), tracer);
        }
    }
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

    /// Construct a new [`Ctx`] with the given [`Host`] and [`GcPolicy`], registering core M0 primitives.
    pub fn new(host: Box<dyn Host>, policy: GcPolicy) -> Result<Self, InternalError> {
        let mut session = RSession::new(policy)?;
        let autoloads = session.alloc_env(BASE_ENV)?;
        let name_sym = session.intern("name")?;
        let autoloads_label = session.scalar_str("Autoloads")?;
        session.set_attrib(autoloads, name_sym, autoloads_label)?;
        session.env_set_enclos(GLOBAL_ENV, autoloads)?;
        let base_s3 = session.alloc_env(BASE_ENV)?;
        let s3_sym = session.intern(".__S3MethodsTable__.")?;
        session.env_define(BASE_NAMESPACE, s3_sym, base_s3)?;
        let max_expressions = host.limits().max_expressions;
        let jit_level = host
            .env_var("ROXIDE_JIT")
            .or_else(|| host.env_var("R_ENABLE_JIT"))
            .and_then(|v| std::str::from_utf8(&v).ok()?.trim().parse::<u8>().ok())
            .map_or(3, |n| n.min(3));
        let opts = OptionsCache {
            expressions: max_expressions,
            ..OptionsCache::default()
        };
        let mut ctx = Self {
            session,
            box_muller_spare: None,
            host: HostStack {
                base: host,
                captures: Vec::new(),
                next_capture_id: 1,
            },
            sources: SourceMap::new(),
            opts,
            prims: Vec::new(),
            internals: HashMap::new(),
            registered_prims: HashSet::new(),
            overridable_prims: HashSet::new(),
            overridable_internals: HashSet::new(),
            namespaces: Vec::new(),
            pending_s3: Vec::new(),
            namespace_loader: None,
            autoloads,
            frames: Vec::new(),
            loops: Vec::new(),
            conditions: Default::default(),
            show_error_messages: true,
            options: Vec::new(),
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
            vm_stack: Default::default(),
            extensions: HashMap::new(),
            running_finalizers: false,
            s4_hooks: None,
            native_hooks: None,
            altrep_aware: HashSet::new(),
            jit_level,
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

    /// Register a non-base R [`Package`] (`stats`, `utils`, `tools`, ...) (ADR-0011, ADR-0013).
    ///
    /// Creates `namespace:<name> -> imports:<name> -> BASE_NAMESPACE`, registers all package modules
    /// and preludes into that namespace, populates `.__NAMESPACE__.` and `.__S3MethodsTable__.`,
    /// and when `pkg.attach` is true attaches `package:<name>` at position 2 on `search()`.
    pub fn register_package(&mut self, pkg: &Package) -> Result<(), InternalError> {
        if pkg.name == "base" || self.namespaces.iter().any(|p| p.name == pkg.name) {
            return Err(InternalError {
                message: format!("package '{}' is already registered", pkg.name),
            });
        }
        let (ns, imports) = self
            .ns_create(pkg.name, "4.6.2", None)
            .map_err(|_| InternalError {
                message: format!("failed to create namespace '{}'", pkg.name),
            })?;
        let mark = self.session.heap.protect_mark();
        self.session.heap.protect(ns);
        self.session.heap.protect(imports);
        let result = (|| -> Result<(), InternalError> {
            for module in pkg.modules {
                self.register_module_in(module, ns, false)?;
            }
            let exports_env = self
                .ns_exports_env(ns)
                .map_err(|_| InternalError {
                    message: format!("missing exports env for '{}'", pkg.name),
                })?
                .ok_or_else(|| InternalError {
                    message: format!("missing exports env for '{}'", pkg.name),
                })?;
            self.session.heap.protect(exports_env);

            let mut exported_bindings = Vec::new();
            if let Some(explicit) = pkg.exports {
                for &name in explicit {
                    let sym = self.session.intern(name)?;
                    let val = self.session.scalar_str(name)?;
                    self.session.env_define(exports_env, sym, val)?;
                    if let Some(binding) = self.session.env_get(ns, sym) {
                        exported_bindings.push((sym, binding));
                    }
                }
            } else {
                for (sym, binding) in self.session.env_entries(ns)? {
                    let sym_name = self.session.sym_name(sym)?.to_owned();
                    if !sym_name.starts_with('.') {
                        let val = self.session.scalar_str(&sym_name)?;
                        self.session.env_define(exports_env, sym, val)?;
                        exported_bindings.push((sym, binding));
                    }
                }
            }

            self.register_builtin_s3_methods(ns)
                .map_err(|_| InternalError {
                    message: format!("failed to register S3 methods for '{}'", pkg.name),
                })?;

            let attached = if pkg.attach {
                let pkg_env = self.session.alloc_env(EMPTY_ENV)?;
                self.session.heap.protect(pkg_env);
                for (sym, binding) in exported_bindings {
                    self.session.env_define_binding(pkg_env, sym, binding)?;
                }
                self.search_attach(pkg_env, 2, &format!("package:{}", pkg.name))
                    .map_err(|_| InternalError {
                        message: format!("failed to attach 'package:{}'", pkg.name),
                    })?;
                Some(pkg_env)
            } else {
                None
            };

            self.ns_register(NsRecord {
                name: pkg.name.to_owned(),
                ns,
                imports,
                attached,
                path: None,
                builtin: true,
            })
            .map_err(|_| InternalError {
                message: format!("failed to register namespace '{}'", pkg.name),
            })?;
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
        &mut self.host
    }

    /// Capture the selected host streams while running `f`.
    ///
    /// With `split`, copy each write to the previous stream destination as well.
    /// The capture ends when `f` returns, including when it returns an R error.
    pub fn capture_streams<R>(
        &mut self,
        capture_out: bool,
        capture_err: bool,
        split: bool,
        f: impl FnOnce(&mut Self) -> EvalResult<R>,
    ) -> EvalResult<(R, Vec<u8>)> {
        self.with_stream_capture(capture_out, capture_err, split, None, f)
    }

    /// Send selected host output to `sink` before the next R primitive runs.
    ///
    /// Each capture limits the total number of bytes that it accepts.
    pub fn capture_streams_to(
        &mut self,
        capture_out: bool,
        capture_err: bool,
        split: bool,
        sink: impl FnMut(&mut Self, &[u8]) -> EvalResult<()> + 'static,
        f: impl FnOnce(&mut Self) -> EvalResult<SexpId>,
    ) -> EvalResult {
        self.with_stream_capture(
            capture_out,
            capture_err,
            split,
            Some(Box::new(sink)),
            |ctx| {
                ctx.scoped(|ctx| {
                    let result = f(ctx);
                    ctx.protect_eval_result(&result);
                    let written = ctx.flush_stream_captures();
                    result.and_then(|value| written.map(|()| value))
                })
            },
        )
        .map(|(value, _)| value)
    }

    /// Push a persistent stream diversion (used by `base::sink()`).
    pub fn push_stream_sink(
        &mut self,
        capture_out: bool,
        capture_err: bool,
        split: bool,
        sink: impl FnMut(&mut Self, &[u8]) -> EvalResult<()> + 'static,
    ) {
        let id = self.host.next_capture_id;
        self.host.next_capture_id += 1;
        self.host.captures.push(StreamCapture {
            id,
            capture_out,
            capture_err,
            split,
            persistent: true,
            exceeded: false,
            received: 0,
            bytes: Vec::new(),
            sink: Some(Box::new(sink)),
            flushing: false,
        });
    }

    /// Flush buffered output and pop the topmost persistent diversion for stdout (`message = false`)
    /// or stderr (`message = true`). Returns `true` if a matching sink was popped.
    pub fn pop_stream_sink(&mut self, message: bool) -> EvalResult<bool> {
        let flushed = self.flush_stream_captures();
        let pos = self.host.captures.iter().rposition(|capture| {
            capture.persistent
                && if message {
                    capture.capture_err
                } else {
                    capture.capture_out
                }
        });
        let removed = pos.map(|index| self.host.captures.remove(index)).is_some();
        flushed?;
        Ok(removed)
    }

    /// Flush all buffered bytes queued on active stream sinks.
    pub fn flush_stream_sinks(&mut self) -> EvalResult<()> {
        self.flush_stream_captures()
    }

    fn with_stream_capture<R>(
        &mut self,
        capture_out: bool,
        capture_err: bool,
        split: bool,
        sink: Option<StreamSink>,
        f: impl FnOnce(&mut Self) -> EvalResult<R>,
    ) -> EvalResult<(R, Vec<u8>)> {
        let id = self.host.next_capture_id;
        self.host.next_capture_id += 1;
        self.host.captures.push(StreamCapture {
            id,
            capture_out,
            capture_err,
            split,
            persistent: false,
            exceeded: false,
            received: 0,
            bytes: Vec::new(),
            sink,
            flushing: false,
        });
        let result = f(self);
        let Some(pos) = self.host.captures.iter().rposition(|c| c.id == id) else {
            return Err(self.stop(SEXP_NIL, "missing output capture"));
        };
        let capture = self.host.captures.remove(pos);
        if capture.exceeded {
            return Err(self.stop(SEXP_NIL, "output capture exceeds the vector size limit"));
        }
        Ok((result?, capture.bytes))
    }

    pub(crate) fn has_stream_sink(&self) -> bool {
        self.host
            .captures
            .iter()
            .any(|capture| capture.sink.is_some())
    }

    fn flush_stream_captures(&mut self) -> EvalResult<()> {
        if self.host.captures.iter().any(|capture| capture.flushing) {
            return Ok(());
        }
        let ids: Vec<u64> = self
            .host
            .captures
            .iter()
            .rev()
            .map(|capture| capture.id)
            .collect();
        for id in ids {
            let Some(capture) = self.host.captures.iter_mut().find(|c| c.id == id) else {
                continue;
            };
            if capture.bytes.is_empty() {
                continue;
            }
            let Some(mut sink) = capture.sink.take() else {
                continue;
            };
            let bytes = std::mem::take(&mut capture.bytes);
            capture.flushing = true;
            let visible = self.visible;
            let result = sink(self, &bytes);
            self.visible = visible;
            if let Some(capture) = self.host.captures.iter_mut().find(|c| c.id == id) {
                capture.flushing = false;
                capture.sink = Some(sink);
            }
            result?;
        }
        Ok(())
    }

    pub(crate) fn invoke_streaming_primitive(
        &mut self,
        fun: crate::PrimFn,
        call: &Call,
        args: &mut Args,
    ) -> EvalResult {
        self.flush_stream_captures()?;
        let result = fun(self, call, args);
        self.scoped(|ctx| {
            ctx.protect_eval_result(&result);
            let written = ctx.flush_stream_captures();
            result.and_then(|value| written.map(|()| value))
        })
    }

    /// Return the registered name of a primitive, if the value is a primitive.
    #[must_use]
    pub fn primitive_name(&self, value: SexpId) -> Option<&'static str> {
        let (id, _) = self.session.prim_parts(value).ok()?;
        self.prims.get(usize::from(id.0)).map(|def| def.name)
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

    /// Return the configured evaluator stack budget in bytes.
    #[must_use]
    pub fn stack_budget(&self) -> usize {
        self.stack_budget
    }

    /// Install the stateless S4 dispatch hook.
    pub fn set_s4_hooks(&mut self, hooks: &'static dyn S4Hooks) {
        self.s4_hooks = Some(hooks);
    }

    /// Print an S4 value through installed `S4Hooks::show`, returning `true` if handled.
    pub fn show_s4(&mut self, value: SexpId, env: SexpId) -> EvalResult<bool> {
        if let Some(hooks) = self.s4_hooks {
            hooks.show(self, value, env)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Query the full S4 superclass chain (`STRSXP`) for `value` via `S4Hooks::class_chain`.
    pub fn s4_class_chain(&mut self, value: SexpId) -> EvalResult<Option<SexpId>> {
        if let Some(hooks) = self.s4_hooks {
            hooks.class_chain(self, value)
        } else {
            Ok(None)
        }
    }

    /// Validate/coerce an S4 slot assignment via `S4Hooks::check_slot`.
    pub fn check_s4_slot(&mut self, obj: SexpId, name: Sym, value: SexpId) -> EvalResult<SexpId> {
        if let Some(hooks) = self.s4_hooks {
            hooks.check_slot(self, obj, name, value)
        } else {
            Ok(value)
        }
    }

    /// Return the identifier of the innermost active closure frame, if any.
    #[must_use]
    pub fn current_frame_id(&self) -> Option<FrameId> {
        self.frames.last().map(|frame| frame.id)
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

    /// Protect a [`SexpId`] on the heap's protect stack.
    pub fn protect(&mut self, id: SexpId) {
        self.session.heap.protect(id);
    }

    /// Protect any R values in an evaluation result during subsequent evaluation.
    ///
    /// This also protects values carried by a non-local transfer, such as an
    /// error condition. Use it before running cleanup that can collect garbage.
    pub fn protect_eval_result(&mut self, result: &EvalResult) {
        self.protect_result(result);
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

    /// Return the extension state if this context has initialized it.
    pub fn ext_ref<T: CtxExt>(&self) -> Option<&T> {
        self.extensions
            .get(&TypeId::of::<T>())?
            .boxed
            .downcast_ref()
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

    /// Skip automatic ALTREP materialization for these primitives.
    pub fn mark_altrep_aware(&mut self, names: &[&'static str]) {
        self.altrep_aware.extend(names.iter().copied());
    }

    pub(crate) fn is_altrep_aware(&self, name: &str) -> bool {
        self.altrep_aware.contains(name)
    }

    /// Materialize a lazy foreign ALTREP value and its direct lazy foreign ALTREP attributes.
    pub fn resolve_altrep(&mut self, x: SexpId) -> EvalResult<()> {
        if !self.session.foreign_alt_seen() {
            return Ok(());
        }
        self.scoped(|ctx| {
            ctx.protect(x);
            ctx.materialize_foreign_altrep(x)?;
            let attrs = ctx.session.attrs(x)?;
            for &(_, value) in &attrs {
                ctx.protect(value);
            }
            for (_, value) in attrs {
                ctx.materialize_foreign_altrep(value)?;
            }
            Ok(())
        })
    }

    /// Resolve foreign ALTREP values in lists, pairlists, calls, expressions, and attributes.
    /// Stop at environments, closures, and promises.
    pub fn resolve_altrep_deep(&mut self, x: SexpId) -> EvalResult<()> {
        if !self.session.foreign_alt_seen() {
            return Ok(());
        }
        let mut pending = vec![x];
        let mut seen = HashSet::from([x]);
        self.session.heap.preserve(x);
        let result = (|| {
            while let Some(value) = pending.pop() {
                let ty = self.session.type_of(value)?;
                if matches!(ty, SexpType::Env | SexpType::Clo | SexpType::Prom) {
                    continue;
                }
                self.resolve_altrep(value)?;
                let mut children: Vec<_> = self
                    .session
                    .attrs(value)?
                    .into_iter()
                    .map(|(_, attr)| attr)
                    .collect();
                match ty {
                    SexpType::Vec | SexpType::Expr => {
                        children.extend_from_slice(self.session.list_slice(value)?);
                    }
                    SexpType::List | SexpType::Lang => {
                        let (car, cdr, _) = self.session.cons_parts(value)?;
                        children.push(car);
                        if cdr != SEXP_NIL {
                            children.push(cdr);
                        }
                    }
                    _ => {}
                }
                for child in children {
                    if seen.insert(child) {
                        self.session.heap.preserve(child);
                        pending.push(child);
                    }
                }
            }
            Ok(())
        })();
        for value in seen {
            self.session.heap.release(value);
        }
        result
    }

    /// Get the vector length without materializing its elements.
    pub fn altrep_len(&mut self, x: SexpId) -> EvalResult<usize> {
        if self.session.foreign_alt_seen()
            && self
                .session
                .foreign_alt(x)?
                .is_some_and(|info| !info.owned && info.len.is_none())
        {
            let AltReply::Len(len) = self.invoke_altrep(AltOp::Length(x))? else {
                return Err(altrep_reply_error());
            };
            self.session.seal_foreign_alt_len(x, len)?;
        }
        Ok(self.session.xlength(x)?)
    }

    /// Get the vector's known sortedness without materializing its elements.
    pub fn altrep_sorted(&mut self, x: SexpId) -> EvalResult<Sortedness> {
        if !self.session.foreign_alt_seen()
            || !self.session.foreign_alt(x)?.is_some_and(|info| !info.owned)
        {
            return Ok(self.session.alt_is_sorted(x)?);
        }
        let AltReply::Flag(code) = self.invoke_altrep(AltOp::IsSorted(x))? else {
            return Err(altrep_reply_error());
        };
        Ok(match code {
            1 => Sortedness::Increasing,
            2 => Sortedness::IncreasingNaFirst,
            -1 => Sortedness::Decreasing,
            -2 => Sortedness::DecreasingNaFirst,
            _ => Sortedness::Unknown,
        })
    }

    /// Check for a guarantee that the vector has no missing elements.
    pub fn altrep_no_na(&mut self, x: SexpId) -> EvalResult<bool> {
        if !self.session.foreign_alt_seen()
            || !self.session.foreign_alt(x)?.is_some_and(|info| !info.owned)
        {
            return Ok(self.session.alt_no_na(x)?);
        }
        let AltReply::Flag(flag) = self.invoke_altrep(AltOp::NoNa(x))? else {
            return Err(altrep_reply_error());
        };
        Ok(flag != 0)
    }

    /// Run a foreign ALTREP method that returns an optional R value.
    /// Resolve vector results from subset, coercion, and summary methods.
    pub fn altrep_method(&mut self, op: AltOp) -> EvalResult<Option<SexpId>> {
        let AltReply::Value(value) = self.invoke_altrep(op)? else {
            return Err(altrep_reply_error());
        };
        if let Some(x) = value
            && matches!(
                op,
                AltOp::ExtractSubset { .. }
                    | AltOp::Coerce { .. }
                    | AltOp::Sum { .. }
                    | AltOp::Min { .. }
                    | AltOp::Max { .. }
            )
        {
            self.resolve_altrep(x)?;
        }
        Ok(value)
    }

    fn materialize_foreign_altrep(&mut self, x: SexpId) -> EvalResult<()> {
        if self.session.foreign_alt(x)?.is_some_and(|info| !info.owned)
            && self.invoke_altrep(AltOp::Materialize(x))? != AltReply::Unit
        {
            return Err(altrep_reply_error());
        }
        Ok(())
    }

    fn invoke_altrep(&mut self, op: AltOp) -> EvalResult<AltReply> {
        let hooks = self.native_hooks().ok_or_else(altrep_hook_missing)?;
        let roots: &[SexpId] = match op {
            AltOp::Length(x)
            | AltOp::Materialize(x)
            | AltOp::IsSorted(x)
            | AltOp::NoNa(x)
            | AltOp::Sum { x, .. }
            | AltOp::Min { x, .. }
            | AltOp::Max { x, .. }
            | AltOp::Coerce { x, .. }
            | AltOp::SerializedState(x) => &[x],
            AltOp::ExtractSubset { x, indx, call } => &[x, indx, call],
            AltOp::Unserialize {
                class,
                package,
                state,
                attrs,
                ..
            } => &[class.as_sexp(), package.as_sexp(), state, attrs],
        };
        self.scoped(|ctx| {
            for &root in roots {
                ctx.protect(root);
            }
            hooks.altrep(ctx, op)
        })
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

    /// Explicit GC safepoint (ADR-0002): runs collection only if policy requests it.
    ///
    /// Does not run queued R/C finalizers directly so C-API allocating paths can call
    /// `safepoint()` safely; evaluator safepoints drain finalizers via [`Ctx::run_pending_finalizers`].
    pub fn safepoint(&mut self) {
        if self.session.heap.wants_collection() {
            let _ = self.collect_gc();
        }
    }

    /// Force an immediate full mark-sweep garbage collection, queuing dead ephemeron finalizers.
    pub fn collect_gc(&mut self) -> roxide_gc::GcStats {
        let roots = CtxRoots {
            frames: &self.frames,
            calls: &self.calls,
            loops: &self.loops,
            eval_env: self.eval_env,
            conditions: &self.conditions,
            options: &self.options,
            autoloads: self.autoloads,
            namespaces: &self.namespaces,
            pending_s3: &self.pending_s3,
            vm_stack: &self.vm_stack,
            extensions: &self.extensions,
        };
        self.session.collect(&roots)
    }

    /// Execute any GC finalizers queued by completed collections (re-entrancy safe, ADR-0019).
    pub fn run_pending_finalizers(&mut self) {
        if self.running_finalizers || !self.session.heap.has_pending_finalizers() {
            return;
        }
        self.running_finalizers = true;
        let visible = self.visible;
        loop {
            let batch = self.session.heap.drain_pending_finalizers();
            if batch.is_empty() {
                break;
            }
            let _ = self.scoped(|ctx| {
                for fin in &batch {
                    ctx.protect(fin.target);
                    if let roxide_gc::FinalizerKind::RClosure(fun) = fin.kind {
                        ctx.protect(fun);
                    }
                }
                for fin in batch.into_iter().rev() {
                    match fin.kind {
                        roxide_gc::FinalizerKind::RClosure(fun) => {
                            let _ = ctx.apply(fun, &[(None, fin.target)], SEXP_NIL, GLOBAL_ENV);
                        }
                        roxide_gc::FinalizerKind::Native { fun, .. } => {
                            if let Some(hooks) = ctx.native_hooks {
                                let _ = hooks.run_c_finalizer(ctx, fun, fin.target);
                            }
                        }
                    }
                }
                Ok(())
            });
        }
        self.visible = visible;
        self.running_finalizers = false;
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

    /// Emit a warning at `call` honoring `options(warn=)` and active warning handlers (`Rf_warning`).
    pub fn warning_at(&mut self, call: SexpId, msg: &str) -> EvalResult<()> {
        self.warn_now(call, msg)
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

    /// Public C-API error signaling helper (`Rf_error` / `Rf_errorcall`).
    pub fn signal_error_at(&mut self, call: SexpId, msg: &str) -> Unwind {
        self.stop(call, msg)
    }

    /// Check for pending user interrupts (`R_CheckUserInterrupt`).
    pub fn check_user_interrupt(&mut self) -> EvalResult<()> {
        if self.host.poll_interrupt() {
            return Err(self.stop(SEXP_NIL, "interrupted"));
        }
        Ok(())
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
        self.run_script_with_opts(
            name,
            src,
            env,
            ScriptOpts {
                auto_print,
                echo: false,
            },
        )
    }

    /// Parse and execute a complete R script string in `env` with explicit [`ScriptOpts`].
    pub fn run_script_with_opts(
        &mut self,
        name: &str,
        src: &str,
        env: SexpId,
        opts: ScriptOpts,
    ) -> EvalResult<()> {
        let fid: SrcFileId = self.sources.add_file(name, src);
        let mut parser = Parser::new(src, fid);
        let mut last_echoed = 0usize;
        loop {
            match parser.parse_next() {
                ParseStep::Eof => {
                    if opts.echo
                        && let Some(tail) = src.get(last_echoed..)
                        && !tail.is_empty()
                    {
                        for line in tail.lines() {
                            let _ = self.host.write_out(b"> ");
                            let _ = self.host.write_out(line.as_bytes());
                            let _ = self.host.write_out(b"\n");
                        }
                    }
                    let _ = self.flush_stream_captures();
                    self.flush_warnings();
                    let _ = self.flush_stream_captures();
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
                    if opts.echo {
                        let span = parser.ast().span(node);
                        let lo = (span.lo as usize).min(src.len());
                        let hi = (span.hi as usize).min(src.len());
                        if lo >= last_echoed
                            && let Some(between) = src.get(last_echoed..lo)
                        {
                            for line in between.lines() {
                                let _ = self.host.write_out(b"> ");
                                let _ = self.host.write_out(line.as_bytes());
                                let _ = self.host.write_out(b"\n");
                            }
                        }
                        if lo <= hi
                            && let Some(snippet) = src.get(lo..hi)
                        {
                            for (idx, line) in snippet.lines().enumerate() {
                                let prefix = if idx == 0 { "> " } else { "+ " };
                                let _ = self.host.write_out(prefix.as_bytes());
                                let _ = self.host.write_out(line.as_bytes());
                                let _ = self.host.write_out(b"\n");
                            }
                            last_echoed = hi;
                            if src.as_bytes().get(last_echoed) == Some(&b'\r') {
                                last_echoed += 1;
                            }
                            if src.as_bytes().get(last_echoed) == Some(&b'\n') {
                                last_echoed += 1;
                            }
                        }
                    }
                    let expr = lower(parser.ast(), node, &mut self.session, KeepSource::No);
                    let expr = self.condition_result(SEXP_NIL, expr.map_err(Into::into))?;
                    let val = self.scoped(|ctx| {
                        ctx.protect(expr);
                        ctx.visible = true;
                        let v = ctx.eval(expr, env)?;
                        if opts.auto_print && ctx.visible {
                            ctx.protect(v);
                            if ctx.session.is_s4(v)? && ctx.show_s4(v, env)? {
                                // Printed by S4 `show()`.
                            } else if ctx.session.is_object(v)? {
                                let print = ctx.session.intern("print")?;
                                let print = ctx.find_fun(print, BASE_ENV)?;
                                ctx.apply(print, &[(None, v)], SEXP_NIL, env)?;
                            } else {
                                let default = ctx.session.intern("print.default")?;
                                if let Some(print) = ctx.get_local(default, BASE_ENV)? {
                                    ctx.apply(print, &[(None, v)], SEXP_NIL, env)?;
                                } else {
                                    builtins::print_value(ctx, v, ctx.opts.digits)?;
                                }
                            }
                        }
                        Ok(v)
                    })?;
                    let _ = val;
                    self.run_pending_finalizers();
                    let _ = self.flush_stream_captures();
                    self.flush_warnings();
                    let _ = self.flush_stream_captures();
                }
            }
        }
    }

    /// Convenience entrypoint for running top-level Rscript code in `.GlobalEnv`.
    pub fn run_global(&mut self, name: &str, src: &str) -> EvalResult<()> {
        self.run_script(name, src, GLOBAL_ENV, true)
    }
}

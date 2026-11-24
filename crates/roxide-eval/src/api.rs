//! Public parallel-worker contracts for `roxide-eval` (ADR-0006 & ADR-0007).

use std::any::Any;
use std::fmt;
use std::path::Path;

use roxide_gc::{SexpId, Trace};
use roxide_value::{PrimId, SEXP_NIL, Sym, ValueApi, ValueError};

use crate::ctx::{Args, Ctx, FrameId};

/// Result type for all fallible R evaluation operations.
pub type EvalResult<T = SexpId> = Result<T, Unwind>;

/// Non-local control transfer kind targeting a specific evaluation frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transfer {
    /// `return(value)` targeting its lexical closure frame.
    Return(SexpId),
    /// `break` targeting the innermost loop frame in the same environment.
    Break,
    /// `next` targeting the innermost loop frame in the same environment.
    Next,
    /// Exiting `tryCatch` handler transfer carrying handler index and condition object.
    ExitingHandler {
        /// Index into handler table.
        handler: usize,
        /// Condition `SEXP`.
        cond: SexpId,
    },
    /// `invokeRestart` transfer.
    Restart {
        /// Restart object handle.
        restart: SexpId,
        /// Argument list handle.
        args: SexpId,
    },
}

/// Engine-level internal error (never panics; surfaced as an R error).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InternalError {
    /// Human-readable diagnostic message.
    pub message: String,
}

impl From<ValueError> for InternalError {
    fn from(value: ValueError) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

impl fmt::Display for InternalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InternalError {}

/// Typed unwind channel propagated via `Result<T, Unwind>` across safe Rust frames (ADR-0006).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Unwind {
    /// Structured jump (`return`, `break`, `next`, exiting condition handler, restart).
    Transfer {
        /// Destination frame identifier.
        target: FrameId,
        /// Transfer payload.
        kind: Transfer,
    },
    /// Unhandled error already reported by the default handler; unwind to REPL/Rscript top level.
    TopLevel,
    /// Graceful `q()` termination.
    Quit {
        /// Process exit code.
        status: i32,
        /// Whether `.Last()` should execute.
        run_last: bool,
    },
    /// Value operation error awaiting translation at an evaluation boundary.
    Value(ValueError),
    /// Engine invariant failure (e.g. stale `SEXP` or memory limit).
    Internal(InternalError),
}

impl From<ValueError> for Unwind {
    fn from(value: ValueError) -> Self {
        match value {
            ValueError::Heap(_) | ValueError::Shared(_) => Self::Internal(value.into()),
            _ => Self::Value(value),
        }
    }
}

/// Builtin / internal function pointer signature.
pub type PrimFn = fn(&mut Ctx, &Call, &mut Args) -> EvalResult;

/// Whether a primitive is exposed as `.Primitive` or `.Internal`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrimKind {
    /// Exposed directly in `base` as a primitive (`SPECIALSXP` / `BUILTINSXP`).
    Primitive,
    /// Callable only through `.Internal(name(...))`.
    Internal,
}

/// Argument evaluation policy for a primitive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArgEval {
    /// Eagerly evaluate all arguments (`BUILTINSXP`).
    Eval,
    /// Eagerly evaluate present arguments while preserving `SEXP_MISSING` slots (e.g. `[`).
    EvalKeepMissing,
    /// Evaluate the object and replacement value, keeping the dollar field name as written.
    EvalDollar,
    /// Pass unevaluated argument expressions (`SPECIALSXP`).
    Unevaluated,
}

/// Expected argument count for a primitive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arity {
    /// Exact argument count.
    Exact(usize),
    /// Inclusive range `[lo, hi]`.
    Range(usize, usize),
    /// Any number of arguments.
    Any,
}

/// S3 group generic categories.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupGeneric {
    /// `Ops` group (`+`, `-`, `*`, `/`, `^`, comparisons, logic).
    Ops,
    /// `Math` group (`abs`, `sqrt`, `exp`, `log`, `floor`, ...).
    Math,
    /// `Summary` group (`sum`, `prod`, `min`, `max`, `range`, `all`, `any`).
    Summary,
    /// `Complex` group (`Re`, `Im`, `Mod`, `Arg`, `Conj`).
    Complex,
    /// `matrixOps` group (`%*%`).
    MatrixOps,
}

/// Automatic S3/S4 dispatch policy run by the evaluator prior to calling `PrimDef::fun`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dispatch {
    /// No automatic dispatch.
    None,
    /// Standard internal S3 generic dispatch by name (e.g. `"length"`).
    Internal(&'static str),
    /// Group generic dispatch.
    Group(GroupGeneric),
}

/// Result visibility rule after a primitive returns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visibility {
    /// Always visible.
    On,
    /// Always invisible (e.g. `<-`, `print`, `cat`).
    Off,
    /// Function calls `ctx.set_visible(...)` explicitly.
    SetByFn,
}

/// Static descriptor for one built-in primitive or `.Internal` routine.
pub struct PrimDef {
    /// R function name.
    pub name: &'static str,
    /// `.Primitive` vs `.Internal`.
    pub kind: PrimKind,
    /// Argument evaluation mode.
    pub args: ArgEval,
    /// Arity constraint.
    pub arity: Arity,
    /// Automatic S3/S4 dispatch rule.
    pub dispatch: Dispatch,
    /// Visibility behavior.
    pub visibility: Visibility,
    /// Rust implementation pointer.
    pub fun: PrimFn,
}

/// Call-site metadata passed to a [`PrimFn`].
#[derive(Clone, Copy, Debug)]
pub struct Call {
    /// Original language expression `LANGSXP`.
    pub expr: SexpId,
    /// Environment in which the call was evaluated.
    pub env: SexpId,
    /// Primitive registry identifier.
    pub prim: PrimId,
    /// User-facing call expression for `Error in ...` diagnostics.
    pub user_call: SexpId,
}

impl Call {
    /// Return the call expression to display in `Error in ...` diagnostics.
    #[inline]
    #[must_use]
    pub const fn err_call(&self) -> SexpId {
        self.user_call
    }
}

/// Registration unit exported by each `roxide-base` / `roxide-stats` / `roxide-utils` submodule (ADR-0007).
pub struct Module {
    /// Static primitive / internal definitions owned by this submodule.
    pub prims: &'static [PrimDef],
    /// Optional clean-room R prelude evaluated during package bootstrap.
    pub prelude: &'static str,
}

/// Package registration descriptor for non-base R packages (`stats`, `utils`, `tools`, ...) (ADR-0011, ADR-0013).
pub struct Package {
    /// Package name (e.g. `"stats"`, `"utils"`, `"tools"`).
    pub name: &'static str,
    /// Whether to attach `package:<name>` onto the search path below `.GlobalEnv`.
    pub attach: bool,
    /// Optional explicit export list. `None` exports all bindings whose names do not start with `.`.
    pub exports: Option<&'static [&'static str]>,
    /// Ordered list of disjoint submodule units owned by this package.
    pub modules: &'static [&'static Module],
}

/// Execution options for [`Ctx::run_script_with_opts`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptOpts {
    /// Automatically print visible top-level results.
    pub auto_print: bool,
    /// Echo each top-level expression's source lines with `"> "` and `"+ "` prompts (`R CMD BATCH`).
    pub echo: bool,
}

impl Default for ScriptOpts {
    fn default() -> Self {
        Self {
            auto_print: true,
            echo: false,
        }
    }
}

/// Cached hot session options (`digits`, `scipen`, `warn`, `expressions`, `width`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OptionsCache {
    /// `options(digits)`, default 7.
    pub digits: usize,
    /// `options(scipen)`, default 0.
    pub scipen: i32,
    /// `options(warn)`, default 0.
    pub warn: i32,
    /// `options(expressions)`, default 5000.
    pub expressions: usize,
    /// `options(width)`, default 80.
    pub width: usize,
    /// `options(warnPartialMatchDollar)`, default false.
    pub warn_partial_match_dollar: bool,
    /// `options(warnPartialMatchArgs)`, default false.
    pub warn_partial_match_args: bool,
}

impl Default for OptionsCache {
    fn default() -> Self {
        Self {
            digits: 7,
            scipen: 0,
            warn: 0,
            expressions: 5000,
            width: 80,
            warn_partial_match_dollar: false,
            warn_partial_match_args: false,
        }
    }
}

/// GC-traced per-module session state stored inside [`Ctx`] via `ctx.ext::<T>()` (ADR-0007).
pub trait CtxExt: Any + Trace + 'static {}

/// Stateless S4 dispatch and object hooks (ADR-0012). Store session state in a traced [`CtxExt`].
pub trait S4Hooks {
    /// Attempt S4 primitive/group generic dispatch; return `Ok(Some(res))` if handled.
    fn dispatch(
        &self,
        ctx: &mut Ctx,
        generic: Sym,
        call: &Call,
        args: &Args,
    ) -> EvalResult<Option<SexpId>> {
        let _ = (ctx, generic, call, args);
        Ok(None)
    }

    /// Execute `standardGeneric(name)` in the active generic call `frame`.
    fn standard_generic(&self, ctx: &mut Ctx, name: Sym, frame: FrameId) -> EvalResult<SexpId> {
        let _ = (name, frame);
        Err(ctx.stop(SEXP_NIL, "S4 methods support is not loaded"))
    }

    /// Print an S4 object via `methods::show(value)`.
    fn show(&self, ctx: &mut Ctx, value: SexpId, env: SexpId) -> EvalResult<()> {
        let _ = env;
        ctx.print_value_default(value, ctx.opts().digits)
    }

    /// Return the full S4 superclass chain (`STRSXP`) used by S3 dispatch and `inherits()` on S4 instances.
    fn class_chain(&self, ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<SexpId>> {
        let _ = (ctx, value);
        Ok(None)
    }

    /// Validate and optionally coerce `value` prior to storing into S4 slot `name` on `obj`.
    fn check_slot(
        &self,
        ctx: &mut Ctx,
        obj: SexpId,
        name: Sym,
        value: SexpId,
    ) -> EvalResult<SexpId> {
        let _ = (ctx, obj, name);
        Ok(value)
    }
}

/// On-demand package namespace loader hook installed by `roxide-pkg` (ADR-0013).
pub trait NamespaceLoader {
    /// Load and register the namespace for `name`, returning its `ENVSXP`.
    fn load(&self, ctx: &mut Ctx, name: &str, call: SexpId) -> EvalResult<SexpId>;
}

/// Foreign function interface dispatch kind (`.Call`, `.External`, `.External2`, `.C`, `.Fortran`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NativeCallKind {
    /// `.Call(...)`
    Call,
    /// `.External(...)`
    External,
    /// `.External2(...)`
    External2,
    /// `.C(...)`
    C,
    /// `.Fortran(...)`
    Fortran,
}

/// Stateless native C-API & dynamic-library hooks installed by `roxide-capi` (ADRs 0017–0020).
pub trait NativeHooks {
    /// Invoke a registered or dynamically looked-up native symbol via `.Call`/`.External`/`.C`/`.Fortran`.
    fn call_native(
        &self,
        ctx: &mut Ctx,
        kind: NativeCallKind,
        symbol: SexpId,
        args: &Args,
        package: Option<&str>,
        call: &Call,
    ) -> EvalResult<SexpId> {
        let _ = (kind, symbol, args, package);
        Err(ctx.stop(call.err_call(), "native C API support is not initialized"))
    }

    /// Load a compiled shared library (`dyn.load`) and return its `DLLInfo` object.
    fn dyn_load(
        &self,
        ctx: &mut Ctx,
        path: &Path,
        local: bool,
        now: bool,
        call: SexpId,
    ) -> EvalResult<SexpId> {
        let _ = (path, local, now);
        Err(ctx.stop(call, "native dynamic library loading is not initialized"))
    }

    /// Unload a previously loaded shared library (`dyn.unload`).
    fn dyn_unload(&self, ctx: &mut Ctx, path: &Path, call: SexpId) -> EvalResult<()> {
        let _ = path;
        Err(ctx.stop(call, "native dynamic library loading is not initialized"))
    }

    /// Check whether a native symbol is loaded (`is.loaded`).
    fn is_loaded(
        &self,
        ctx: &mut Ctx,
        symbol: &str,
        package: Option<&str>,
        kind: Option<NativeCallKind>,
    ) -> EvalResult<bool> {
        let _ = (ctx, symbol, package, kind);
        Ok(false)
    }

    /// Return a `NativeSymbolInfo` list (`getNativeSymbolInfo`).
    fn get_native_symbol_info(
        &self,
        ctx: &mut Ctx,
        name: &str,
        package: Option<&str>,
        with_registration: bool,
        call: SexpId,
    ) -> EvalResult<SexpId> {
        let _ = (name, package, with_registration);
        Err(ctx.stop(call, "native symbol lookup is not initialized"))
    }

    /// Return the list of loaded `DLLInfo` objects (`getLoadedDLLs`).
    fn get_loaded_dlls(&self, ctx: &mut Ctx) -> EvalResult<SexpId> {
        Ok(ctx.session_mut().alloc_list(0)?)
    }

    /// Return registered routines for a DLL (`getDLLRegisteredRoutines`).
    fn get_dll_registered_routines(
        &self,
        ctx: &mut Ctx,
        dll: SexpId,
        add_names: bool,
        call: SexpId,
    ) -> EvalResult<SexpId> {
        let _ = (dll, add_names);
        Err(ctx.stop(call, "native routine inspection is not initialized"))
    }

    /// Invoke a C finalizer function pointer (`R_CFinalizer_t`) across `roxide-capi`'s trampoline.
    fn run_c_finalizer(&self, ctx: &mut Ctx, fun_addr: usize, target: SexpId) {
        let _ = (ctx, fun_addr, target);
    }
}

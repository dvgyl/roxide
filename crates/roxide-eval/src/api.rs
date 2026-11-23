//! Public parallel-worker contracts for `roxide-eval` (ADR-0006 & ADR-0007).

use std::any::Any;
use std::fmt;

use roxide_gc::{SexpId, Trace};
use roxide_value::{PrimId, Sym, ValueError};

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

/// Registration unit exported by each `roxide-base` / `roxide-stats` submodule (ADR-0007).
pub struct Module {
    /// Static primitive / internal definitions owned by this submodule.
    pub prims: &'static [PrimDef],
    /// Optional clean-room R prelude evaluated during package bootstrap.
    pub prelude: &'static str,
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

/// Stateless S4 dispatch hook. Store session state in a traced [`CtxExt`].
pub trait S4Hooks {
    /// Attempt S4 dispatch; return `Ok(Some(res))` if handled, `Ok(None)` to fall through.
    fn dispatch(
        &self,
        ctx: &mut Ctx,
        generic: Sym,
        call: &Call,
        args: &Args,
    ) -> EvalResult<Option<SexpId>>;
}

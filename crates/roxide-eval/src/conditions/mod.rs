mod handlers;
mod objects;
mod options;
mod restarts;
mod signaling;

use roxide_gc::{SexpId, Tracer};

use crate::{Arity, Dispatch, FrameId, Module};

/// Register condition and option primitives.
pub(crate) const MODULE: Module = Module {
    prims: &[
        primitive!(
            "simpleCondition",
            objects::simple_condition,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "errorCondition",
            objects::custom_condition,
            Eval,
            Arity::Any,
            On
        ),
        primitive!(
            "warningCondition",
            objects::custom_condition,
            Eval,
            Arity::Any,
            On
        ),
        primitive!(
            "try",
            handlers::try_eval,
            Unevaluated,
            Arity::Range(1, 3),
            SetByFn
        ),
        primitive!(
            "geterrmessage",
            signaling::get_error_message,
            Eval,
            Arity::Exact(0),
            On
        ),
        primitive!(
            "packageStartupMessage",
            signaling::notify,
            Eval,
            Arity::Any,
            Off
        ),
        primitive!(
            "computeRestarts",
            restarts::compute_restarts,
            Eval,
            Arity::Range(0, 1),
            On
        ),
        primitive!(
            "tryInvokeRestart",
            restarts::invoke_restart,
            Eval,
            Arity::Any,
            SetByFn
        ),
        primitive!(
            "simpleError",
            objects::simple_condition,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "simpleWarning",
            objects::simple_condition,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "simpleMessage",
            objects::simple_condition,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "conditionMessage",
            objects::condition_field,
            Eval,
            Arity::Exact(1),
            On,
            Dispatch::Internal("conditionMessage")
        ),
        primitive!(
            "conditionCall",
            objects::condition_field,
            Eval,
            Arity::Exact(1),
            On,
            Dispatch::Internal("conditionCall")
        ),
        primitive!("stop", signaling::notify, Eval, Arity::Any, Off),
        primitive!("warning", signaling::notify, Eval, Arity::Any, Off),
        primitive!("message", signaling::notify, Eval, Arity::Any, Off),
        primitive!(
            "signalCondition",
            signaling::signal,
            Eval,
            Arity::Exact(1),
            Off
        ),
        primitive!(
            "tryCatch",
            handlers::with_handlers,
            Unevaluated,
            Arity::Any,
            SetByFn
        ),
        primitive!(
            "withCallingHandlers",
            handlers::with_handlers,
            Unevaluated,
            Arity::Any,
            SetByFn
        ),
        primitive!(
            "withRestarts",
            restarts::with_restarts,
            Unevaluated,
            Arity::Any,
            SetByFn
        ),
        primitive!(
            "invokeRestart",
            restarts::invoke_restart,
            Eval,
            Arity::Any,
            SetByFn
        ),
        primitive!(
            "findRestart",
            restarts::find_restart,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!("options", options::options, Eval, Arity::Any, SetByFn),
        primitive!(
            "getOption",
            options::get_option,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "stopifnot",
            signaling::stop_if_not,
            Unevaluated,
            Arity::Any,
            Off
        ),
    ],
    prelude: r#"
suppressWarnings <- function(expr, classes = "warning") {
    withCallingHandlers(expr, warning = function(w) {
        if (inherits(w, classes)) tryInvokeRestart("muffleWarning")
    })
}
suppressMessages <- function(expr, classes = "message") {
    withCallingHandlers(expr, message = function(m) {
        if (inherits(m, classes)) tryInvokeRestart("muffleMessage")
    })
}
suppressPackageStartupMessages <- function(expr) suppressMessages(expr, "packageStartupMessage")
.obsoleteMessage <- function(old, new, package, kind, topic) {
    msg <- paste0("'", old, "' is ", kind, ".")
    if (!is.null(new)) msg <- paste0(msg, "\nUse '", new, "' instead.")
    msg <- paste0(msg, "\nSee help(\"", topic, "\")")
    if (!is.null(package))
        msg <- paste0(msg, " and help(\"", package, "-", kind, "\").")
    msg
}
.Deprecated <- function(new, package = NULL, msg, old = as.character(sys.call(sys.parent()))[1L]) {
    if (missing(new)) new <- NULL
    if (missing(msg)) msg <- .obsoleteMessage(old, new, package, "deprecated", "Deprecated")
    warning(warningCondition(msg, old = old, new = new, package = package,
                             class = "deprecatedWarning", call = sys.call(sys.parent())))
    invisible(msg)
}
.Defunct <- function(new, package = NULL, msg) {
    if (missing(new)) new <- NULL
    call <- sys.call(sys.parent())
    old <- as.character(call)[1L]
    if (missing(msg)) msg <- .obsoleteMessage(old, new, package, "defunct", "Defunct")
    stop(errorCondition(msg, old = old, new = new, package = package,
                        class = "defunctError", call = call))
}
print.condition <- function(x, ...) {
    msg <- conditionMessage(x)
    call <- conditionCall(x)
    cl <- class(x)[1L]
    if (!is.null(call)) {
        cat("<", cl, " in ", deparse(call)[1L], ": ", msg, ">\n", sep = "")
    } else {
        cat("<", cl, ": ", msg, ">\n", sep = "")
    }
    invisible(x)
}
"#,
};

struct Handler {
    target: Option<FrameId>,
    class: String,
    fun: SexpId,
}

struct Restart {
    target: FrameId,
    name: String,
    value: SexpId,
    fun: SexpId,
}

/// Condition handlers, restarts, and the latest error text.
#[derive(Default)]
pub(crate) struct Conditions {
    handlers: Vec<Handler>,
    restarts: Vec<Restart>,
    /// Outer handler/restart stacks isolated by `Ctx::with_native_toplevel` (`R_ToplevelExec`),
    /// kept here so they remain GC-rooted across nested collections.
    saved_stacks: Vec<(Vec<Handler>, Vec<Restart>)>,
    last_error: String,
}

impl Conditions {
    /// Trace the R values in the current and saved condition stacks.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for handler in &self.handlers {
            tracer.edge(handler.fun);
        }
        for restart in &self.restarts {
            tracer.edge(restart.fun);
            tracer.edge(restart.value);
        }
        for (handlers, restarts) in &self.saved_stacks {
            for handler in handlers {
                tracer.edge(handler.fun);
            }
            for restart in restarts {
                tracer.edge(restart.fun);
                tracer.edge(restart.value);
            }
        }
    }
}

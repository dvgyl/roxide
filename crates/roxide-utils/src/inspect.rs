//! Inspect R objects, output, and session state.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod capture;
mod ends;
mod session;
mod size;

use roxide_eval::{
    ArgEval, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, RSession, SexpType, ValueApi};

/// Register the utils inspection functions.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_rox_inspect_ends", 4, ends::ends, Visibility::On),
        internal(
            "C_rox_capture_output",
            5,
            capture::capture,
            Visibility::SetByFn,
        ),
        internal("C_rox_object_size", 1, size::size, Visibility::On),
        internal("C_rox_format_size", 4, size::format, Visibility::On),
        internal("C_rox_session_info", 1, session::info, Visibility::On),
    ],
    prelude: r#"
head <- function(x, ...) UseMethod("head")
head.default <- function(x, n = 6L, ...) .Internal(C_rox_inspect_ends(x, n, FALSE, FALSE))
head.array <- head.matrix <- head.data.frame <- head.function <- head.default

tail <- function(x, ...) UseMethod("tail")
tail.default <- function(x, n = 6L, ...) .Internal(C_rox_inspect_ends(x, n, TRUE, FALSE))
tail.array <- tail.matrix <- function(x, n = 6L, keepnums = TRUE, ...)
    .Internal(C_rox_inspect_ends(x, n, TRUE, keepnums))
tail.data.frame <- function(x, n = 6L, keepnums = FALSE, ...)
    .Internal(C_rox_inspect_ends(x, n, TRUE, keepnums))
tail.function <- tail.default

capture.output <- function(..., file = NULL, append = FALSE, type = c("output", "message"),
                           split = FALSE)
    .Internal(C_rox_capture_output(file, append, type, split, parent.frame()))

object.size <- function(x) .Internal(C_rox_object_size(x))
format.object_size <- function(x, units = "auto", standard = "legacy", digits = 1, ...)
    .Internal(C_rox_format_size(x, units, standard, digits))
print.object_size <- function(x, quote = FALSE, units = "auto", ...)
    invisible({ cat(format.object_size(x, units = units, ...), "\n", sep = ""); x })

sessionInfo <- function(package = NULL) .Internal(C_rox_session_info(package))
print.sessionInfo <- function(x, ...) {
    cat(x$R.version$version.string, "\n", sep = "")
    cat("Platform: ", x$platform, "\nRunning under: ", x$running, "\n\n", sep = "")
    cat("Matrix products: ", x$matprod, "\n", sep = "")
    cat("BLAS: ", x$BLAS, "\nLAPACK: ", x$LAPACK, "\n\n", sep = "")
    cat("locale:\n", x$locale, "\n\n", sep = "")
    cat("time zone: ", x$tzone, "\ntzcode source: ", x$tzcode_type, "\n\n", sep = "")
    cat("attached base packages:\n")
    print(x$basePkgs)
    if (length(x$loadedOnly)) {
        cat("\nloaded via a namespace (and not attached):\n")
        print(names(x$loadedOnly))
    }
    invisible(x)
}
"#,
};

const fn internal(
    name: &'static str,
    count: usize,
    fun: PrimFn,
    visibility: Visibility,
) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(count),
        dispatch: Dispatch::None,
        visibility,
        fun,
    }
}

fn attr(s: &mut RSession, x: SexpId, key: &str) -> EvalResult {
    let key = s.intern(key)?;
    Ok(s.attr(x, key))
}

fn set(s: &mut RSession, x: SexpId, key: &str, value: SexpId) -> EvalResult<()> {
    let key = s.intern(key)?;
    Ok(s.set_attrib(x, key, value)?)
}

fn strings<B: AsRef<[u8]>>(s: &mut RSession, values: impl IntoIterator<Item = B>) -> EvalResult {
    let ids = values
        .into_iter()
        .map(|text| s.mk_char(text.as_ref(), Encoding::Native))
        .collect::<Result<Vec<CharId>, _>>()?;
    Ok(s.alloc_str_from(&ids)?)
}

fn flag(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<bool> {
    let value = ctx.session_mut().coerce(value, SexpType::Lgl)?.0;
    let values = ctx.session().lgl(value)?;
    if let Some(flag) = values.iter().next().and_then(|v| v.get()) {
        Ok(flag)
    } else {
        Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"))
    }
}

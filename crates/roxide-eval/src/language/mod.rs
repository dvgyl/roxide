mod arguments;
mod evaluation;
mod functions;
mod quotation;
mod source;
mod values;

use crate::{Arity, Module};

/// Register language primitives.
pub(crate) const MODULE: Module = Module {
    prims: &[
        primitive!(
            "formals",
            functions::function_parts,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "formals<-",
            functions::set_function_parts,
            Eval,
            Arity::Range(2, 3),
            On
        ),
        primitive!("body", functions::function_parts, Eval, Arity::Exact(1), On),
        primitive!(
            "body<-",
            functions::set_function_parts,
            Eval,
            Arity::Range(2, 3),
            On
        ),
        primitive!("args", functions::function_parts, Eval, Arity::Exact(1), On),
        primitive!("alist", values::alist, Unevaluated, Arity::Any, On),
        primitive!(
            "as.pairlist",
            values::as_pairlist,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "as.function",
            functions::as_function,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "call",
            values::construct_call,
            EvalKeepMissing,
            Arity::Any,
            On
        ),
        primitive!("as.call", values::as_call, Eval, Arity::Exact(1), On),
        primitive!(
            "bquote",
            quotation::bquote,
            Unevaluated,
            Arity::Range(1, 3),
            On
        ),
        primitive!(
            "match.arg",
            arguments::match_arg,
            Eval,
            Arity::Range(1, 3),
            On
        ),
        primitive!(
            "match.fun",
            functions::match_fun,
            Eval,
            Arity::Range(1, 2),
            On
        ),
        primitive!(
            "Recall",
            evaluation::recall,
            Unevaluated,
            Arity::Any,
            SetByFn
        ),
        primitive!("nargs", arguments::nargs, Eval, Arity::Exact(0), On),
        primitive!(
            "str2lang",
            source::str_to_language,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "str2expression",
            source::str_to_language,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "eval.parent",
            evaluation::eval_parent,
            Eval,
            Arity::Range(1, 2),
            SetByFn
        ),
        primitive!(
            "forceAndCall",
            evaluation::force_and_call,
            Unevaluated,
            Arity::Any,
            SetByFn
        ),
        primitive!(
            "switch",
            evaluation::switch,
            Unevaluated,
            Arity::Any,
            SetByFn
        ),
        primitive!(
            "is.primitive",
            values::language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "is.call",
            values::language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "is.name",
            values::language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "is.symbol",
            values::language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "is.pairlist",
            values::language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "is.expression",
            values::language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "is.language",
            values::language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "is.recursive",
            values::language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "is.object",
            values::language_predicate,
            Eval,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "function",
            functions::closure,
            Unevaluated,
            Arity::Range(2, 3),
            On
        ),
        primitive!("quote", quotation::quote, Unevaluated, Arity::Exact(1), On),
        primitive!(
            "missing",
            arguments::missing,
            Unevaluated,
            Arity::Exact(1),
            On
        ),
        primitive!(
            "substitute",
            quotation::substitute,
            Unevaluated,
            Arity::Range(0, 2),
            On
        ),
        primitive!(
            "match.call",
            arguments::match_call,
            Unevaluated,
            Arity::Range(0, 4),
            On
        ),
        primitive!(
            "...length",
            arguments::dots_length,
            Eval,
            Arity::Exact(0),
            On
        ),
        primitive!("...elt", arguments::dots_element, Eval, Arity::Exact(1), On),
        primitive!(
            "eval",
            evaluation::evaluate,
            Unevaluated,
            Arity::Range(1, 3),
            SetByFn
        ),
        primitive!(
            "evalq",
            evaluation::evaluate,
            Unevaluated,
            Arity::Range(1, 3),
            SetByFn
        ),
        primitive!(
            "local",
            evaluation::evaluate,
            Unevaluated,
            Arity::Range(1, 2),
            SetByFn
        ),
        primitive!(
            "withVisible",
            evaluation::with_visible,
            Unevaluated,
            Arity::Exact(1),
            On
        ),
        primitive!("parse", source::parse, Eval, Arity::Any, On),
        primitive!("deparse", source::deparse, Eval, Arity::Any, On),
        primitive!("deparse1", source::deparse, Eval, Arity::Any, On),
        primitive!(
            "do.call",
            evaluation::do_call,
            Eval,
            Arity::Range(2, 4),
            SetByFn
        ),
        primitive!(
            ".Internal",
            evaluation::internal,
            Unevaluated,
            Arity::Exact(1),
            SetByFn
        ),
    ],
    prelude: r#"
identity <- function(x) x
force <- function(x) x
Negate <- function(f) {
    f <- match.fun(f)
    function(...) !f(...)
}
Vectorize <- function(FUN, vectorize.args = arg.names, SIMPLIFY = TRUE, USE.NAMES = TRUE) {
    FUN <- match.fun(FUN)
    arg.names <- names(formals(FUN))
    if (is.null(arg.names)) stop("FUN must be a function with formal arguments")
    if (!is.character(vectorize.args) && !is.null(vectorize.args))
        stop("'vectorize.args' must be a character vector")
    if (length(vectorize.args) == 0L) return(FUN)
    includes <- function(x, choices) vapply(x, function(name) any(name == choices), FALSE)
    if (!all(includes(vectorize.args, arg.names)))
        stop("must specify formal argument names in 'vectorize.args'")
    if (any(includes(vectorize.args, c("FUN", "SIMPLIFY", "USE.NAMES", "MoreArgs"))))
        stop("'FUN', 'SIMPLIFY', 'USE.NAMES' and 'MoreArgs' cannot be vectorized")
    wrapped <- function() {
        supplied <- as.list(match.call(expand.dots = FALSE))[-1L]
        values <- lapply(supplied, eval, envir = parent.frame())
        varying <- includes(names(values), vectorize.args)
        do.call(mapply, c(list(FUN = FUN), values[varying],
                          list(MoreArgs = values[!varying], SIMPLIFY = SIMPLIFY,
                               USE.NAMES = USE.NAMES)), quote = TRUE)
    }
    formals(wrapped) <- formals(FUN)
    wrapped
}
ifelse <- function(test, yes, no) {
    if (is.null(attributes(test)) && length(test) == 1L) {
        if (is.na(test)) return(NA)
        if (test && length(yes) == 1L && is.null(attributes(yes))) return(yes)
        if (!test && length(no) == 1L && is.null(attributes(no))) return(no)
    }
    if (!is.logical(test)) {
        saved <- attributes(test)
        test <- as.logical(test)
        attributes(test) <- saved
    }
    result <- test
    indices <- seq_along(test)
    positive <- indices[!is.na(test) & test]
    negative <- indices[!is.na(test) & !test]
    if (length(positive)) result[positive] <- rep(yes, length.out = length(test))[positive]
    if (length(negative)) result[negative] <- rep(no, length.out = length(test))[negative]
    result
}
"#,
};

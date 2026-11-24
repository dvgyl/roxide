//! Arithmetic, logic, reductions, coercion, and attributes.

mod arithmetic;
pub(crate) mod attributes;
pub(crate) mod common;
mod summary;
pub(crate) mod types;

use roxide_eval::{ArgEval, Arity, Dispatch, GroupGeneric, Module, PrimDef, PrimKind, Visibility};

macro_rules! primitive {
    ($name:literal, $fun:path) => {
        primitive!($name, $fun, Eval, Any, Dispatch::None, On)
    };
    ($name:literal, $fun:path, $eval:ident, $arity:expr, $dispatch:expr, $visibility:ident) => {
        PrimDef {
            name: $name,
            kind: PrimKind::Primitive,
            args: ArgEval::$eval,
            arity: $arity,
            dispatch: $dispatch,
            visibility: Visibility::$visibility,
            fun: $fun,
        }
    };
}
macro_rules! group {
    ($name:literal, $fun:path, $arity:expr, $group:ident) => {
        primitive!(
            $name,
            $fun,
            Eval,
            $arity,
            Dispatch::Group(GroupGeneric::$group),
            On
        )
    };
}
pub(crate) use primitive;

use Arity::{Any, Exact, Range};

/// Base operators and value primitives.
pub const MODULE: Module = Module {
    prims: &[
        group!("+", arithmetic::add, Range(1, 2), Ops),
        group!("-", arithmetic::sub, Range(1, 2), Ops),
        group!("*", arithmetic::mul, Exact(2), Ops),
        group!("/", arithmetic::div, Exact(2), Ops),
        group!("^", arithmetic::pow, Exact(2), Ops),
        group!("%/%", arithmetic::idiv, Exact(2), Ops),
        group!("%%", arithmetic::rem, Exact(2), Ops),
        group!("==", arithmetic::eq, Exact(2), Ops),
        group!("!=", arithmetic::ne, Exact(2), Ops),
        group!("<", arithmetic::lt, Exact(2), Ops),
        group!("<=", arithmetic::le, Exact(2), Ops),
        group!(">", arithmetic::gt, Exact(2), Ops),
        group!(">=", arithmetic::ge, Exact(2), Ops),
        group!("&", arithmetic::and, Exact(2), Ops),
        group!("|", arithmetic::or, Exact(2), Ops),
        group!("!", arithmetic::not, Exact(1), Ops),
        primitive!("xor", arithmetic::xor, Eval, Exact(2), Dispatch::None, On),
        primitive!(
            "&&",
            arithmetic::short_and,
            Unevaluated,
            Exact(2),
            Dispatch::None,
            On
        ),
        primitive!(
            "||",
            arithmetic::short_or,
            Unevaluated,
            Exact(2),
            Dispatch::None,
            On
        ),
        primitive!(
            "%||%",
            arithmetic::coalesce,
            Unevaluated,
            Exact(2),
            Dispatch::None,
            On
        ),
        group!("Re", arithmetic::re, Exact(1), Complex),
        group!("Im", arithmetic::im, Exact(1), Complex),
        group!("Mod", arithmetic::modulus, Exact(1), Complex),
        group!("Arg", arithmetic::arg, Exact(1), Complex),
        group!("Conj", arithmetic::conj, Exact(1), Complex),
        group!("all", summary::all, Any, Summary),
        group!("any", summary::any, Any, Summary),
        group!("sum", summary::sum, Any, Summary),
        group!("prod", summary::prod, Any, Summary),
        group!("min", summary::min, Any, Summary),
        group!("max", summary::max, Any, Summary),
        group!("range", summary::range, Any, Summary),
        group!("cumsum", summary::cumsum, Exact(1), Math),
        group!("cumprod", summary::cumprod, Exact(1), Math),
        group!("cummin", summary::cummin, Exact(1), Math),
        group!("cummax", summary::cummax, Exact(1), Math),
        group!("abs", summary::abs, Exact(1), Math),
        group!("floor", summary::floor, Exact(1), Math),
        group!("ceiling", summary::ceiling, Exact(1), Math),
        group!("trunc", summary::trunc, Exact(1), Math),
        group!("round", summary::round, Range(1, 2), Math),
        group!("signif", summary::signif, Range(1, 2), Math),
        group!("sign", summary::sign, Exact(1), Math),
        group!("sqrt", summary::sqrt, Exact(1), Math),
        group!("exp", summary::exp, Exact(1), Math),
        group!("log", summary::log, Range(1, 2), Math),
        group!("sin", summary::sin, Exact(1), Math),
        group!("cos", summary::cos, Exact(1), Math),
        group!("tan", summary::tan, Exact(1), Math),
        primitive!(
            "is.na",
            types::is_na,
            Eval,
            Exact(1),
            Dispatch::Internal("is.na"),
            On
        ),
        primitive!(
            "anyNA",
            types::any_na,
            Eval,
            Range(1, 2),
            Dispatch::Internal("anyNA"),
            On
        ),
        primitive!("is.nan", types::is_nan),
        primitive!("is.finite", types::is_finite),
        primitive!("is.infinite", types::is_infinite),
        primitive!("is.null", types::is_null),
        primitive!("is.logical", types::is_logical),
        primitive!("is.integer", types::is_integer),
        primitive!("is.double", types::is_double),
        primitive!("is.numeric", types::is_numeric),
        primitive!("is.complex", types::is_complex),
        primitive!("is.character", types::is_character),
        primitive!("is.raw", types::is_raw),
        primitive!("is.list", types::is_list),
        primitive!("is.vector", types::is_vector),
        primitive!("is.atomic", types::is_atomic),
        primitive!("is.function", types::is_function),
        primitive!("is.environment", types::is_environment),
        primitive!("is.matrix", types::is_matrix),
        primitive!("is.array", types::is_array),
        primitive!("is.factor", types::is_factor),
        primitive!("is.data.frame", types::is_data_frame),
        primitive!("typeof", types::typeof_),
        primitive!("mode", types::mode),
        primitive!("storage.mode", types::storage_mode),
        primitive!("identical", types::identical),
        primitive!("logical", types::logical),
        primitive!("integer", types::integer),
        primitive!("numeric", types::double),
        primitive!("double", types::double),
        primitive!("character", types::character),
        primitive!("complex", types::complex),
        primitive!("raw", types::raw),
        primitive!(
            "as.logical",
            types::as_logical,
            Eval,
            Any,
            Dispatch::Internal("as.logical"),
            On
        ),
        primitive!(
            "as.integer",
            types::as_integer,
            Eval,
            Any,
            Dispatch::Internal("as.integer"),
            On
        ),
        primitive!(
            "as.double",
            types::as_double,
            Eval,
            Any,
            Dispatch::Internal("as.double"),
            On
        ),
        primitive!(
            "as.numeric",
            types::as_double,
            Eval,
            Any,
            Dispatch::Internal("as.double"),
            On
        ),
        primitive!(
            "as.complex",
            types::as_complex,
            Eval,
            Any,
            Dispatch::Internal("as.complex"),
            On
        ),
        primitive!(
            "as.character",
            types::as_character,
            Eval,
            Any,
            Dispatch::Internal("as.character"),
            On
        ),
        primitive!(
            "as.raw",
            types::as_raw,
            Eval,
            Any,
            Dispatch::Internal("as.raw"),
            On
        ),
        primitive!(
            "as.list",
            types::as_list,
            Eval,
            Any,
            Dispatch::Internal("as.list"),
            On
        ),
        primitive!(
            "as.vector",
            types::as_vector,
            Eval,
            Range(1, 2),
            Dispatch::Internal("as.vector"),
            On
        ),
        primitive!(
            "all.vars",
            types::all_vars,
            Eval,
            Range(1, 4),
            Dispatch::None,
            On
        ),
        primitive!(
            "environmentName",
            types::environment_name,
            Eval,
            Exact(1),
            Dispatch::None,
            On
        ),
        primitive!(
            "length<-",
            attributes::set_length,
            Eval,
            Exact(2),
            Dispatch::Internal("length<-"),
            On
        ),
        primitive!("attributes", attributes::attributes),
        primitive!("attributes<-", attributes::set_attributes),
        primitive!("attr", attributes::attr),
        primitive!("attr<-", attributes::set_attr),
        primitive!(
            "names",
            attributes::names,
            Eval,
            Exact(1),
            Dispatch::Internal("names"),
            On
        ),
        primitive!(
            "names<-",
            attributes::set_names,
            Eval,
            Exact(2),
            Dispatch::Internal("names<-"),
            On
        ),
        primitive!("class", attributes::class),
        primitive!("class<-", attributes::set_class),
        primitive!("oldClass", attributes::old_class),
        primitive!(
            "dim",
            attributes::dim,
            Eval,
            Exact(1),
            Dispatch::Internal("dim"),
            On
        ),
        primitive!(
            "dim<-",
            attributes::set_dim,
            Eval,
            Exact(2),
            Dispatch::Internal("dim<-"),
            On
        ),
        primitive!(
            "dimnames",
            attributes::dimnames,
            Eval,
            Exact(1),
            Dispatch::Internal("dimnames"),
            On
        ),
        primitive!(
            "dimnames<-",
            attributes::set_dimnames,
            Eval,
            Exact(2),
            Dispatch::Internal("dimnames<-"),
            On
        ),
        primitive!("rownames", attributes::rownames),
        primitive!("colnames", attributes::colnames),
        primitive!(
            "levels",
            attributes::levels,
            Eval,
            Exact(1),
            Dispatch::Internal("levels"),
            On
        ),
        primitive!(
            "levels<-",
            attributes::set_levels,
            Eval,
            Exact(2),
            Dispatch::Internal("levels<-"),
            On
        ),
        primitive!("structure", attributes::structure),
        primitive!("unname", attributes::unname),
        primitive!("unclass", attributes::unclass),
        primitive!("inherits", attributes::inherits),
        primitive!("c", types::combine, Eval, Any, Dispatch::Internal("c"), On),
    ],
    prelude: r#"
anyNA.data.frame <- function(x, recursive = FALSE)
    any(vapply(x, anyNA, FALSE))

`colnames<-` <- function(x, value) {
    if (is.data.frame(x)) {
        names(x) <- value
    } else {
        if (length(dim(x)) < 2L) stop("attempt to set 'colnames' on an object with less than two dimensions")
        dn <- dimnames(x)
        if (is.null(dn)) dn <- rep(list(NULL), length(dim(x)))
        dn[2L] <- list(value)
        dimnames(x) <- dn
    }
    x
}
Ops.factor <- function(e1, e2) {
    if (.Generic != "==" && .Generic != "!=") {
        warning(paste0("‘", .Generic, "’ not meaningful for factors"))
        len <- length(e1)
        if (!missing(e2)) len <- max(len, length(e2))
        return(rep(NA, len))
    }
    f1 <- is.factor(e1)
    f2 <- is.factor(e2)
    if (f1 && f2 && !identical(sort(levels(e1)), sort(levels(e2))))
        stop("level sets of factors are different")
    convert <- function(x) {
        y <- as.character(x)
        if (!is.null(dim(x))) {
            dim(y) <- dim(x)
            dimnames(y) <- dimnames(x)
        } else names(y) <- names(x)
        y
    }
    if (f1) e1 <- convert(e1)
    if (f2) e2 <- convert(e2)
    if (.Generic == "==") e1 == e2 else e1 != e2
}
as.vector.factor <- function(x, mode = "any") {
    if (mode == "any" || mode == "character") as.character(x)
    else as.vector(unclass(x), mode)
}
"#,
};

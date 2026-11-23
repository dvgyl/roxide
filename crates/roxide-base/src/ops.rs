//! Arithmetic, logic, reductions, coercion, and attributes.

mod arithmetic;
pub(crate) mod attributes;
pub(crate) mod common;
mod strings;
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
        group!("sign", summary::sign, Exact(1), Math),
        group!("sqrt", summary::sqrt, Exact(1), Math),
        group!("exp", summary::exp, Exact(1), Math),
        group!("log", summary::log, Range(1, 2), Math),
        group!("sin", summary::sin, Exact(1), Math),
        group!("cos", summary::cos, Exact(1), Math),
        group!("tan", summary::tan, Exact(1), Math),
        primitive!("is.na", types::is_na),
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
        primitive!("as.logical", types::as_logical),
        primitive!("as.integer", types::as_integer),
        primitive!("as.double", types::as_double),
        primitive!("as.numeric", types::as_double),
        primitive!("as.complex", types::as_complex),
        primitive!("as.character", types::as_character),
        primitive!("as.raw", types::as_raw),
        primitive!("as.list", types::as_list),
        primitive!("attributes", attributes::attributes),
        primitive!("attributes<-", attributes::set_attributes),
        primitive!("attr", attributes::attr),
        primitive!("attr<-", attributes::set_attr),
        primitive!("names", attributes::names),
        primitive!("names<-", attributes::set_names),
        primitive!("class", attributes::class),
        primitive!("class<-", attributes::set_class),
        primitive!("oldClass", attributes::old_class),
        primitive!("dim", attributes::dim),
        primitive!("dim<-", attributes::set_dim),
        primitive!("dimnames", attributes::dimnames),
        primitive!("dimnames<-", attributes::set_dimnames),
        primitive!("rownames", attributes::rownames),
        primitive!("colnames", attributes::colnames),
        primitive!("levels", attributes::levels),
        primitive!("levels<-", attributes::set_levels),
        primitive!("structure", attributes::structure),
        primitive!("unname", attributes::unname),
        primitive!("unclass", attributes::unclass),
        primitive!("inherits", attributes::inherits),
        primitive!("c", types::combine),
        primitive!("paste", strings::paste),
        primitive!("paste0", strings::paste0),
        primitive!("nchar", strings::nchar),
    ],
    prelude: "",
};

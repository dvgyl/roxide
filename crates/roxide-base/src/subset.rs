//! Subsetting, containers, sequences, and higher-order functions.

mod apply;
mod containers;
mod index;
mod printing;
mod replacement;
mod sequence;

use roxide_eval::{ArgEval, Arity, Dispatch, Module, PrimDef, PrimKind, Visibility};

use crate::ops::primitive;
use Arity::{Any, Exact};

/// Base container, subsetting, sequence, and apply primitives.
pub const MODULE: Module = Module {
    prims: &[
        primitive!(
            "[",
            index::subset,
            EvalKeepMissing,
            Any,
            Dispatch::Internal("["),
            On
        ),
        primitive!(
            "[[",
            index::extract,
            EvalKeepMissing,
            Any,
            Dispatch::Internal("[["),
            On
        ),
        primitive!(
            "$",
            index::dollar,
            EvalDollar,
            Exact(2),
            Dispatch::Internal("$"),
            On
        ),
        primitive!(
            "[<-",
            replacement::replace,
            EvalKeepMissing,
            Any,
            Dispatch::Internal("[<-"),
            On
        ),
        primitive!(
            "[[<-",
            replacement::replace_one,
            EvalKeepMissing,
            Any,
            Dispatch::Internal("[[<-"),
            On
        ),
        primitive!(
            "$<-",
            replacement::replace_dollar,
            EvalDollar,
            Exact(3),
            Dispatch::Internal("$<-"),
            On
        ),
        primitive!("list", containers::list),
        primitive!(
            "pairlist",
            containers::pairlist,
            EvalKeepMissing,
            Any,
            Dispatch::None,
            On
        ),
        primitive!(":", sequence::colon, Eval, Exact(2), Dispatch::None, On),
        primitive!("seq", sequence::seq),
        primitive!("seq.int", sequence::seq),
        primitive!("seq_along", sequence::seq_along),
        primitive!("seq_len", sequence::seq_len),
        primitive!("rep", sequence::rep),
        primitive!("rev", sequence::rev),
        primitive!("sort", sequence::sort),
        primitive!("order", sequence::order),
        primitive!("head", sequence::head),
        primitive!("tail", sequence::tail),
        primitive!("rle", containers::rle),
        primitive!("inverse.rle", containers::inverse_rle),
        primitive!("print", printing::print, Eval, Any, Dispatch::None, Off),
        primitive!("matrix", containers::matrix),
        primitive!("rbind", containers::rbind),
        primitive!("cbind", containers::cbind),
        primitive!("nrow", containers::nrow),
        primitive!("ncol", containers::ncol),
        primitive!("factor", containers::factor),
        primitive!("ordered", containers::ordered),
        primitive!("data.frame", containers::data_frame),
        primitive!("lapply", apply::lapply),
        primitive!("sapply", apply::sapply),
        primitive!("vapply", apply::vapply),
        primitive!("Reduce", apply::reduce),
        primitive!("Filter", apply::filter),
        primitive!("do.call", apply::do_call),
    ],
    prelude: "",
};

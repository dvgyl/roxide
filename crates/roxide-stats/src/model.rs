//! Statistical formulas, model frames, and design matrices.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod formula;
mod frame;
mod matrix;
mod poly;

use roxide_eval::{ArgEval, Arity, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility};
use roxide_gc::SexpId;
use roxide_value::{Encoding, RSession, SEXP_NIL, ValueApi};

use crate::distn::support::internal;

/// Registration table for statistical model functions and methods.
pub const MODULE: Module = Module {
    prims: &[
        PrimDef {
            name: "~",
            kind: PrimKind::Primitive,
            args: ArgEval::Unevaluated,
            arity: Arity::Range(1, 2),
            dispatch: Dispatch::None,
            visibility: Visibility::On,
            fun: formula::tilde,
        },
        internal("C_formula_call", 2, formula::as_formula),
        internal("C_formula_names", 2, formula::from_names),
        internal("C_termsform", 6, formula::terms),
        internal("C_update_formula", 2, formula::update),
        internal("C_delete_response", 1, formula::delete_response),
        internal("C_drop_terms", 3, formula::drop_terms),
        internal("C_model_labels", 1, formula::labels),
        internal("C_model_symbols", 1, formula::all_symbols),
        internal("C_model_call", 2, frame::model_call),
        internal("C_model_match", 2, frame::match_names),
        internal("C_model_unique_rows", 1, frame::unique_rows),
        internal("C_model_distinct", 1, poly::distinct),
        internal("C_complete_cases", 1, frame::complete_cases),
        internal("C_model_rownames", 1, frame::row_names),
        internal("C_model_predvar", 2, frame::predvar),
        internal("C_modelmatrix", 5, matrix::model_matrix),
        internal("C_model_poly", 5, poly::poly),
    ],
    prelude: concat!(
        include_str!("model/formula.R"),
        include_str!("model/frame.R"),
        include_str!("model/missing.R"),
        include_str!("model/poly.R"),
        include_str!("model/matrix.R"),
    ),
};

fn strings<'a>(s: &mut RSession, values: impl IntoIterator<Item = &'a str>) -> EvalResult {
    let values = values
        .into_iter()
        .map(|value| s.mk_char(value.as_bytes(), Encoding::Utf8))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(s.alloc_str_from(&values)?)
}

fn list(s: &mut RSession, values: &[SexpId]) -> EvalResult {
    let result = s.alloc_list(values.len())?;
    for (i, &value) in values.iter().enumerate() {
        s.list_set(result, i, value)?;
    }
    Ok(result)
}

fn call(
    s: &mut RSession,
    head: SexpId,
    args: impl DoubleEndedIterator<Item = (SexpId, SexpId)>,
) -> EvalResult {
    let mut tail = SEXP_NIL;
    for (tag, value) in args.rev() {
        tail = s.cons(value, tail, tag)?;
    }
    Ok(s.lcons(head, tail, SEXP_NIL)?)
}

fn lang(s: &mut RSession, name: &str, args: &[SexpId]) -> EvalResult {
    let head = s.intern(name)?.as_sexp();
    call(s, head, args.iter().map(|&value| (SEXP_NIL, value)))
}

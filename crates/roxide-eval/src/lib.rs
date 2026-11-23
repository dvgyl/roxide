#![forbid(unsafe_code)]

//! Clean-room GNU R 4.6.2 evaluator and worker extension boundary (`Ctx`, `Module`, `PrimDef`).
//!
//! Downstream package crates (`roxide-base`, `roxide-stats`, etc.) depend on [`api`]
//! and never access private [`Ctx`] fields directly (ADR-0007).

pub mod api;
pub mod builtins;
pub mod ctx;
pub mod eval;
pub mod testing;

pub use api::{
    ArgEval, Arity, Call, CtxExt, Dispatch, EvalResult, GroupGeneric, InternalError, Module,
    OptionsCache, PrimDef, PrimFn, PrimKind, S4Hooks, Transfer, Unwind, Visibility,
};
pub use ctx::{Args, Ctx, FrameId};

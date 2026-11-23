#![forbid(unsafe_code)]

//! Clean-room GNU R 4.6.2 lexer, Pratt parser, AST, and operator precedence table.
//!
//! This crate has no heap dependency: it produces a compact flat [`Ast`] that
//! lowers 1:1 into `roxide-value` language objects (`LANGSXP`/`SYMSXP`/literals).

pub mod ast;
pub mod lexer;
mod literal;
pub mod ops;
pub mod parser;

pub use ast::{Arg, ArgName, Ast, Const, Node, NodeId, StrEncoding};
pub use lexer::{LexError, Lexer, TokKind, Token};
pub use parser::{ParseError, ParseStep, ParseWarning, Parser};

#[cfg(test)]
mod tests;

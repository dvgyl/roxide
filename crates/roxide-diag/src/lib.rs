#![forbid(unsafe_code)]

//! Source locations, `srcref` coordinates, and GNU R 4.6.2 diagnostic formatting.
//!
//! This is the leaf crate in the Roxide workspace dependency graph and holds no
//! heap references or OS handles.

pub mod format;
pub mod msg;
pub mod source;

pub use source::{LineDirective, SourceMap, Span, SrcFile, SrcFileId};

/// Broad condition category corresponding to R's built-in condition hierarchy.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ConditionKind {
    /// Fatal or catchable error condition (`simpleError`, `error`, `condition`).
    Error,
    /// Warning condition (`simpleWarning`, `warning`, `condition`).
    Warning,
    /// Diagnostic message (`simpleMessage`, `message`, `condition`).
    Message,
    /// User/host interrupt (`interrupt`, `condition`).
    Interrupt,
}

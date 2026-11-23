//! `NAMESPACE` directive parser and evaluator (M3 W1-C).

/// Parsed `NAMESPACE` specification produced by evaluating directives as R expressions.
#[derive(Clone, Debug, Default)]
pub struct NsSpec {
    /// Explicit exported object names.
    pub exports: Vec<String>,
    /// Regular expressions from `exportPattern()`.
    pub export_patterns: Vec<String>,
}

//! `DESCRIPTION` file parsing and metadata validation (reusing `roxide-base::dcf`).

/// Parsed package `DESCRIPTION` metadata (populated in M3 W1-C).
#[derive(Clone, Debug, Default)]
pub struct Description {
    /// Package name (`Package:`).
    pub package: String,
    /// Package version (`Version:`).
    pub version: String,
    /// Optional `Collate:` file order.
    pub collate: Vec<String>,
}

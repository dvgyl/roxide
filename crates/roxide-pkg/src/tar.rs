//! Clean-room POSIX `ustar` archive reader over `flate2` (M3 W2-C).

/// Placeholder ustar entry descriptor (implemented in M3 W2-C).
#[derive(Clone, Debug)]
pub struct TarEntry {
    /// Entry relative path inside the archive.
    pub path: String,
    /// Uncompressed payload bytes.
    pub data: Vec<u8>,
}

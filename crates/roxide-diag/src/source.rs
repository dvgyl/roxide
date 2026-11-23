//! Source files, half-open byte spans, `#line` directives, and R `srcref` 8-tuples.

use std::sync::Arc;

/// Identifier for a source file stored in a [`SourceMap`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
pub struct SrcFileId(pub u32);

/// Half-open byte span `[lo, hi)` inside a [`SrcFile`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Span {
    /// Source file containing this span.
    pub file: SrcFileId,
    /// Inclusive start byte offset.
    pub lo: u32,
    /// Exclusive end byte offset.
    pub hi: u32,
}

impl Span {
    /// Construct a new half-open span.
    #[must_use]
    pub const fn new(file: SrcFileId, lo: u32, hi: u32) -> Self {
        Self { file, lo, hi }
    }

    /// Return the smallest span enclosing both `self` and `other` (same file).
    #[must_use]
    pub const fn merge(self, other: Self) -> Self {
        let lo = if self.lo < other.lo {
            self.lo
        } else {
            other.lo
        };
        let hi = if self.hi > other.hi {
            self.hi
        } else {
            other.hi
        };
        Self {
            file: self.file,
            lo,
            hi,
        }
    }
}

/// A `#line <line> ["filename"]` directive parsed from R source.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LineDirective {
    /// 1-based physical line number on which the directive appeared.
    pub physical_line: u32,
    /// 1-based logical line number assigned to the following line.
    pub logical_line: u32,
    /// Optional logical filename override.
    pub filename: Option<String>,
}

/// One registered source text unit with precomputed line-start byte offsets.
#[derive(Clone, Debug)]
pub struct SrcFile {
    /// Display name or path for diagnostics (`<text>`, script path, etc.).
    pub name: String,
    /// Full UTF-8 source text.
    pub text: Arc<str>,
    line_starts: Vec<u32>,
    line_map: Vec<LineDirective>,
}

impl SrcFile {
    /// Create a new [`SrcFile`] and index its newline byte positions.
    #[must_use]
    pub fn new(name: impl Into<String>, text: impl Into<Arc<str>>) -> Self {
        let text: Arc<str> = text.into();
        let mut line_starts = vec![0u32];
        for (idx, byte) in text.as_bytes().iter().enumerate() {
            if *byte == b'\n'
                && let Ok(next) = u32::try_from(idx + 1)
            {
                line_starts.push(next);
            }
        }
        Self {
            name: name.into(),
            text,
            line_starts,
            line_map: Vec::new(),
        }
    }

    /// Record a parsed `#line` directive in physical line order.
    pub fn push_line_directive(&mut self, directive: LineDirective) {
        self.line_map.push(directive);
    }

    /// Return `(1-based physical line, 1-based byte column, 1-based char column)` for a byte offset.
    #[must_use]
    pub fn line_col(&self, byte_offset: u32) -> (u32, u32, u32) {
        let clamped = (byte_offset as usize).min(self.text.len());
        let line_idx = match self.line_starts.binary_search(&(clamped as u32)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx] as usize;
        let prefix = self.text.get(line_start..clamped).unwrap_or("");
        let byte_col = (clamped.saturating_sub(line_start) + 1) as u32;
        let char_col = (prefix.chars().count() + 1) as u32;
        ((line_idx + 1) as u32, byte_col, char_col)
    }

    /// Map a 1-based physical line to its logical line via active `#line` directives.
    #[must_use]
    pub fn logical_line(&self, physical_line: u32) -> u32 {
        let mut active: Option<&LineDirective> = None;
        for dir in &self.line_map {
            if dir.physical_line < physical_line {
                active = Some(dir);
            } else {
                break;
            }
        }
        match active {
            Some(dir) => dir
                .logical_line
                .saturating_add(physical_line.saturating_sub(dir.physical_line + 1)),
            None => physical_line,
        }
    }

    /// Compute R's standard 8-integer `srcref` vector for `[lo, hi)`:
    /// `[first_line, first_byte, last_line, last_byte, first_col, last_col, first_parsed, last_parsed]`.
    #[must_use]
    pub fn srcref8(&self, lo: u32, hi: u32) -> [i32; 8] {
        let (first_phys, first_byte, first_col) = self.line_col(lo);
        let end_inclusive = if hi > lo { hi - 1 } else { lo };
        let (last_phys, last_byte, last_col) = self.line_col(end_inclusive);
        let first_log = self.logical_line(first_phys);
        let last_log = self.logical_line(last_phys);
        [
            first_log as i32,
            first_byte as i32,
            last_log as i32,
            last_byte as i32,
            first_col as i32,
            last_col as i32,
            first_phys as i32,
            last_phys as i32,
        ]
    }
}

/// Collection of source files registered during a session.
#[derive(Default, Debug)]
pub struct SourceMap {
    files: Vec<SrcFile>,
}

impl SourceMap {
    /// Create an empty [`SourceMap`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new source file and return its [`SrcFileId`].
    pub fn add_file(&mut self, name: impl Into<String>, text: impl Into<Arc<str>>) -> SrcFileId {
        let id = SrcFileId(self.files.len() as u32);
        self.files.push(SrcFile::new(name, text));
        id
    }

    /// Look up a source file by [`SrcFileId`].
    #[must_use]
    pub fn get(&self, id: SrcFileId) -> Option<&SrcFile> {
        self.files.get(id.0 as usize)
    }

    /// Mutable lookup for attaching `#line` directives.
    #[must_use]
    pub fn get_mut(&mut self, id: SrcFileId) -> Option<&mut SrcFile> {
        self.files.get_mut(id.0 as usize)
    }

    /// Compute the 8-element R `srcref` vector for a [`Span`].
    #[must_use]
    pub fn srcref8(&self, span: Span) -> [i32; 8] {
        self.get(span.file)
            .map_or([1, 1, 1, 1, 1, 1, 1, 1], |f| f.srcref8(span.lo, span.hi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_srcref8_and_line_directives() {
        let mut map = SourceMap::new();
        let fid = map.add_file("<text>", "a <- 1\nb <- 2\n");
        assert_eq!(map.srcref8(Span::new(fid, 0, 6)), [1, 1, 1, 6, 1, 6, 1, 1]);
        assert_eq!(map.srcref8(Span::new(fid, 7, 13)), [2, 1, 2, 6, 1, 6, 2, 2]);
        if let Some(f) = map.get_mut(fid) {
            f.push_line_directive(LineDirective {
                physical_line: 1,
                logical_line: 100,
                filename: None,
            });
        }
        assert_eq!(
            map.srcref8(Span::new(fid, 7, 13)),
            [100, 1, 100, 6, 1, 6, 2, 2]
        );
    }
}

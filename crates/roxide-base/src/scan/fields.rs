use std::borrow::Cow;

use super::input::{ConnectionLines, InputOptions, input_text};

use roxide_eval::{Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::SEXP_NIL;

/// Delimiters and escape rules for a scan.
pub struct Dialect {
    /// Field separator. Use whitespace when absent.
    pub separator: Option<char>,
    /// Characters that can start a quoted field.
    pub quotes: String,
    /// Character that discards the rest of an unquoted line.
    pub comment: Option<char>,
    /// Interpret backslash escape sequences.
    pub escapes: bool,
    /// Omit records with no fields.
    pub skip_blank: bool,
}

/// One logical record, which can cover multiple physical lines.
pub struct Record {
    /// Fields in input order.
    pub fields: Vec<String>,
    /// Offset of the first physical line, starting at one.
    pub start_line: usize,
    /// Offset of the last physical line, starting at one.
    pub end_line: usize,
    /// The record consumed its final line break or reached end of input.
    pub line_ended: bool,
    /// The stream ended before the closing quote.
    pub unclosed_quote: bool,
}

/// Iterator over the fields in a text input.
pub struct Fields<'a> {
    input: Cow<'a, str>,
    offset: usize,
    source: Option<ConnectionLines<'a>>,
    previewing: bool,
    stream_bytes: usize,
    escaped_delimiter: Option<char>,
    scan_line: usize,
    dialect: &'a Dialect,
    line: usize,
    pending_empty: bool,
}

impl<'a> Fields<'a> {
    /// Start scanning a text input with normalized newlines.
    pub fn new(input: &'a str, dialect: &'a Dialect) -> Self {
        Self {
            input: Cow::Borrowed(input),
            offset: 0,
            source: None,
            previewing: false,
            stream_bytes: 0,
            escaped_delimiter: None,
            scan_line: 1,
            dialect,
            line: 1,
            pending_empty: false,
        }
    }

    /// Open a text input. Read from an open connection one line at a time.
    pub fn open(
        ctx: &mut Ctx,
        call: &Call,
        file: SexpId,
        text: SexpId,
        options: InputOptions<'a>,
        dialect: &'a Dialect,
    ) -> EvalResult<Self> {
        if text == SEXP_NIL
            && let Some(source) = ConnectionLines::open(ctx, call, file, options)?
        {
            let mut current = source.take_remainder(ctx);
            for _ in 0..options.skip {
                if current.is_empty() && source.next(ctx, call)?.is_none() {
                    break;
                }
                current.clear();
            }
            return Ok(Self::stream(current, source, dialect));
        }
        let input = input_text(ctx, call, file, text, options)?;
        let mut fields = Self::new("", dialect);
        fields.input = Cow::Owned(input);
        Ok(fields)
    }

    /// Retain the input needed to replay the table header and first records.
    pub fn start_preview(&mut self) -> Position {
        self.previewing = true;
        self.position()
    }

    /// Record the current position during preview.
    pub fn position(&self) -> Position {
        Position {
            offset: self.offset,
            line: self.line,
            scan_line: self.scan_line,
            pending_empty: self.pending_empty,
            escaped_delimiter: self.escaped_delimiter,
        }
    }

    /// End preview and resume at a saved position in this input.
    pub fn end_preview(&mut self, pos: Position) {
        self.offset = pos.offset.min(self.input.len());
        self.line = pos.line;
        self.scan_line = pos.scan_line;
        self.pending_empty = pos.pending_empty;
        self.escaped_delimiter = pos.escaped_delimiter;
        self.previewing = false;
    }

    pub(super) fn stream(input: String, source: ConnectionLines<'a>, dialect: &'a Dialect) -> Self {
        Self {
            input: Cow::Owned(input),
            offset: 0,
            source: Some(source),
            previewing: false,
            stream_bytes: 0,
            escaped_delimiter: None,
            scan_line: 1,
            dialect,
            line: 1,
            pending_empty: false,
        }
    }

    /// Save buffered input for the next scan on the same open connection.
    pub fn finish(self, ctx: &mut Ctx) {
        if let Some(source) = &self.source {
            source.save_remainder(ctx, self.remaining());
        }
    }

    /// Return the input that is already buffered at the current position.
    pub fn remaining(&self) -> &str {
        &self.input[self.offset..]
    }

    fn refill(&mut self, ctx: &mut Ctx, call: &Call) -> EvalResult<bool> {
        if self.offset < self.input.len() {
            return Ok(true);
        }
        if let Some(source) = &self.source {
            if let Some(line) = source.next(ctx, call)? {
                self.stream_bytes = self.stream_bytes.saturating_add(line.len());
                if self.stream_bytes > ctx.host().limits().max_vector_bytes {
                    return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
                }
                if self.previewing {
                    self.input.to_mut().push_str(&line);
                } else {
                    self.input = Cow::Owned(line);
                    self.offset = 0;
                }
                return Ok(true);
            }
            self.source = None;
        }
        Ok(false)
    }

    pub(super) fn scan_line(&self) -> usize {
        self.scan_line
    }

    fn peek(&self) -> Option<char> {
        self.escaped_delimiter
            .or_else(|| self.remaining().chars().next())
    }

    fn take(&mut self) -> Option<char> {
        let ch = self.peek()?;
        if self.escaped_delimiter.take().is_none() {
            self.offset += ch.len_utf8();
            self.line += usize::from(ch == '\n');
        }
        Some(ch)
    }

    fn discard_comment(&mut self) {
        if self.peek().is_some_and(|c| Some(c) == self.dialect.comment) {
            self.offset += self
                .remaining()
                .find('\n')
                .unwrap_or(self.remaining().len());
        }
    }

    /// Discard the rest of the current physical line.
    pub fn flush_line(&mut self) {
        self.pending_empty = false;
        while let Some(c) = self.take() {
            if c == '\n' {
                self.scan_line += 1;
                break;
            }
        }
    }

    /// Read a record with per-column character and whitespace settings.
    ///
    /// A positive field limit leaves the remaining fields available for the next scan.
    pub fn record(
        &mut self,
        ctx: &mut Ctx,
        call: &Call,
        limit: usize,
        mode: impl FnMut(usize) -> (bool, bool),
    ) -> EvalResult<Option<Record>> {
        self.record_until(ctx, call, limit, 0, mode)
    }

    pub(super) fn record_until(
        &mut self,
        ctx: &mut Ctx,
        call: &Call,
        limit: usize,
        nlines: usize,
        mut mode: impl FnMut(usize) -> (bool, bool),
    ) -> EvalResult<Option<Record>> {
        let bytes = ctx.host().limits().max_vector_bytes;
        loop {
            if nlines != 0 && self.scan_line > nlines {
                return Ok(None);
            }
            if !self.pending_empty && !self.refill(ctx, call)? {
                return Ok(None);
            }
            let start_line = self.line;
            let mut fields = Vec::new();
            let mut field_bytes = 0usize;
            let mut after_sep = std::mem::take(&mut self.pending_empty);
            let mut unclosed_quote = false;
            while fields.len() < limit {
                let (quoted, strip) = mode(fields.len());
                let numeric_blank = self.dialect.skip_blank
                    && !quoted
                    && !after_sep
                    && fields.is_empty()
                    && self
                        .remaining()
                        .split_once('\n')
                        .is_some_and(|(line, _)| line.chars().all(horizontal));
                if self.dialect.separator.is_none() || numeric_blank {
                    self.trim_horizontal();
                }
                self.discard_comment();
                if !after_sep && matches!(self.peek(), Some('\n') | None) {
                    break;
                }
                let (value, unclosed) = self.field(ctx, call, quoted, strip)?;
                unclosed_quote |= unclosed;
                field_bytes = field_bytes.saturating_add(value.len() + size_of::<String>());
                if field_bytes > bytes {
                    return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
                }
                fields.push(value);
                self.discard_comment();
                after_sep = self
                    .peek()
                    .is_some_and(|c| Some(c) == self.dialect.separator && c != '\n');
                if after_sep {
                    self.take();
                }
            }
            let end_line = self.line;
            if self.dialect.separator.is_none() {
                self.trim_horizontal();
                self.discard_comment();
            }
            let line_ended = !after_sep && matches!(self.peek(), Some('\n') | None);
            if line_ended && self.peek() == Some('\n') {
                self.take();
                self.scan_line += 1;
            }
            self.pending_empty = after_sep;
            if !fields.is_empty() || !self.dialect.skip_blank {
                return Ok(Some(Record {
                    fields,
                    start_line,
                    end_line,
                    line_ended,
                    unclosed_quote,
                }));
            }
        }
    }

    fn trim_horizontal(&mut self) {
        if self.escaped_delimiter.is_some_and(horizontal) {
            self.escaped_delimiter = None;
        }
        let rest = self.remaining();
        self.offset += rest.len() - rest.trim_start_matches(horizontal).len();
    }

    fn field(
        &mut self,
        ctx: &mut Ctx,
        call: &Call,
        quoted: bool,
        strip: bool,
    ) -> EvalResult<(String, bool)> {
        let separated = self.dialect.separator.is_some();
        let mut value = String::new();
        let mut keep = 0;
        while let Some(ch) = self.peek() {
            if ch == '\n' || Some(ch) == self.dialect.separator || Some(ch) == self.dialect.comment
            {
                break;
            }
            if !separated && horizontal(ch) {
                break;
            }
            self.take();
            if quoted && self.dialect.quotes.contains(ch) && (separated || value.is_empty()) {
                if !self.quoted(ctx, call, ch, &mut value)? {
                    return Ok((value, true));
                }
                keep = value.len();
                if !separated {
                    break;
                }
            } else if ch == '\\' && self.dialect.escapes {
                let decoded = self.escape();
                if matches!(decoded, '\n' | '\r')
                    || Some(decoded) == self.dialect.separator
                    || (!separated && horizontal(decoded))
                {
                    if value.is_empty() && !separated && horizontal(decoded) {
                        continue;
                    }
                    self.escaped_delimiter = Some(if decoded == '\r' { '\n' } else { decoded });
                    break;
                }
                if !horizontal(decoded) || quoted && (!strip || !value.is_empty()) {
                    value.push(decoded);
                    if !horizontal(decoded) {
                        keep = value.len();
                    }
                }
            } else if !horizontal(ch) {
                value.push(ch);
                keep = value.len();
            } else if quoted && (!strip || !value.is_empty()) {
                value.push(ch);
            }
        }
        if strip {
            value.truncate(keep);
        }
        Ok((value, false))
    }

    fn quoted(
        &mut self,
        ctx: &mut Ctx,
        call: &Call,
        quote: char,
        value: &mut String,
    ) -> EvalResult<bool> {
        let bound = ctx.host().limits().max_vector_bytes;
        while self.refill(ctx, call)? {
            let Some(ch) = self.take() else { break };
            if value.len().saturating_add(ch.len_utf8()) > bound {
                return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
            }
            if ch == quote {
                if self.dialect.separator.is_some() && self.peek() == Some(quote) {
                    self.take();
                    value.push(quote);
                } else {
                    return Ok(true);
                }
            } else if ch == '\\' && self.dialect.separator.is_none() && self.peek() == Some(quote) {
                self.take();
                value.push(quote);
            } else if ch == '\\' && self.dialect.escapes {
                value.push(self.escape());
            } else {
                value.push(ch);
            }
        }
        Ok(false)
    }

    fn escape(&mut self) -> char {
        match self.take() {
            Some('a') => '\x07',
            Some('b') => '\x08',
            Some('f') => '\x0c',
            Some('n') => '\n',
            Some('r') => '\r',
            Some('t') => '\t',
            Some('v') => '\x0b',
            Some('x') => char::from(self.escape_digits(16, 0, 2)),
            Some(c @ '0'..='7') => char::from(self.escape_digits(8, c as u8 - b'0', 2)),
            Some(c) => c,
            None => '\\',
        }
    }

    fn escape_digits(&mut self, radix: u32, mut value: u8, count: usize) -> u8 {
        for _ in 0..count {
            let Some(digit) = self.peek().and_then(|ch| ch.to_digit(radix)) else {
                break;
            };
            self.take();
            value = value.wrapping_mul(radix as u8).wrapping_add(digit as u8);
        }
        value
    }
}

/// Saved position within a table header preview.
pub struct Position {
    offset: usize,
    line: usize,
    scan_line: usize,
    pending_empty: bool,
    escaped_delimiter: Option<char>,
}

fn horizontal(ch: char) -> bool {
    matches!(ch, ' ' | '\t')
}

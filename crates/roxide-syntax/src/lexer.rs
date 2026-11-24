//! Clean-room GNU R 4.6.2 lexer.

use std::fmt;

use roxide_diag::{Span, SrcFileId};

/// Token kind produced by [`Lexer`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TokKind {
    /// Numeric double literal (including hex float `0x1p4`).
    Num,
    /// Integer literal with `L` suffix (`1L`, `0x10L`).
    Int,
    /// Imaginary literal with `i` suffix (`2i`, `0x1p2i`).
    Complex,
    /// Quoted or raw string literal.
    Str {
        /// True for raw string literals `r"(...)"` / `R"--[...]--"`.
        raw: bool,
    },
    /// Bare identifier symbol (`x`, `.foo`, `T`, `F`).
    Symbol,
    /// Backtick-quoted symbol (`` `foo bar` ``).
    Backtick,
    /// `NULL`
    Null,
    /// `NA`
    Na,
    /// `NA_integer_`
    NaInteger,
    /// `NA_real_`
    NaReal,
    /// `NA_complex_`
    NaComplex,
    /// `NA_character_`
    NaCharacter,
    /// `TRUE`
    True,
    /// `FALSE`
    False,
    /// `Inf`
    Inf,
    /// `NaN`
    NaN,
    /// `function`
    Function,
    /// `\` shorthand lambda.
    Lambda,
    /// `if`
    If,
    /// `else`
    Else,
    /// `for`
    For,
    /// `in`
    In,
    /// `while`
    While,
    /// `repeat`
    Repeat,
    /// `break`
    Break,
    /// `next`
    Next,
    /// `|>` native pipe.
    Pipe,
    /// `_` pipe placeholder.
    Placeholder,
    /// `%...%` special infix operator (including `%||%`, `%*%`, `%%`, `%/%`).
    Special,
    /// `<-`
    LeftAssign,
    /// `<<-`
    SuperLeftAssign,
    /// `->`
    RightAssign,
    /// `->>`
    SuperRightAssign,
    /// `:=`
    ColonAssign,
    /// `=`
    Eq,
    /// `==`
    EqEq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `^` or `**`
    Caret,
    /// `:`
    Colon,
    /// `::`
    NsGet,
    /// `:::`
    NsGetInt,
    /// `~`
    Tilde,
    /// `?`
    Question,
    /// `!`
    Bang,
    /// `&`
    And,
    /// `&&`
    AndAnd,
    /// `|`
    Or,
    /// `||`
    OrOr,
    /// `$`
    Dollar,
    /// `@`
    At,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `[[`
    LBracket2,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `,`
    Comma,
    /// `;`
    Semi,
    /// `# ...` comment (excluding trailing newline).
    Comment,
    /// `\n` or `\r\n`.
    Newline,
    /// End of input.
    Eof,
}

/// A single lexed token with its source [`Span`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Token {
    /// Kind of token.
    pub kind: TokKind,
    /// Byte span into the source text.
    pub span: Span,
}

/// Lexical error.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LexError {
    /// Human-readable message.
    pub message: String,
    /// Error span.
    pub span: Span,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LexError {}

/// Zero-copy R 4.6.2 lexer over UTF-8 source text.
pub struct Lexer<'s> {
    src: &'s str,
    bytes: &'s [u8],
    file: SrcFileId,
    pos: usize,
}

impl<'s> Lexer<'s> {
    /// Create a new lexer over `src`.
    #[must_use]
    pub fn new(src: &'s str, file: SrcFileId) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            file,
            pos: 0,
        }
    }

    /// Tokenize the entire source into a [`Vec<Token>`] ending with [`TokKind::Eof`].
    pub fn tokenize_all(mut self) -> Result<Vec<Token>, LexError> {
        let mut out = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.kind == TokKind::Eof;
            out.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(out)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn span(&self, lo: usize, hi: usize) -> Span {
        Span::new(self.file, lo as u32, hi as u32)
    }

    /// Scan and return the next [`Token`].
    pub fn next_token(&mut self) -> Result<Token, LexError> {
        if self.src.len() > u32::MAX as usize {
            return Err(LexError {
                message: String::from("source exceeds the byte limit"),
                span: self.span(0, 0),
            });
        }
        while let Some(ch) = self.src[self.pos..].chars().next()
            && matches!(ch, ' ' | '\t' | '\x0c' | '\u{1680}' | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200a}' | '\u{205f}' | '\u{3000}')
        {
            self.pos += ch.len_utf8();
        }

        let start = self.pos;
        let Some(b) = self.peek() else {
            return Ok(Token {
                kind: TokKind::Eof,
                span: self.span(start, start),
            });
        };

        if b == b'\r' {
            self.pos += 1;
            if self.peek() == Some(b'\n') {
                self.pos += 1;
            }
            return Ok(Token {
                kind: TokKind::Newline,
                span: self.span(start, self.pos),
            });
        }
        if b == b'\n' {
            self.pos += 1;
            return Ok(Token {
                kind: TokKind::Newline,
                span: self.span(start, self.pos),
            });
        }

        if b == b'#' {
            while let Some(c) = self.peek() {
                if c == b'\n' || c == b'\r' {
                    break;
                }
                self.pos += 1;
            }
            return Ok(Token {
                kind: TokKind::Comment,
                span: self.span(start, self.pos),
            });
        }

        if matches!(b, b'r' | b'R') && matches!(self.peek_at(1), Some(b'"' | b'\'')) {
            return self.lex_raw_string(start);
        }

        if b == b'"' || b == b'\'' {
            return self.lex_string(start, b);
        }

        if b == b'`' {
            return self.lex_backtick(start);
        }

        if b.is_ascii_digit() || (b == b'.' && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()))
        {
            return self.lex_number(start);
        }

        if b == b'_' {
            self.pos += 1;
            return Ok(Token {
                kind: TokKind::Placeholder,
                span: self.span(start, self.pos),
            });
        }

        if b == b'.' || self.src[start..].starts_with(char::is_alphabetic) {
            return Ok(self.lex_ident_or_keyword(start));
        }

        if b == b'%' {
            return self.lex_special(start);
        }

        self.pos += 1;
        let kind = match b {
            b'(' => TokKind::LParen,
            b')' => TokKind::RParen,
            b'{' => TokKind::LBrace,
            b'}' => TokKind::RBrace,
            b']' => TokKind::RBracket,
            b',' => TokKind::Comma,
            b';' => TokKind::Semi,
            b'+' => TokKind::Plus,
            b'/' => TokKind::Slash,
            b'^' => TokKind::Caret,
            b'~' => TokKind::Tilde,
            b'?' => TokKind::Question,
            b'$' => TokKind::Dollar,
            b'@' => TokKind::At,
            b'\\' => TokKind::Lambda,
            b'[' => {
                if self.peek() == Some(b'[') {
                    self.pos += 1;
                    TokKind::LBracket2
                } else {
                    TokKind::LBracket
                }
            }
            b'*' => {
                if self.peek() == Some(b'*') {
                    self.pos += 1;
                    TokKind::Caret
                } else {
                    TokKind::Star
                }
            }
            b'-' => {
                if self.peek() == Some(b'>') {
                    self.pos += 1;
                    if self.peek() == Some(b'>') {
                        self.pos += 1;
                        TokKind::SuperRightAssign
                    } else {
                        TokKind::RightAssign
                    }
                } else {
                    TokKind::Minus
                }
            }
            b'<' => match self.peek() {
                Some(b'-') => {
                    self.pos += 1;
                    TokKind::LeftAssign
                }
                Some(b'<') if self.peek_at(1) == Some(b'-') => {
                    self.pos += 2;
                    TokKind::SuperLeftAssign
                }
                Some(b'=') => {
                    self.pos += 1;
                    TokKind::Le
                }
                _ => TokKind::Lt,
            },
            b'>' => {
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    TokKind::Ge
                } else {
                    TokKind::Gt
                }
            }
            b'=' => {
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    TokKind::EqEq
                } else {
                    TokKind::Eq
                }
            }
            b'!' => {
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    TokKind::Ne
                } else {
                    TokKind::Bang
                }
            }
            b'&' => {
                if self.peek() == Some(b'&') {
                    self.pos += 1;
                    TokKind::AndAnd
                } else {
                    TokKind::And
                }
            }
            b'|' => match self.peek() {
                Some(b'>') => {
                    self.pos += 1;
                    TokKind::Pipe
                }
                Some(b'|') => {
                    self.pos += 1;
                    TokKind::OrOr
                }
                _ => TokKind::Or,
            },
            b':' => match self.peek() {
                Some(b':') => {
                    self.pos += 1;
                    if self.peek() == Some(b':') {
                        self.pos += 1;
                        TokKind::NsGetInt
                    } else {
                        TokKind::NsGet
                    }
                }
                Some(b'=') => {
                    self.pos += 1;
                    TokKind::ColonAssign
                }
                _ => TokKind::Colon,
            },
            _ => {
                let ch = self.src[start..]
                    .chars()
                    .next()
                    .unwrap_or(char::REPLACEMENT_CHARACTER);
                self.pos = start + ch.len_utf8();
                return Err(LexError {
                    message: format!("unexpected input '{ch}'"),
                    span: self.span(start, self.pos),
                });
            }
        };

        Ok(Token {
            kind,
            span: self.span(start, self.pos),
        })
    }

    fn lex_string(&mut self, start: usize, quote: u8) -> Result<Token, LexError> {
        self.pos += 1; // opening quote
        while let Some(b) = self.peek() {
            if b == b'\\' {
                self.pos += 1;
                if self.pos < self.bytes.len() {
                    self.pos += 1;
                }
            } else if b == quote {
                self.pos += 1;
                return Ok(Token {
                    kind: TokKind::Str { raw: false },
                    span: self.span(start, self.pos),
                });
            } else {
                self.pos += 1;
            }
        }
        Err(LexError {
            message: String::from("unterminated string literal"),
            span: self.span(start, self.pos),
        })
    }

    fn lex_raw_string(&mut self, start: usize) -> Result<Token, LexError> {
        self.pos += 1; // r or R
        let quote = self.bytes[self.pos];
        self.pos += 1;
        let dash_start = self.pos;
        while self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let dashes = self.pos - dash_start;
        let close_delim = match self.peek() {
            Some(b'(') => b')',
            Some(b'[') => b']',
            Some(b'{') => b'}',
            _ => {
                return Err(LexError {
                    message: String::from("malformed raw string literal"),
                    span: self.span(start, self.pos),
                });
            }
        };
        self.pos += 1;
        while let Some(b) = self.peek() {
            if b == close_delim {
                let mut matched = true;
                for i in 0..dashes {
                    if self.peek_at(1 + i) != Some(b'-') {
                        matched = false;
                        break;
                    }
                }
                if matched && self.peek_at(1 + dashes) == Some(quote) {
                    self.pos += 2 + dashes;
                    return Ok(Token {
                        kind: TokKind::Str { raw: true },
                        span: self.span(start, self.pos),
                    });
                }
            }
            self.pos += 1;
        }
        Err(LexError {
            message: String::from("unterminated raw string literal"),
            span: self.span(start, self.pos),
        })
    }

    fn lex_backtick(&mut self, start: usize) -> Result<Token, LexError> {
        self.pos += 1;
        while let Some(b) = self.peek() {
            if b == b'\\' {
                self.pos += 1;
                if self.pos < self.bytes.len() {
                    self.pos += 1;
                }
            } else if b == b'`' {
                self.pos += 1;
                return Ok(Token {
                    kind: TokKind::Backtick,
                    span: self.span(start, self.pos),
                });
            } else {
                self.pos += 1;
            }
        }
        Err(LexError {
            message: String::from("unterminated backtick symbol"),
            span: self.span(start, self.pos),
        })
    }

    fn lex_special(&mut self, start: usize) -> Result<Token, LexError> {
        self.pos += 1; // opening %
        while let Some(b) = self.peek() {
            if b == b'%' {
                self.pos += 1;
                return Ok(Token {
                    kind: TokKind::Special,
                    span: self.span(start, self.pos),
                });
            }
            if b == b'\n' || b == b'\r' {
                break;
            }
            self.pos += 1;
        }
        Err(LexError {
            message: String::from("unterminated special infix operator"),
            span: self.span(start, self.pos),
        })
    }

    fn lex_number(&mut self, start: usize) -> Result<Token, LexError> {
        if self.peek() == Some(b'0') && matches!(self.peek_at(1), Some(b'x' | b'X')) {
            self.pos += 2;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            if self.peek() == Some(b'.') {
                self.pos += 1;
                while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    self.pos += 1;
                }
            }
            if matches!(self.peek(), Some(b'p' | b'P')) {
                self.pos += 1;
                if matches!(self.peek(), Some(b'+' | b'-')) {
                    self.pos += 1;
                }
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
        } else {
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.peek() == Some(b'.') {
                self.pos += 1;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
            if matches!(self.peek(), Some(b'e' | b'E')) {
                self.pos += 1;
                if matches!(self.peek(), Some(b'+' | b'-')) {
                    self.pos += 1;
                }
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
        }

        let kind = match self.peek() {
            Some(b'L') => {
                self.pos += 1;
                TokKind::Int
            }
            Some(b'i') => {
                self.pos += 1;
                TokKind::Complex
            }
            _ => TokKind::Num,
        };
        Ok(Token {
            kind,
            span: self.span(start, self.pos),
        })
    }

    fn lex_ident_or_keyword(&mut self, start: usize) -> Token {
        while let Some(ch) = self.src[self.pos..].chars().next() {
            if ch.is_alphanumeric() || ch == '.' || ch == '_' {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];
        let kind = match text {
            "NULL" => TokKind::Null,
            "NA" => TokKind::Na,
            "NA_integer_" => TokKind::NaInteger,
            "NA_real_" => TokKind::NaReal,
            "NA_complex_" => TokKind::NaComplex,
            "NA_character_" => TokKind::NaCharacter,
            "TRUE" => TokKind::True,
            "FALSE" => TokKind::False,
            "Inf" => TokKind::Inf,
            "NaN" => TokKind::NaN,
            "function" => TokKind::Function,
            "if" => TokKind::If,
            "else" => TokKind::Else,
            "for" => TokKind::For,
            "in" => TokKind::In,
            "while" => TokKind::While,
            "repeat" => TokKind::Repeat,
            "break" => TokKind::Break,
            "next" => TokKind::Next,
            _ => TokKind::Symbol,
        };
        Token {
            kind,
            span: self.span(start, self.pos),
        }
    }
}

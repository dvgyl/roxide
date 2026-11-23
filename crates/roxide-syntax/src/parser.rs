//! Clean-room Pratt parser for GNU R 4.6.2.

use std::fmt;

use roxide_diag::{Span, SrcFileId};

use crate::ast::{Arg, ArgName, Ast, Const, Node, NodeId, StrEncoding};
use crate::lexer::{Lexer, TokKind, Token};
use crate::ops::{self, Assoc, Prec};

/// Non-fatal parser warning (e.g. non-integer value with `L` suffix).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ParseWarning {
    /// Warning message.
    pub message: String,
    /// Source span.
    pub span: Span,
}

/// Fatal syntax error returned by [`Parser`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ParseError {
    /// Error message.
    pub message: String,
    /// Offending source span.
    pub span: Span,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ParseError {}

/// Incremental top-level parse result returned by [`Parser::parse_next`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ParseStep {
    /// Parsed one complete top-level expression.
    Expr(NodeId),
    /// Input ended mid-expression (REPL continuation prompt `+ `).
    Incomplete,
    /// End of input reached cleanly.
    Eof,
    /// Syntax error.
    Error(ParseError),
}

/// R 4.6.2 Pratt parser producing an [`Ast`].
pub struct Parser<'s> {
    src: &'s str,
    tokens: Vec<Token>,
    cursor: usize,
    ast: Ast,
    warnings: Vec<ParseWarning>,
    lex_error: Option<ParseError>,
}

impl<'s> Parser<'s> {
    /// Lex and initialize a [`Parser`] over `src`.
    #[must_use]
    pub fn new(src: &'s str, file: SrcFileId) -> Self {
        match Lexer::new(src, file).tokenize_all() {
            Ok(tokens) => Self {
                src,
                tokens,
                cursor: 0,
                ast: Ast::new(),
                warnings: Vec::new(),
                lex_error: None,
            },
            Err(err) => Self {
                src,
                tokens: Vec::new(),
                cursor: 0,
                ast: Ast::new(),
                warnings: Vec::new(),
                lex_error: Some(ParseError {
                    message: err.message,
                    span: err.span,
                }),
            },
        }
    }

    /// Borrow the underlying [`Ast`].
    #[must_use]
    pub fn ast(&self) -> &Ast {
        &self.ast
    }

    /// Consume the parser and return its [`Ast`].
    #[must_use]
    pub fn into_ast(self) -> Ast {
        self.ast
    }

    /// Borrow accumulated parse warnings.
    #[must_use]
    pub fn warnings(&self) -> &[ParseWarning] {
        &self.warnings
    }

    /// Borrow all lexed tokens (for `getParseData`).
    #[must_use]
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// Parse all top-level expressions until EOF.
    pub fn parse_all(&mut self) -> Result<Vec<NodeId>, ParseError> {
        let mut exprs = Vec::new();
        loop {
            match self.parse_next() {
                ParseStep::Expr(id) => exprs.push(id),
                ParseStep::Eof => return Ok(exprs),
                ParseStep::Incomplete => {
                    return Err(ParseError {
                        message: String::from("unexpected end of input"),
                        span: self.current_span(),
                    });
                }
                ParseStep::Error(err) => return Err(err),
            }
        }
    }

    /// Parse the next single top-level expression.
    pub fn parse_next(&mut self) -> ParseStep {
        if let Some(err) = self.lex_error.take() {
            return ParseStep::Error(err);
        }
        self.skip_trivia_and_separators();
        if self.is_eof() {
            return ParseStep::Eof;
        }
        match self.parse_expr(Prec(0), false) {
            Ok(expr) => {
                // Consume optional trailing `;` or newline separator.
                self.skip_comments();
                if matches!(self.peek_kind(), TokKind::Semi | TokKind::Newline) {
                    self.advance();
                } else if !self.is_eof() {
                    return ParseStep::Error(ParseError {
                        message: format!("unexpected '{}'", self.current_text()),
                        span: self.current_span(),
                    });
                }
                ParseStep::Expr(expr)
            }
            Err(err) => ParseStep::Error(err),
        }
    }

    fn slice(&self, span: Span) -> &'s str {
        &self.src[span.lo as usize..span.hi as usize]
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.cursor)
            .unwrap_or_else(|| &self.tokens[self.tokens.len() - 1])
    }

    fn peek_kind(&self) -> &TokKind {
        &self.peek().kind
    }

    fn current_span(&self) -> Span {
        if self.tokens.is_empty() {
            Span::default()
        } else {
            self.peek().span
        }
    }

    fn current_text(&self) -> &'s str {
        if self.tokens.is_empty() {
            ""
        } else {
            self.slice(self.peek().span)
        }
    }

    fn is_eof(&self) -> bool {
        self.tokens.is_empty() || self.peek_kind() == &TokKind::Eof
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if self.cursor + 1 < self.tokens.len() {
            self.cursor += 1;
        }
        tok
    }

    fn skip_comments(&mut self) {
        while self.peek_kind() == &TokKind::Comment {
            self.advance();
        }
    }

    fn skip_newlines_and_comments(&mut self) {
        while matches!(self.peek_kind(), TokKind::Comment | TokKind::Newline) {
            self.advance();
        }
    }

    fn skip_trivia_and_separators(&mut self) {
        while matches!(
            self.peek_kind(),
            TokKind::Comment | TokKind::Newline | TokKind::Semi
        ) {
            self.advance();
        }
    }

    /// Parse an expression with minimum binding power `min_prec`.
    /// When `ignore_newlines` is true (inside `(...)` or `[...]`), newlines do not terminate expressions.
    fn parse_expr(&mut self, min_prec: Prec, ignore_newlines: bool) -> Result<NodeId, ParseError> {
        if ignore_newlines {
            self.skip_newlines_and_comments();
        } else {
            self.skip_comments();
        }

        let mut lhs = self.parse_prefix(ignore_newlines)?;

        loop {
            if ignore_newlines {
                self.skip_newlines_and_comments();
            } else {
                self.skip_comments();
            }

            // Postfix calls and indexing bind tighter than binary ops (except `::`/`:::`, `$`/`@`).
            match self.peek_kind() {
                TokKind::LParen => {
                    lhs = self.parse_call(lhs)?;
                    continue;
                }
                TokKind::LBracket => {
                    lhs = self.parse_subset(lhs, false)?;
                    continue;
                }
                TokKind::LBracket2 => {
                    lhs = self.parse_subset(lhs, true)?;
                    continue;
                }
                _ => {}
            }

            let Some((op_str, prec, assoc)) = self.peek_binary_op() else {
                break;
            };
            if prec < min_prec {
                break;
            }

            let op_tok = self.advance();
            let op_text = op_str.to_owned();
            // After any binary operator, newlines are always skipped before the RHS.
            self.skip_newlines_and_comments();

            let next_min = match assoc {
                Assoc::Left => Prec(prec.0 + 1),
                Assoc::Right => prec,
                Assoc::NonAssoc => Prec(prec.0 + 1),
            };
            let rhs = self.parse_expr(next_min, ignore_newlines)?;
            let span = self.ast.span(lhs).merge(self.ast.span(rhs));

            lhs = match op_text.as_str() {
                "->" => self.make_binary_call("<-", op_tok.span, rhs, lhs, span),
                "->>" => self.make_binary_call("<<-", op_tok.span, rhs, lhs, span),
                "**" => self.make_binary_call("^", op_tok.span, lhs, rhs, span),
                "|>" => self.desugar_pipe(lhs, rhs, span)?,
                _ => self.make_binary_call(&op_text, op_tok.span, lhs, rhs, span),
            };
        }

        Ok(lhs)
    }

    fn parse_prefix(&mut self, ignore_newlines: bool) -> Result<NodeId, ParseError> {
        let tok = self.peek().clone();
        match tok.kind {
            TokKind::Null => {
                self.advance();
                Ok(self.ast.push_node(Node::Const(Const::Null), tok.span))
            }
            TokKind::True => {
                self.advance();
                Ok(self
                    .ast
                    .push_node(Node::Const(Const::Lgl(Some(true))), tok.span))
            }
            TokKind::False => {
                self.advance();
                Ok(self
                    .ast
                    .push_node(Node::Const(Const::Lgl(Some(false))), tok.span))
            }
            TokKind::Na => {
                self.advance();
                Ok(self.ast.push_node(Node::Const(Const::Lgl(None)), tok.span))
            }
            TokKind::NaInteger => {
                self.advance();
                Ok(self
                    .ast
                    .push_node(Node::Const(Const::Int(i32::MIN)), tok.span))
            }
            TokKind::NaReal => {
                self.advance();
                Ok(self
                    .ast
                    .push_node(Node::Const(Const::Real(0x7FF0_0000_0000_07A2)), tok.span))
            }
            TokKind::NaComplex => {
                self.advance();
                Ok(self.ast.push_node(
                    Node::Const(Const::Complex {
                        re_bits: 0x7FF0_0000_0000_07A2,
                        im_bits: 0x7FF0_0000_0000_07A2,
                    }),
                    tok.span,
                ))
            }
            TokKind::NaCharacter => {
                self.advance();
                Ok(self.ast.push_node(Node::Const(Const::NaChar), tok.span))
            }
            TokKind::Inf => {
                self.advance();
                Ok(self
                    .ast
                    .push_node(Node::Const(Const::Real(f64::INFINITY.to_bits())), tok.span))
            }
            TokKind::NaN => {
                self.advance();
                Ok(self
                    .ast
                    .push_node(Node::Const(Const::Real(0x7FF8_0000_0000_0000)), tok.span))
            }
            TokKind::Num => {
                self.advance();
                let val = parse_r_double(self.slice(tok.span)).ok_or_else(|| ParseError {
                    message: format!("invalid numeric constant '{}'", self.slice(tok.span)),
                    span: tok.span,
                })?;
                Ok(self
                    .ast
                    .push_node(Node::Const(Const::Real(val.to_bits())), tok.span))
            }
            TokKind::Int => {
                self.advance();
                let raw = self.slice(tok.span);
                let num_str = raw.strip_suffix('L').unwrap_or(raw);
                let node = parse_r_int(num_str, tok.span, &mut self.warnings)?;
                Ok(self.ast.push_node(node, tok.span))
            }
            TokKind::Complex => {
                self.advance();
                let raw = self.slice(tok.span);
                let num_str = raw.strip_suffix('i').unwrap_or(raw);
                let im = parse_r_double(num_str).ok_or_else(|| ParseError {
                    message: format!("invalid complex constant '{raw}'"),
                    span: tok.span,
                })?;
                Ok(self.ast.push_node(
                    Node::Const(Const::Complex {
                        re_bits: 0.0f64.to_bits(),
                        im_bits: im.to_bits(),
                    }),
                    tok.span,
                ))
            }
            TokKind::Str { raw } => {
                self.advance();
                let (bytes, enc) = decode_string(self.slice(tok.span), raw, tok.span)?;
                Ok(self
                    .ast
                    .push_node(Node::Const(Const::Str { bytes, enc }), tok.span))
            }
            TokKind::Symbol | TokKind::Placeholder => {
                self.advance();
                let name: Box<str> = self.slice(tok.span).into();
                Ok(self.ast.push_node(Node::Sym(name), tok.span))
            }
            TokKind::Backtick => {
                self.advance();
                let raw = self.slice(tok.span);
                let inner = &raw[1..raw.len() - 1];
                let (bytes, _) = decode_escaped(inner, tok.span)?;
                let name = String::from_utf8_lossy(&bytes)
                    .into_owned()
                    .into_boxed_str();
                Ok(self.ast.push_node(Node::Sym(name), tok.span))
            }
            TokKind::Plus | TokKind::Minus | TokKind::Bang | TokKind::Tilde | TokKind::Question => {
                self.advance();
                let op_str = self.slice(tok.span);
                let prec = ops::unary(op_str).unwrap_or(ops::prec::UNARY);
                self.skip_newlines_and_comments();
                let rhs = self.parse_expr(prec, ignore_newlines)?;
                let span = tok.span.merge(self.ast.span(rhs));
                let func = self.ast.push_node(Node::Sym(op_str.into()), tok.span);
                let args = self.ast.push_args([Arg {
                    name: None,
                    value: Some(rhs),
                    span: self.ast.span(rhs),
                }]);
                Ok(self.ast.push_node(Node::Call { func, args }, span))
            }
            TokKind::LParen => {
                let open = self.advance();
                self.skip_newlines_and_comments();
                let inner = self.parse_expr(Prec(0), true)?;
                self.skip_newlines_and_comments();
                if self.peek_kind() != &TokKind::RParen {
                    return Err(ParseError {
                        message: String::from("expected ')'"),
                        span: self.current_span(),
                    });
                }
                let close = self.advance();
                let span = open.span.merge(close.span);
                let func = self.ast.push_node(Node::Sym("(".into()), open.span);
                let args = self.ast.push_args([Arg {
                    name: None,
                    value: Some(inner),
                    span: self.ast.span(inner),
                }]);
                Ok(self.ast.push_node(Node::Call { func, args }, span))
            }
            TokKind::LBrace => self.parse_brace_block(),
            TokKind::Function | TokKind::Lambda => self.parse_function(ignore_newlines),
            TokKind::If => self.parse_if(ignore_newlines),
            TokKind::While => self.parse_while(ignore_newlines),
            TokKind::Repeat => self.parse_repeat(ignore_newlines),
            TokKind::For => self.parse_for(ignore_newlines),
            TokKind::Break | TokKind::Next => {
                self.advance();
                let name = self.slice(tok.span);
                let func = self.ast.push_node(Node::Sym(name.into()), tok.span);
                let args = self.ast.push_args([]);
                Ok(self.ast.push_node(Node::Call { func, args }, tok.span))
            }
            _ => Err(ParseError {
                message: format!("unexpected '{}'", self.current_text()),
                span: tok.span,
            }),
        }
    }

    fn parse_brace_block(&mut self) -> Result<NodeId, ParseError> {
        let open = self.advance();
        let mut stmts = Vec::new();
        loop {
            self.skip_trivia_and_separators();
            if self.peek_kind() == &TokKind::RBrace {
                let close = self.advance();
                let span = open.span.merge(close.span);
                let func = self.ast.push_node(Node::Sym("{".into()), open.span);
                let args = self.ast.push_args(stmts);
                return Ok(self.ast.push_node(Node::Call { func, args }, span));
            }
            if self.is_eof() {
                return Err(ParseError {
                    message: String::from("unexpected end of input"),
                    span: self.current_span(),
                });
            }
            let stmt = self.parse_expr(Prec(0), false)?;
            let stmt_span = self.ast.span(stmt);
            stmts.push(Arg {
                name: None,
                value: Some(stmt),
                span: stmt_span,
            });
            self.skip_comments();
            if matches!(self.peek_kind(), TokKind::Semi | TokKind::Newline) {
                self.advance();
            } else if self.peek_kind() != &TokKind::RBrace {
                return Err(ParseError {
                    message: format!("unexpected '{}'", self.current_text()),
                    span: self.current_span(),
                });
            }
        }
    }

    fn parse_function(&mut self, ignore_newlines: bool) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::LParen {
            return Err(ParseError {
                message: String::from("expected '(' after function"),
                span: self.current_span(),
            });
        }
        self.advance();
        let mut formals = Vec::new();
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::RParen {
            loop {
                self.skip_newlines_and_comments();
                let name_tok = self.advance();
                let name_str: Box<str> = match name_tok.kind {
                    TokKind::Symbol => self.slice(name_tok.span).into(),
                    TokKind::Backtick => {
                        let raw = self.slice(name_tok.span);
                        raw[1..raw.len() - 1].into()
                    }
                    _ => {
                        return Err(ParseError {
                            message: String::from("expected formal parameter name"),
                            span: name_tok.span,
                        });
                    }
                };
                self.skip_newlines_and_comments();
                let default = if self.peek_kind() == &TokKind::Eq {
                    self.advance();
                    self.skip_newlines_and_comments();
                    Some(self.parse_expr(Prec(0), true)?)
                } else {
                    None
                };
                let fspan =
                    default.map_or(name_tok.span, |d| name_tok.span.merge(self.ast.span(d)));
                formals.push(Arg {
                    name: Some(ArgName::Sym(name_str)),
                    value: default,
                    span: fspan,
                });
                self.skip_newlines_and_comments();
                if self.peek_kind() == &TokKind::Comma {
                    self.advance();
                    self.skip_newlines_and_comments();
                    if self.peek_kind() == &TokKind::RParen {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        if self.peek_kind() != &TokKind::RParen {
            return Err(ParseError {
                message: String::from("expected ')' in function formals"),
                span: self.current_span(),
            });
        }
        self.advance();
        self.skip_newlines_and_comments();
        let body = self.parse_expr(Prec(0), ignore_newlines)?;
        let span = kw.span.merge(self.ast.span(body));
        let formals_range = self.ast.push_args(formals);
        Ok(self.ast.push_node(
            Node::Function {
                formals: formals_range,
                body,
                span,
            },
            span,
        ))
    }

    fn parse_if(&mut self, ignore_newlines: bool) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::LParen {
            return Err(ParseError {
                message: String::from("expected '(' after 'if'"),
                span: self.current_span(),
            });
        }
        self.advance();
        let cond = self.parse_expr(Prec(0), true)?;
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::RParen {
            return Err(ParseError {
                message: String::from("expected ')' after 'if' condition"),
                span: self.current_span(),
            });
        }
        self.advance();
        self.skip_newlines_and_comments();
        let then_branch = self.parse_expr(Prec(0), ignore_newlines)?;

        let mut args_vec = vec![
            Arg {
                name: None,
                value: Some(cond),
                span: self.ast.span(cond),
            },
            Arg {
                name: None,
                value: Some(then_branch),
                span: self.ast.span(then_branch),
            },
        ];

        let saved = self.cursor;
        if ignore_newlines {
            self.skip_newlines_and_comments();
        } else {
            self.skip_comments();
        }
        let end_span = if self.peek_kind() == &TokKind::Else {
            self.advance();
            self.skip_newlines_and_comments();
            let else_branch = self.parse_expr(Prec(0), ignore_newlines)?;
            let es = self.ast.span(else_branch);
            args_vec.push(Arg {
                name: None,
                value: Some(else_branch),
                span: es,
            });
            es
        } else {
            self.cursor = saved;
            self.ast.span(then_branch)
        };

        let span = kw.span.merge(end_span);
        let func = self.ast.push_node(Node::Sym("if".into()), kw.span);
        let args = self.ast.push_args(args_vec);
        Ok(self.ast.push_node(Node::Call { func, args }, span))
    }

    fn parse_while(&mut self, ignore_newlines: bool) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::LParen {
            return Err(ParseError {
                message: String::from("expected '(' after 'while'"),
                span: self.current_span(),
            });
        }
        self.advance();
        let cond = self.parse_expr(Prec(0), true)?;
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::RParen {
            return Err(ParseError {
                message: String::from("expected ')' after 'while' condition"),
                span: self.current_span(),
            });
        }
        self.advance();
        self.skip_newlines_and_comments();
        let body = self.parse_expr(Prec(0), ignore_newlines)?;
        let span = kw.span.merge(self.ast.span(body));
        let func = self.ast.push_node(Node::Sym("while".into()), kw.span);
        let args = self.ast.push_args([
            Arg {
                name: None,
                value: Some(cond),
                span: self.ast.span(cond),
            },
            Arg {
                name: None,
                value: Some(body),
                span: self.ast.span(body),
            },
        ]);
        Ok(self.ast.push_node(Node::Call { func, args }, span))
    }

    fn parse_repeat(&mut self, ignore_newlines: bool) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        self.skip_newlines_and_comments();
        let body = self.parse_expr(Prec(0), ignore_newlines)?;
        let span = kw.span.merge(self.ast.span(body));
        let func = self.ast.push_node(Node::Sym("repeat".into()), kw.span);
        let args = self.ast.push_args([Arg {
            name: None,
            value: Some(body),
            span: self.ast.span(body),
        }]);
        Ok(self.ast.push_node(Node::Call { func, args }, span))
    }

    fn parse_for(&mut self, ignore_newlines: bool) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::LParen {
            return Err(ParseError {
                message: String::from("expected '(' after 'for'"),
                span: self.current_span(),
            });
        }
        self.advance();
        self.skip_newlines_and_comments();
        let var_tok = self.advance();
        let var_name: Box<str> = match var_tok.kind {
            TokKind::Symbol => self.slice(var_tok.span).into(),
            TokKind::Backtick => {
                let raw = self.slice(var_tok.span);
                raw[1..raw.len() - 1].into()
            }
            _ => {
                return Err(ParseError {
                    message: String::from("expected loop variable name"),
                    span: var_tok.span,
                });
            }
        };
        let var_node = self.ast.push_node(Node::Sym(var_name), var_tok.span);
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::In {
            return Err(ParseError {
                message: String::from("expected 'in' in 'for' loop"),
                span: self.current_span(),
            });
        }
        self.advance();
        self.skip_newlines_and_comments();
        let seq = self.parse_expr(Prec(0), true)?;
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::RParen {
            return Err(ParseError {
                message: String::from("expected ')' in 'for' loop"),
                span: self.current_span(),
            });
        }
        self.advance();
        self.skip_newlines_and_comments();
        let body = self.parse_expr(Prec(0), ignore_newlines)?;
        let span = kw.span.merge(self.ast.span(body));
        let func = self.ast.push_node(Node::Sym("for".into()), kw.span);
        let args = self.ast.push_args([
            Arg {
                name: None,
                value: Some(var_node),
                span: var_tok.span,
            },
            Arg {
                name: None,
                value: Some(seq),
                span: self.ast.span(seq),
            },
            Arg {
                name: None,
                value: Some(body),
                span: self.ast.span(body),
            },
        ]);
        Ok(self.ast.push_node(Node::Call { func, args }, span))
    }

    fn parse_call(&mut self, callee: NodeId) -> Result<NodeId, ParseError> {
        let _open = self.advance(); // '('
        let (args_vec, close_span) = self.parse_arg_list(TokKind::RParen)?;
        let span = self.ast.span(callee).merge(close_span);
        let args = self.ast.push_args(args_vec);
        Ok(self.ast.push_node(Node::Call { func: callee, args }, span))
    }

    fn parse_subset(&mut self, target: NodeId, double: bool) -> Result<NodeId, ParseError> {
        let open = self.advance();
        let (mut args_vec, close_span) = self.parse_arg_list(TokKind::RBracket)?;
        let end_span = if double {
            self.skip_newlines_and_comments();
            if self.peek_kind() != &TokKind::RBracket {
                return Err(ParseError {
                    message: String::from("expected ']]'"),
                    span: self.current_span(),
                });
            }
            self.advance().span
        } else {
            close_span
        };
        let mut all_args = Vec::with_capacity(args_vec.len() + 1);
        all_args.push(Arg {
            name: None,
            value: Some(target),
            span: self.ast.span(target),
        });
        all_args.append(&mut args_vec);
        let op = if double { "[[" } else { "[" };
        let func = self.ast.push_node(Node::Sym(op.into()), open.span);
        let span = self.ast.span(target).merge(end_span);
        let args = self.ast.push_args(all_args);
        Ok(self.ast.push_node(Node::Call { func, args }, span))
    }

    fn parse_arg_list(&mut self, closing: TokKind) -> Result<(Vec<Arg>, Span), ParseError> {
        let mut args = Vec::new();
        self.skip_newlines_and_comments();
        if self.peek_kind() == &closing {
            let close = self.advance();
            return Ok((args, close.span));
        }
        loop {
            self.skip_newlines_and_comments();
            if self.peek_kind() == &closing {
                let close = self.advance();
                args.push(Arg {
                    name: None,
                    value: None,
                    span: close.span,
                });
                return Ok((args, close.span));
            }
            if self.peek_kind() == &TokKind::Comma {
                let comma = self.advance();
                args.push(Arg {
                    name: None,
                    value: None,
                    span: comma.span,
                });
                continue;
            }
            // Check named argument `<sym|str|NULL> = [expr]`
            let named = self.try_parse_arg_name()?;
            if let Some((arg_name, name_span)) = named {
                self.skip_newlines_and_comments();
                let value = if self.peek_kind() == &TokKind::Comma || self.peek_kind() == &closing {
                    None
                } else {
                    Some(self.parse_expr(Prec(0), true)?)
                };
                let span = value.map_or(name_span, |v| name_span.merge(self.ast.span(v)));
                args.push(Arg {
                    name: Some(arg_name),
                    value,
                    span,
                });
            } else {
                let expr = self.parse_expr(Prec(0), true)?;
                args.push(Arg {
                    name: None,
                    value: Some(expr),
                    span: self.ast.span(expr),
                });
            }
            self.skip_newlines_and_comments();
            if self.peek_kind() == &TokKind::Comma {
                self.advance();
                continue;
            }
            if self.peek_kind() == &closing {
                let close = self.advance();
                return Ok((args, close.span));
            }
            return Err(ParseError {
                message: format!("unexpected '{}'", self.current_text()),
                span: self.current_span(),
            });
        }
    }

    fn try_parse_arg_name(&mut self) -> Result<Option<(ArgName, Span)>, ParseError> {
        let saved = self.cursor;
        let first = self.peek().clone();
        let candidate = match first.kind {
            TokKind::Symbol => Some(ArgName::Sym(self.slice(first.span).into())),
            TokKind::Backtick => {
                let raw = self.slice(first.span);
                Some(ArgName::Sym(raw[1..raw.len() - 1].into()))
            }
            TokKind::Null => Some(ArgName::Null),
            TokKind::Str { raw } => {
                let (bytes, _) = decode_string(self.slice(first.span), raw, first.span)?;
                Some(ArgName::Str(
                    String::from_utf8_lossy(&bytes)
                        .into_owned()
                        .into_boxed_str(),
                ))
            }
            _ => None,
        };
        if let Some(name) = candidate {
            self.advance();
            self.skip_newlines_and_comments();
            if self.peek_kind() == &TokKind::Eq {
                self.advance();
                return Ok(Some((name, first.span)));
            }
        }
        self.cursor = saved;
        Ok(None)
    }

    fn peek_binary_op(&self) -> Option<(&'s str, Prec, Assoc)> {
        let tok = self.peek();
        let text = match tok.kind {
            TokKind::Question
            | TokKind::Eq
            | TokKind::LeftAssign
            | TokKind::SuperLeftAssign
            | TokKind::RightAssign
            | TokKind::SuperRightAssign
            | TokKind::ColonAssign
            | TokKind::Tilde
            | TokKind::Or
            | TokKind::OrOr
            | TokKind::And
            | TokKind::AndAnd
            | TokKind::Lt
            | TokKind::Le
            | TokKind::Gt
            | TokKind::Ge
            | TokKind::EqEq
            | TokKind::Ne
            | TokKind::Plus
            | TokKind::Minus
            | TokKind::Star
            | TokKind::Slash
            | TokKind::Special
            | TokKind::Pipe
            | TokKind::Colon
            | TokKind::Caret
            | TokKind::Dollar
            | TokKind::At
            | TokKind::NsGet
            | TokKind::NsGetInt => self.slice(tok.span),
            _ => return None,
        };
        let (prec, assoc) = ops::binary(text)?;
        Some((text, prec, assoc))
    }

    fn make_binary_call(
        &mut self,
        op: &str,
        op_span: Span,
        lhs: NodeId,
        rhs: NodeId,
        span: Span,
    ) -> NodeId {
        let func = self.ast.push_node(Node::Sym(op.into()), op_span);
        let args = self.ast.push_args([
            Arg {
                name: None,
                value: Some(lhs),
                span: self.ast.span(lhs),
            },
            Arg {
                name: None,
                value: Some(rhs),
                span: self.ast.span(rhs),
            },
        ]);
        self.ast.push_node(Node::Call { func, args }, span)
    }

    fn desugar_pipe(&mut self, lhs: NodeId, rhs: NodeId, span: Span) -> Result<NodeId, ParseError> {
        match self.ast.node(rhs).clone() {
            Node::Call { func, args } => {
                let mut existing: Vec<Arg> = self.ast.args(args).to_vec();
                let mut placeholder_idx = None;
                for (i, arg) in existing.iter().enumerate() {
                    if let Some(val) = arg.value
                        && matches!(self.ast.node(val), Node::Sym(s) if s.as_ref() == "_")
                    {
                        if arg.name.is_none() {
                            return Err(ParseError {
                                message: String::from(
                                    "pipe placeholder may only be used as a named argument",
                                ),
                                span: arg.span,
                            });
                        }
                        if placeholder_idx.is_some() {
                            return Err(ParseError {
                                message: String::from(
                                    "pipe placeholder may only appear once in RHS",
                                ),
                                span: arg.span,
                            });
                        }
                        placeholder_idx = Some(i);
                    }
                }
                if let Some(idx) = placeholder_idx {
                    existing[idx].value = Some(lhs);
                } else {
                    existing.insert(
                        0,
                        Arg {
                            name: None,
                            value: Some(lhs),
                            span: self.ast.span(lhs),
                        },
                    );
                }
                let new_args = self.ast.push_args(existing);
                Ok(self.ast.push_node(
                    Node::Call {
                        func,
                        args: new_args,
                    },
                    span,
                ))
            }
            _ => Err(ParseError {
                message: String::from("pipe operator requires a function call as RHS"),
                span,
            }),
        }
    }
}

fn parse_r_double(s: &str) -> Option<f64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return parse_hex_float(hex);
    }
    s.parse::<f64>().ok()
}

fn parse_hex_float(hex: &str) -> Option<f64> {
    let (mantissa, exp) = match hex.find(['p', 'P']) {
        Some(idx) => {
            let e = hex[idx + 1..].parse::<i32>().ok()?;
            (&hex[..idx], e)
        }
        None => (hex, 0),
    };
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(idx) => (&mantissa[..idx], &mantissa[idx + 1..]),
        None => (mantissa, ""),
    };
    let mut val = 0.0f64;
    for b in int_part.bytes() {
        let d = hex_digit(b)?;
        val = val * 16.0 + f64::from(d);
    }
    let mut scale = 1.0f64 / 16.0;
    for b in frac_part.bytes() {
        let d = hex_digit(b)?;
        val += f64::from(d) * scale;
        scale /= 16.0;
    }
    Some(val * 2.0f64.powi(exp))
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_r_int(
    num_str: &str,
    span: Span,
    warnings: &mut Vec<ParseWarning>,
) -> Result<Node, ParseError> {
    if let Some(hex) = num_str
        .strip_prefix("0x")
        .or_else(|| num_str.strip_prefix("0X"))
    {
        if !hex.contains('.')
            && !hex.contains(['p', 'P'])
            && let Ok(v) = i64::from_str_radix(hex, 16)
            && v <= i64::from(i32::MAX)
        {
            return Ok(Node::Const(Const::Int(v as i32)));
        }
    } else if let Ok(v) = num_str.parse::<i64>()
        && v <= i64::from(i32::MAX)
    {
        return Ok(Node::Const(Const::Int(v as i32)));
    }

    let f = parse_r_double(num_str).ok_or_else(|| ParseError {
        message: format!("invalid integer literal '{num_str}L'"),
        span,
    })?;
    if f.fract() == 0.0 && f >= f64::from(-i32::MAX) && f <= f64::from(i32::MAX) {
        Ok(Node::Const(Const::Int(f as i32)))
    } else {
        warnings.push(ParseWarning {
            message: format!("non-integer value {num_str} qualified with L; using numeric value"),
            span,
        });
        Ok(Node::Const(Const::Real(f.to_bits())))
    }
}

fn decode_string(
    raw_token: &str,
    is_raw: bool,
    span: Span,
) -> Result<(Box<[u8]>, StrEncoding), ParseError> {
    if is_raw {
        // Strip r/R, quote, dashes, opening delim ... closing delim, dashes, quote.
        let inner = &raw_token[2..raw_token.len() - 1];
        let dashes = inner.bytes().take_while(|&b| b == b'-').count();
        let body = &inner[dashes + 1..inner.len() - dashes - 1];
        let enc = if body.is_ascii() {
            StrEncoding::Native
        } else {
            StrEncoding::Utf8
        };
        return Ok((body.as_bytes().into(), enc));
    }
    let inner = &raw_token[1..raw_token.len() - 1];
    decode_escaped(inner, span)
}

fn decode_escaped(inner: &str, span: Span) -> Result<(Box<[u8]>, StrEncoding), ParseError> {
    let mut out = Vec::with_capacity(inner.len());
    let mut has_unicode_escape = false;
    let mut has_high_hex = false;
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= bytes.len() {
            return Err(ParseError {
                message: String::from("unterminated escape sequence"),
                span,
            });
        }
        match bytes[i] {
            b'n' => {
                out.push(b'\n');
                i += 1;
            }
            b'r' => {
                out.push(b'\r');
                i += 1;
            }
            b't' => {
                out.push(b'\t');
                i += 1;
            }
            b'b' => {
                out.push(0x08);
                i += 1;
            }
            b'a' => {
                out.push(0x07);
                i += 1;
            }
            b'f' => {
                out.push(0x0C);
                i += 1;
            }
            b'v' => {
                out.push(0x0B);
                i += 1;
            }
            b'\\' => {
                out.push(b'\\');
                i += 1;
            }
            b'"' => {
                out.push(b'"');
                i += 1;
            }
            b'\'' => {
                out.push(b'\'');
                i += 1;
            }
            b'`' => {
                out.push(b'`');
                i += 1;
            }
            b' ' => {
                out.push(b' ');
                i += 1;
            }
            b'x' => {
                i += 1;
                let mut val = 0u8;
                let mut count = 0;
                while count < 2 && i < bytes.len() {
                    if let Some(d) = hex_digit(bytes[i]) {
                        val = val * 16 + d;
                        i += 1;
                        count += 1;
                    } else {
                        break;
                    }
                }
                if count == 0 {
                    return Err(ParseError {
                        message: String::from("\\x used without hex digits"),
                        span,
                    });
                }
                if val == 0 {
                    return Err(ParseError {
                        message: String::from("nul character not allowed"),
                        span,
                    });
                }
                if val >= 0x80 {
                    has_high_hex = true;
                }
                out.push(val);
            }
            b'u' | b'U' => {
                let max_digits = if bytes[i] == b'u' { 4 } else { 8 };
                i += 1;
                let braced = i < bytes.len() && bytes[i] == b'{';
                if braced {
                    i += 1;
                }
                let mut cp = 0u32;
                let mut count = 0;
                while count < max_digits && i < bytes.len() {
                    if let Some(d) = hex_digit(bytes[i]) {
                        cp = cp * 16 + u32::from(d);
                        i += 1;
                        count += 1;
                    } else {
                        break;
                    }
                }
                if braced {
                    if i >= bytes.len() || bytes[i] != b'}' {
                        return Err(ParseError {
                            message: String::from("unterminated unicode escape"),
                            span,
                        });
                    }
                    i += 1;
                }
                if count == 0 || cp == 0 {
                    return Err(ParseError {
                        message: String::from("invalid unicode escape"),
                        span,
                    });
                }
                let ch = char::from_u32(cp).ok_or_else(|| ParseError {
                    message: String::from("invalid unicode scalar value"),
                    span,
                })?;
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                has_unicode_escape = true;
            }
            b'0'..=b'7' => {
                let mut val = 0u32;
                let mut count = 0;
                while count < 3 && i < bytes.len() && (b'0'..=b'7').contains(&bytes[i]) {
                    val = val * 8 + u32::from(bytes[i] - b'0');
                    i += 1;
                    count += 1;
                }
                if val == 0 {
                    return Err(ParseError {
                        message: String::from("nul character not allowed"),
                        span,
                    });
                }
                if val >= 0x80 {
                    has_high_hex = true;
                }
                out.push((val & 0xFF) as u8);
            }
            other => {
                return Err(ParseError {
                    message: format!("'\\{}' is an unrecognized escape", other as char),
                    span,
                });
            }
        }
    }
    if has_unicode_escape && has_high_hex {
        return Err(ParseError {
            message: String::from(
                "mixing Unicode and octal/hex escapes in a string is not allowed",
            ),
            span,
        });
    }
    let enc = if has_high_hex {
        StrEncoding::Bytes
    } else if has_unicode_escape || !out.is_ascii() {
        StrEncoding::Utf8
    } else {
        StrEncoding::Native
    };
    Ok((out.into_boxed_slice(), enc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unary_minus_and_right_assoc_pow() {
        let mut p = Parser::new("-2 ^ 3 ^ 2", SrcFileId(0));
        let exprs = p.parse_all().unwrap();
        assert_eq!(exprs.len(), 1);
        let ast = p.into_ast();
        // Top-level node must be unary `-` applied to `2 ^ (3 ^ 2)`.
        let Node::Call { func, args } = ast.node(exprs[0]) else {
            panic!("expected call");
        };
        assert!(matches!(ast.node(*func), Node::Sym(s) if s.as_ref() == "-"));
        assert_eq!(ast.args(args.clone()).len(), 1);
    }

    #[test]
    fn parses_raw_strings_and_pipe() {
        let mut p = Parser::new(r#"R"--[a\nb]--" |> f(y = _)"#, SrcFileId(0));
        let exprs = p.parse_all().unwrap();
        assert_eq!(exprs.len(), 1);
    }
}

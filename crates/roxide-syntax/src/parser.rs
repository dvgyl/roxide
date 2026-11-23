//! Clean-room Pratt parser for GNU R 4.6.2.

use std::{collections::HashSet, fmt};

use roxide_diag::{LineDirective, Span, SrcFileId};

use crate::ast::{Arg, ArgName, Ast, Const, Node, NodeId, StrEncoding};
use crate::lexer::{Lexer, TokKind, Token};
use crate::literal::{decode_escaped, decode_string, parse_r_double, parse_r_int, symbol_name};
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Newlines {
    TopLevel,
    Block,
    Ignore,
}

#[derive(Clone, Copy)]
struct ExprContext {
    newlines: Newlines,
    allow_equals: bool,
}

impl ExprContext {
    const TOP_LEVEL: Self = Self {
        newlines: Newlines::TopLevel,
        allow_equals: true,
    };
    const BLOCK: Self = Self {
        newlines: Newlines::Block,
        allow_equals: true,
    };
    const PAREN: Self = Self {
        newlines: Newlines::Ignore,
        allow_equals: true,
    };
    const ARGUMENT: Self = Self {
        newlines: Newlines::Ignore,
        allow_equals: false,
    };

    fn body(self) -> Self {
        Self {
            allow_equals: true,
            ..self
        }
    }
}

/// R 4.6.2 Pratt parser producing an [`Ast`].
pub struct Parser<'s> {
    src: &'s str,
    error_call: Option<NodeId>,
    expression_start: usize,
    tokens: Vec<Token>,
    cursor: usize,
    ast: Ast,
    warnings: Vec<ParseWarning>,
    lex_error: Option<ParseError>,
    line_directives: Vec<LineDirective>,
    placeholders: Vec<NodeId>,
    depth: usize,
}

impl<'s> Parser<'s> {
    /// Lex and initialize a [`Parser`] over `src`.
    #[must_use]
    pub fn new(src: &'s str, file: SrcFileId) -> Self {
        let mut lexer = Lexer::new(src, file);
        let mut tokens = Vec::new();
        let lex_error = loop {
            match lexer.next_token() {
                Ok(token) => {
                    let eof = token.kind == TokKind::Eof;
                    tokens.push(token);
                    if eof {
                        break None;
                    }
                }
                Err(err) => {
                    tokens.push(Token {
                        kind: TokKind::Eof,
                        span: err.span,
                    });
                    break Some(ParseError {
                        message: err.message,
                        span: err.span,
                    });
                }
            }
        };
        let line_directives = collect_line_directives(src, &tokens);
        Self {
            src,
            error_call: None,
            expression_start: 0,
            tokens,
            cursor: 0,
            ast: Ast::new(),
            warnings: Vec::new(),
            lex_error,
            line_directives,
            placeholders: Vec::new(),
            depth: 0,
        }
    }

    /// Return source-line directives in physical line order.
    ///
    /// Attach these records to the source file to map byte spans to logical lines.
    #[must_use]
    pub fn line_directives(&self) -> &[LineDirective] {
        &self.line_directives
    }

    fn end_of_input(&mut self, needs_more_input: bool) -> ParseStep {
        match self.lex_error.take() {
            Some(err)
                if matches!(
                    err.message.as_str(),
                    "unterminated string literal"
                        | "unterminated raw string literal"
                        | "unterminated backtick symbol"
                ) =>
            {
                ParseStep::Incomplete
            }
            Some(err) => ParseStep::Error(err),
            None if needs_more_input => ParseStep::Incomplete,
            None => ParseStep::Eof,
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

    /// Return the call associated with the most recent parse error, if present.
    #[must_use]
    pub fn error_call(&self) -> Option<NodeId> {
        self.error_call
    }

    /// Parse the next single top-level expression.
    pub fn parse_next(&mut self) -> ParseStep {
        self.skip_newlines_and_comments();
        if self.is_eof() {
            return self.end_of_input(false);
        }
        self.error_call = None;
        self.expression_start = self
            .cursor
            .checked_sub(1)
            .map_or(0, |i| self.tokens[i].span.hi as usize);
        self.placeholders.clear();
        match self.parse_expr(Prec(0), ExprContext::TOP_LEVEL) {
            Ok(expr) => {
                // Consume optional trailing `;` or newline separator.
                self.skip_comments();
                if matches!(self.peek_kind(), TokKind::Semi | TokKind::Newline) {
                    self.advance();
                } else if !self.is_eof() {
                    return ParseStep::Error(self.unexpected());
                } else if self.lex_error.is_some() {
                    return self.end_of_input(false);
                }
                if let Some(&placeholder) = self.placeholders.first() {
                    return ParseStep::Error(self.invalid_placeholder(placeholder));
                }
                ParseStep::Expr(expr)
            }
            Err(err) if self.is_eof() && err.span == self.current_span() => self.end_of_input(true),
            Err(err) => ParseStep::Error(err),
        }
    }

    fn unexpected(&self) -> ParseError {
        ParseError {
            message: format!("unexpected '{}'", self.current_text()),
            span: self.current_span(),
        }
    }

    fn slice(&self, span: Span) -> &'s str {
        &self.src[span.lo as usize..span.hi as usize]
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn peek_kind(&self) -> &TokKind {
        &self.peek().kind
    }

    fn current_span(&self) -> Span {
        self.peek().span
    }

    fn current_text(&self) -> &'s str {
        self.slice(self.peek().span)
    }

    fn is_eof(&self) -> bool {
        self.peek_kind() == &TokKind::Eof
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

    fn skip_expr_trivia(&mut self, context: ExprContext) {
        if context.newlines == Newlines::Ignore {
            self.skip_newlines_and_comments();
        } else {
            self.skip_comments();
        }
    }

    fn parse_expr(&mut self, min_prec: Prec, context: ExprContext) -> Result<NodeId, ParseError> {
        const MAX_DEPTH: usize = 256;
        self.skip_expr_trivia(context);
        if self.depth == MAX_DEPTH && !self.is_eof() {
            return Err(ParseError {
                message: String::from("expression nested too deeply"),
                span: self.current_span(),
            });
        }
        self.depth += 1;
        let result = self.parse_expr_inner(min_prec, context);
        self.depth -= 1;
        result
    }

    fn parse_expr_inner(
        &mut self,
        min_prec: Prec,
        context: ExprContext,
    ) -> Result<NodeId, ParseError> {
        let mut lhs = self.parse_prefix(context)?;

        loop {
            self.skip_expr_trivia(context);

            if min_prec <= ops::prec::SUBSET {
                match self.peek_kind() {
                    TokKind::LParen => {
                        lhs = self.parse_call(lhs)?;
                        continue;
                    }
                    TokKind::LBracket | TokKind::LBracket2 => {
                        lhs = self.parse_subset(lhs, self.peek_kind() == &TokKind::LBracket2)?;
                        continue;
                    }
                    _ => {}
                }
            }

            let Some((op_str, prec, assoc)) = self.peek_binary_op() else {
                break;
            };
            if prec < min_prec || (op_str == "=" && !context.allow_equals) {
                break;
            }

            if matches!(prec, ops::prec::NS | ops::prec::EXTRACT) {
                lhs = self.parse_member(lhs, context)?;
                continue;
            }

            let op_tok = self.advance();
            self.skip_newlines_and_comments();

            let next_min = match assoc {
                Assoc::Right => prec,
                Assoc::Left | Assoc::NonAssoc => Prec(prec.0 + 1),
            };
            let placeholder_start = self.placeholders.len();
            let rhs = self.parse_expr(next_min, context)?;
            if assoc == Assoc::NonAssoc
                && self
                    .peek_binary_op()
                    .is_some_and(|(_, next, _)| next == prec)
            {
                return Err(self.unexpected());
            }
            let span = self.ast.span(lhs).merge(self.ast.span(rhs));

            lhs = match op_str {
                "->" => self.make_binary_call("<-", op_tok.span, rhs, lhs, span),
                "->>" => self.make_binary_call("<<-", op_tok.span, rhs, lhs, span),
                "**" => self.make_binary_call("^", op_tok.span, lhs, rhs, span),
                "|>" => self.desugar_pipe(lhs, rhs, span, placeholder_start)?,
                _ => self.make_binary_call(op_str, op_tok.span, lhs, rhs, span),
            };
        }

        Ok(lhs)
    }

    fn parse_member(&mut self, lhs: NodeId, context: ExprContext) -> Result<NodeId, ParseError> {
        let namespace = matches!(self.peek_kind(), TokKind::NsGet | TokKind::NsGetInt);
        if namespace
            && !matches!(
                self.ast.node(lhs),
                Node::Sym(_) | Node::Const(Const::Str { .. })
            )
        {
            return Err(self.unexpected());
        }
        if namespace && self.placeholders.last() == Some(&lhs) {
            return Err(self.invalid_placeholder(lhs));
        }
        let op = self.advance();
        if namespace {
            self.skip_expr_trivia(context);
        } else {
            self.skip_newlines_and_comments();
        }
        if !matches!(
            self.peek_kind(),
            TokKind::Symbol | TokKind::Backtick | TokKind::Str { .. }
        ) {
            return Err(self.unexpected());
        }
        let rhs = self.parse_prefix(context)?;
        let span = self.ast.span(lhs).merge(self.ast.span(rhs));
        Ok(self.make_binary_call(self.slice(op.span), op.span, lhs, rhs, span))
    }

    fn parse_prefix(&mut self, context: ExprContext) -> Result<NodeId, ParseError> {
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
                let node = self.ast.push_node(Node::Sym(name), tok.span);
                if tok.kind == TokKind::Placeholder {
                    self.placeholders.push(node);
                }
                Ok(node)
            }
            TokKind::Backtick => {
                self.advance();
                let name = self.decode_symbol(&tok)?;
                Ok(self.ast.push_node(Node::Sym(name), tok.span))
            }
            TokKind::Plus | TokKind::Minus | TokKind::Bang | TokKind::Tilde | TokKind::Question => {
                self.advance();
                let op_str = self.slice(tok.span);
                let mut prec = ops::unary(op_str).unwrap_or(ops::prec::UNARY);
                if matches!(tok.kind, TokKind::Tilde | TokKind::Question) {
                    prec.0 += 1;
                }
                let operand_context = if tok.kind == TokKind::Question {
                    context.body()
                } else {
                    context
                };
                self.skip_newlines_and_comments();
                let rhs = self.parse_expr(prec, operand_context)?;
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
                let inner = self.parse_expr(Prec(0), ExprContext::PAREN)?;
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
            TokKind::Function | TokKind::Lambda => self.parse_function(context),
            TokKind::If => self.parse_if(context),
            TokKind::While => self.parse_while(context),
            TokKind::Repeat => self.parse_repeat(context),
            TokKind::For => self.parse_for(context),
            TokKind::Break | TokKind::Next => {
                self.advance();
                let name = self.slice(tok.span);
                let func = self.ast.push_node(Node::Sym(name.into()), tok.span);
                let args = self.ast.push_args([]);
                Ok(self.ast.push_node(Node::Call { func, args }, tok.span))
            }
            _ => Err(self.unexpected()),
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
            let stmt = self.parse_expr(Prec(0), ExprContext::BLOCK)?;
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

    fn parse_function(&mut self, context: ExprContext) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        if kw.kind == TokKind::Lambda {
            self.skip_expr_trivia(context);
        } else {
            self.skip_newlines_and_comments();
        }
        if self.peek_kind() != &TokKind::LParen {
            return Err(ParseError {
                message: String::from("expected '(' after function"),
                span: self.current_span(),
            });
        }
        self.advance();
        let mut formals = Vec::new();
        let mut names = HashSet::new();
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::RParen {
            loop {
                self.skip_newlines_and_comments();
                let name_tok = self.advance();
                let name_str: Box<str> = match name_tok.kind {
                    TokKind::Symbol | TokKind::Backtick => self.decode_symbol(&name_tok)?,
                    _ => {
                        return Err(ParseError {
                            message: String::from("expected formal parameter name"),
                            span: name_tok.span,
                        });
                    }
                };
                if !names.insert(name_str.clone()) {
                    return Err(ParseError {
                        message: format!("repeated formal argument '{name_str}'"),
                        span: name_tok.span,
                    });
                }
                self.skip_newlines_and_comments();
                let default = if self.peek_kind() == &TokKind::Eq {
                    self.advance();
                    self.skip_newlines_and_comments();
                    Some(self.parse_expr(Prec(0), ExprContext::ARGUMENT)?)
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
        let body = self.parse_expr(ops::prec::EQ_ASSIGN, context.body())?;
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

    fn parse_if(&mut self, context: ExprContext) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::LParen {
            return Err(ParseError {
                message: String::from("expected '(' after 'if'"),
                span: self.current_span(),
            });
        }
        self.advance();
        let cond = self.parse_expr(Prec(0), ExprContext::ARGUMENT)?;
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::RParen {
            return Err(ParseError {
                message: String::from("expected ')' after 'if' condition"),
                span: self.current_span(),
            });
        }
        self.advance();
        self.skip_newlines_and_comments();
        let then_branch = self.parse_expr(ops::prec::EQ_ASSIGN, context.body())?;

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
        if context.newlines != Newlines::TopLevel {
            self.skip_newlines_and_comments();
        } else {
            self.skip_comments();
        }
        let end_span = if self.peek_kind() == &TokKind::Else {
            self.advance();
            self.skip_newlines_and_comments();
            let else_branch = self.parse_expr(ops::prec::EQ_ASSIGN, context.body())?;
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

    fn parse_while(&mut self, context: ExprContext) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::LParen {
            return Err(ParseError {
                message: String::from("expected '(' after 'while'"),
                span: self.current_span(),
            });
        }
        self.advance();
        let cond = self.parse_expr(Prec(0), ExprContext::ARGUMENT)?;
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::RParen {
            return Err(ParseError {
                message: String::from("expected ')' after 'while' condition"),
                span: self.current_span(),
            });
        }
        self.advance();
        self.skip_newlines_and_comments();
        let body = self.parse_expr(ops::prec::EQ_ASSIGN, context.body())?;
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

    fn parse_repeat(&mut self, context: ExprContext) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        self.skip_newlines_and_comments();
        let body = self.parse_expr(ops::prec::EQ_ASSIGN, context.body())?;
        let span = kw.span.merge(self.ast.span(body));
        let func = self.ast.push_node(Node::Sym("repeat".into()), kw.span);
        let args = self.ast.push_args([Arg {
            name: None,
            value: Some(body),
            span: self.ast.span(body),
        }]);
        Ok(self.ast.push_node(Node::Call { func, args }, span))
    }

    fn parse_for(&mut self, context: ExprContext) -> Result<NodeId, ParseError> {
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
            TokKind::Symbol | TokKind::Backtick => self.decode_symbol(&var_tok)?,
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
        let seq = self.parse_expr(Prec(0), ExprContext::ARGUMENT)?;
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::RParen {
            return Err(ParseError {
                message: String::from("expected ')' in 'for' loop"),
                span: self.current_span(),
            });
        }
        self.advance();
        self.skip_newlines_and_comments();
        let body = self.parse_expr(ops::prec::EQ_ASSIGN, context.body())?;
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
        self.advance();
        let callee_span = self.ast.span(callee);
        let callee = if let Node::Const(Const::Str { bytes, .. }) = self.ast.node(callee) {
            let name = symbol_name(bytes, callee_span)?;
            self.ast.push_node(Node::Sym(name), callee_span)
        } else {
            callee
        };
        let (args_vec, close_span) = self.parse_arg_list(TokKind::RParen)?;
        let span = callee_span.merge(close_span);
        let args = self.ast.push_args(args_vec);
        Ok(self.ast.push_node(Node::Call { func: callee, args }, span))
    }

    fn parse_subset(&mut self, target: NodeId, double: bool) -> Result<NodeId, ParseError> {
        let open = self.advance();
        let (mut args_vec, close_span) = self.parse_arg_list(TokKind::RBracket)?;
        if args_vec.is_empty() {
            args_vec.push(Arg {
                name: None,
                value: None,
                span: close_span,
            });
        }
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
                    Some(self.parse_expr(Prec(0), ExprContext::ARGUMENT)?)
                };
                let span = value.map_or(name_span, |v| name_span.merge(self.ast.span(v)));
                args.push(Arg {
                    name: Some(arg_name),
                    value,
                    span,
                });
            } else {
                let expr = self.parse_expr(Prec(0), ExprContext::ARGUMENT)?;
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
        if !matches!(
            first.kind,
            TokKind::Symbol | TokKind::Backtick | TokKind::Null | TokKind::Str { .. }
        ) {
            return Ok(None);
        }
        self.advance();
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::Eq {
            self.cursor = saved;
            return Ok(None);
        }
        let name = match first.kind {
            TokKind::Null => ArgName::Null,
            TokKind::Str { raw } => {
                let (bytes, _) = decode_string(self.slice(first.span), raw, first.span)?;
                ArgName::Str(symbol_name(&bytes, first.span)?)
            }
            _ => ArgName::Sym(self.decode_symbol(&first)?),
        };
        self.advance();
        Ok(Some((name, first.span)))
    }

    fn decode_symbol(&self, token: &Token) -> Result<Box<str>, ParseError> {
        let raw = self.slice(token.span);
        if token.kind == TokKind::Backtick {
            let (bytes, _) = decode_escaped(&raw[1..raw.len() - 1], token.span, true)?;
            symbol_name(&bytes, token.span)
        } else {
            Ok(raw.into())
        }
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

    fn invalid_placeholder(&self, placeholder: NodeId) -> ParseError {
        ParseError {
            message: String::from("invalid use of pipe placeholder"),
            span: self.ast.span(placeholder),
        }
    }

    fn desugar_pipe(
        &mut self,
        lhs: NodeId,
        rhs: NodeId,
        span: Span,
        placeholder_start: usize,
    ) -> Result<NodeId, ParseError> {
        let rhs_span = self.ast.span(rhs);
        if self.placeholders.get(placeholder_start) == Some(&rhs)
            || !matches!(
                self.ast.node(rhs),
                Node::Call { .. } | Node::Function { .. }
            )
        {
            return Err(ParseError {
                message: String::from("The pipe operator requires a function call as RHS"),
                span: rhs_span,
            });
        }
        let Node::Call { func, args } = self.ast.node(rhs).clone() else {
            return Err(ParseError {
                message: String::from("function 'function' not supported in RHS call of a pipe"),
                span: rhs_span,
            });
        };
        let pending = &self.placeholders[placeholder_start..];
        let func_span = self.ast.span(func);
        if pending.iter().any(|&id| {
            let placeholder = self.ast.span(id);
            func_span.lo <= placeholder.lo && placeholder.hi <= func_span.hi
        }) {
            return Err(ParseError {
                message: String::from("pipe placeholder cannot be used in the RHS function"),
                span: func_span,
            });
        }
        if pending.len() > 1 {
            return Err(ParseError {
                message: String::from("pipe placeholder may only appear once"),
                span: self.ast.span(pending[1]),
            });
        }

        let mut arguments = self.ast.args(args).to_vec();
        if let Some(&placeholder) = pending.first() {
            if self.replace_extraction_root(rhs, lhs, placeholder) {
                self.ast.set_span(rhs, span);
                self.placeholders.truncate(placeholder_start);
                return Ok(rhs);
            }
            let Some(arg) = arguments
                .iter_mut()
                .find(|arg| arg.value == Some(placeholder))
            else {
                return Err(self.invalid_placeholder(placeholder));
            };
            if arg.name.is_none() {
                let prefix = &self.src[self.expression_start..rhs_span.lo as usize];
                let line = 1 + prefix.bytes().filter(|&byte| byte == b'\n').count();
                let column = prefix.rsplit('\n').next().map_or(1, |line| line.len() + 1);
                let literal = Const::Str {
                    bytes: Box::from(b"_".as_slice()),
                    enc: StrEncoding::Native,
                };
                let span = arg.span;
                arg.value = Some(self.ast.push_node(Node::Const(literal), span));
                let args = self.ast.push_args(arguments);
                self.error_call = Some(self.ast.push_node(Node::Call { func, args }, rhs_span));
                return Err(ParseError {
                    message: format!(
                        "pipe placeholder can only be used as a named argument (<input>:{line}:{column})"
                    ),
                    span,
                });
            }
            arg.value = Some(lhs);
            self.placeholders.truncate(placeholder_start);
        } else {
            if let Node::Sym(name) = self.ast.node(func)
                && pipe_forbids_insertion(name)
            {
                return Err(ParseError {
                    message: format!("function '{name}' not supported in RHS call of a pipe"),
                    span: rhs_span,
                });
            }
            arguments.insert(
                0,
                Arg {
                    name: None,
                    value: Some(lhs),
                    span: self.ast.span(lhs),
                },
            );
        }
        let args = self.ast.push_args(arguments);
        Ok(self.ast.push_node(Node::Call { func, args }, span))
    }

    fn replace_extraction_root(&mut self, rhs: NodeId, lhs: NodeId, placeholder: NodeId) -> bool {
        let mut node = rhs;
        let mut chain = Vec::new();
        while let Node::Call { func, args } = self.ast.node(node) {
            if !matches!(self.ast.node(*func), Node::Sym(name) if matches!(name.as_ref(), "$" | "@" | "[" | "[["))
            {
                break;
            }
            let Some(target) = self
                .ast
                .args(args.clone())
                .first()
                .and_then(|arg| arg.value)
            else {
                break;
            };
            chain.push(node);
            if target == placeholder {
                self.ast.replace_arg(args.start, lhs);
                let lhs_span = self.ast.span(lhs);
                for extraction in chain {
                    self.ast
                        .set_span(extraction, lhs_span.merge(self.ast.span(extraction)));
                }
                return true;
            }
            node = target;
        }
        false
    }
}

fn pipe_forbids_insertion(name: &str) -> bool {
    matches!(
        name,
        "if" | "while"
            | "for"
            | "repeat"
            | "function"
            | "break"
            | "next"
            | "return"
            | "{"
            | "("
            | "["
            | "[["
            | "$"
            | "@"
            | "::"
            | ":::"
            | "+"
            | "-"
            | "*"
            | "/"
            | "^"
            | ":"
            | "~"
            | "?"
            | "!"
            | "&"
            | "&&"
            | "|"
            | "||"
            | "<"
            | "<="
            | ">"
            | ">="
            | "=="
            | "!="
            | "<-"
            | "<<-"
            | "="
            | "|>"
            | "%%"
            | "%/%"
            | "%*%"
    )
}

fn collect_line_directives(src: &str, tokens: &[Token]) -> Vec<LineDirective> {
    let mut directives = Vec::new();
    let mut physical_line = 1u32;
    let mut previous = 0;
    for token in tokens.iter().filter(|token| token.kind == TokKind::Comment) {
        let lo = token.span.lo as usize;
        let hi = token.span.hi as usize;
        if lo != 0 && src.as_bytes()[lo - 1] != b'\n' {
            continue;
        }
        let Some(text) = src[lo..hi].strip_prefix("#line") else {
            continue;
        };
        let text = text.trim_start_matches([' ', '\t']);
        let digits = text.bytes().take_while(u8::is_ascii_digit).count();
        let Ok(logical_line) = text[..digits].parse::<u32>() else {
            continue;
        };
        let physical_lines = src.as_bytes()[previous..lo]
            .iter()
            .filter(|&&b| b == b'\n')
            .count();
        physical_line += physical_lines as u32;
        previous = lo;
        let filename =
            directive_filename(text[digits..].trim_start_matches([' ', '\t']), token.span);
        directives.push(LineDirective {
            physical_line,
            logical_line,
            filename,
        });
    }
    directives
}

fn directive_filename(text: &str, span: Span) -> Option<String> {
    if !text.starts_with('"') {
        return None;
    }
    let token = Lexer::new(text, span.file).next_token().ok()?;
    let (bytes, _) = decode_string(&text[..token.span.hi as usize], false, span).ok()?;
    String::from_utf8(bytes.into_vec()).ok()
}

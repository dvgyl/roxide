//! Clean-room Pratt parser for GNU R 4.6.2.

use std::fmt;

use roxide_diag::{LineDirective, Span, SrcFileId};

use crate::ast::{Arg, ArgName, Ast, Const, Node, NodeId};
use crate::lexer::{Lexer, TokKind, Token};
use crate::literal::{decode_escaped, decode_string, parse_r_double, parse_r_int, symbol_name};
use crate::ops::{self, Assoc, Prec};

mod forms;
mod pipe;

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
enum ExprContext {
    TopLevel,
    Block,
    Parens,
    Arg,
}

impl ExprContext {
    fn body(self) -> Self {
        match self {
            Self::Arg => Self::Parens,
            other => other,
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
        match self.parse_expr(Prec(0), ExprContext::TopLevel) {
            Ok(expr) => {
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
        if matches!(context, ExprContext::Parens | ExprContext::Arg) {
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
            if prec < min_prec || (op_str == "=" && context == ExprContext::Arg) {
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
                "->" => self.operand_call("<-", op_tok.span, [rhs, lhs], span),
                "->>" => self.operand_call("<<-", op_tok.span, [rhs, lhs], span),
                "**" => self.operand_call("^", op_tok.span, [lhs, rhs], span),
                "|>" => self.desugar_pipe(lhs, rhs, span, placeholder_start)?,
                _ => self.operand_call(op_str, op_tok.span, [lhs, rhs], span),
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
        Ok(self.operand_call(self.slice(op.span), op.span, [lhs, rhs], span))
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
                Ok(self.operand_call(op_str, tok.span, [rhs], span))
            }
            TokKind::LParen => {
                let open = self.advance();
                self.skip_newlines_and_comments();
                let inner = self.parse_expr(Prec(0), ExprContext::Parens)?;
                self.skip_newlines_and_comments();
                if self.peek_kind() != &TokKind::RParen {
                    return Err(ParseError {
                        message: String::from("expected ')'"),
                        span: self.current_span(),
                    });
                }
                let close = self.advance();
                let span = open.span.merge(close.span);
                Ok(self.operand_call("(", open.span, [inner], span))
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
                Ok(self.operand_call(name, tok.span, [], tok.span))
            }
            _ => Err(self.unexpected()),
        }
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
            let named = self.try_parse_arg_name()?;
            if let Some((arg_name, name_span)) = named {
                self.skip_newlines_and_comments();
                let value = if self.peek_kind() == &TokKind::Comma || self.peek_kind() == &closing {
                    None
                } else {
                    Some(self.parse_expr(Prec(0), ExprContext::Arg)?)
                };
                let span = value.map_or(name_span, |v| name_span.merge(self.ast.span(v)));
                args.push(Arg {
                    name: Some(arg_name),
                    value,
                    span,
                });
            } else {
                let expr = self.parse_expr(Prec(0), ExprContext::Arg)?;
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

    fn operand_call<const N: usize>(
        &mut self,
        name: &str,
        name_span: Span,
        operands: [NodeId; N],
        span: Span,
    ) -> NodeId {
        let func = self.ast.push_node(Node::Sym(name.into()), name_span);
        let args = operands.map(|value| Arg {
            name: None,
            value: Some(value),
            span: self.ast.span(value),
        });
        let args = self.ast.push_args(args);
        self.ast.push_node(Node::Call { func, args }, span)
    }
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

use std::collections::HashSet;

use super::{Arg, ArgName, ExprContext, Node, NodeId, ParseError, Parser, Prec, TokKind, ops};

impl Parser<'_> {
    pub(super) fn parse_brace_block(&mut self) -> Result<NodeId, ParseError> {
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
            let stmt = self.parse_expr(Prec(0), ExprContext::Block)?;
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

    pub(super) fn parse_function(&mut self, context: ExprContext) -> Result<NodeId, ParseError> {
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
                    Some(self.parse_expr(Prec(0), ExprContext::Arg)?)
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

    pub(super) fn parse_if(&mut self, context: ExprContext) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        let cond = self.parse_condition("if")?;
        let then_branch = self.parse_expr(ops::prec::EQ_ASSIGN, context.body())?;

        let saved = self.cursor;
        if context != ExprContext::TopLevel {
            self.skip_newlines_and_comments();
        } else {
            self.skip_comments();
        }
        if self.peek_kind() == &TokKind::Else {
            self.advance();
            self.skip_newlines_and_comments();
            let else_branch = self.parse_expr(ops::prec::EQ_ASSIGN, context.body())?;
            let span = kw.span.merge(self.ast.span(else_branch));
            Ok(self.operand_call("if", kw.span, [cond, then_branch, else_branch], span))
        } else {
            self.cursor = saved;
            let span = kw.span.merge(self.ast.span(then_branch));
            Ok(self.operand_call("if", kw.span, [cond, then_branch], span))
        }
    }

    fn parse_condition(&mut self, keyword: &str) -> Result<NodeId, ParseError> {
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::LParen {
            return Err(ParseError {
                message: format!("expected '(' after '{keyword}'"),
                span: self.current_span(),
            });
        }
        self.advance();
        let cond = self.parse_expr(Prec(0), ExprContext::Arg)?;
        self.skip_newlines_and_comments();
        if self.peek_kind() != &TokKind::RParen {
            return Err(ParseError {
                message: format!("expected ')' after '{keyword}' condition"),
                span: self.current_span(),
            });
        }
        self.advance();
        self.skip_newlines_and_comments();
        Ok(cond)
    }

    pub(super) fn parse_while(&mut self, context: ExprContext) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        let cond = self.parse_condition("while")?;
        let body = self.parse_expr(ops::prec::EQ_ASSIGN, context.body())?;
        let span = kw.span.merge(self.ast.span(body));
        Ok(self.operand_call("while", kw.span, [cond, body], span))
    }

    pub(super) fn parse_repeat(&mut self, context: ExprContext) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        self.skip_newlines_and_comments();
        let body = self.parse_expr(ops::prec::EQ_ASSIGN, context.body())?;
        let span = kw.span.merge(self.ast.span(body));
        Ok(self.operand_call("repeat", kw.span, [body], span))
    }

    pub(super) fn parse_for(&mut self, context: ExprContext) -> Result<NodeId, ParseError> {
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
        let seq = self.parse_expr(Prec(0), ExprContext::Arg)?;
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
        Ok(self.operand_call("for", kw.span, [var_node, seq, body], span))
    }
}

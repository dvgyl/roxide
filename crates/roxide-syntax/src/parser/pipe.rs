use std::ops::Range;

use crate::ast::StrEncoding;

use super::{Arg, Const, Node, NodeId, ParseError, Parser, Span};

enum PipeTarget {
    Call {
        func: NodeId,
        args: Range<u32>,
        placement: Placement,
    },
    Extraction {
        argument: u32,
        chain: Vec<NodeId>,
    },
}

enum Placement {
    First,
    Named(usize),
    Unnamed(usize),
}

impl Parser<'_> {
    pub(super) fn invalid_placeholder(&self, placeholder: NodeId) -> ParseError {
        ParseError {
            message: String::from("invalid use of pipe placeholder"),
            span: self.ast.span(placeholder),
        }
    }

    pub(super) fn desugar_pipe(
        &mut self,
        lhs: NodeId,
        rhs: NodeId,
        span: Span,
        placeholder_start: usize,
    ) -> Result<NodeId, ParseError> {
        let (func, mut arguments, placement) = match self.pipe_target(rhs, placeholder_start)? {
            PipeTarget::Extraction { argument, chain } => {
                self.ast.replace_arg(argument, lhs);
                let lhs_span = self.ast.span(lhs);
                for extraction in chain {
                    self.ast
                        .set_span(extraction, lhs_span.merge(self.ast.span(extraction)));
                }
                self.ast.set_span(rhs, span);
                self.placeholders.truncate(placeholder_start);
                return Ok(rhs);
            }
            PipeTarget::Call {
                func,
                args,
                placement,
            } => (func, self.ast.args(args).to_vec(), placement),
        };
        match placement {
            Placement::First => arguments.insert(
                0,
                Arg {
                    name: None,
                    value: Some(lhs),
                    span: self.ast.span(lhs),
                },
            ),
            Placement::Named(index) => {
                arguments[index].value = Some(lhs);
                self.placeholders.truncate(placeholder_start);
            }
            Placement::Unnamed(index) => {
                let rhs_span = self.ast.span(rhs);
                let prefix = &self.src[self.expression_start..rhs_span.lo as usize];
                let line = 1 + prefix.bytes().filter(|&byte| byte == b'\n').count();
                let column = prefix.rsplit('\n').next().map_or(1, |line| line.len() + 1);
                let literal = Const::Str {
                    bytes: Box::from(b"_".as_slice()),
                    enc: StrEncoding::Native,
                };
                let span = arguments[index].span;
                arguments[index].value = Some(self.ast.push_node(Node::Const(literal), span));
                let args = self.ast.push_args(arguments);
                self.error_call = Some(self.ast.push_node(Node::Call { func, args }, rhs_span));
                return Err(ParseError {
                    message: format!(
                        "pipe placeholder can only be used as a named argument (<input>:{line}:{column})"
                    ),
                    span,
                });
            }
        }
        let args = self.ast.push_args(arguments);
        Ok(self.ast.push_node(Node::Call { func, args }, span))
    }

    fn pipe_target(&self, rhs: NodeId, placeholder_start: usize) -> Result<PipeTarget, ParseError> {
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

        let placement = if let Some(&placeholder) = pending.first() {
            if let Some(extraction) = self.extraction_target(rhs, placeholder) {
                return Ok(extraction);
            } else {
                let arguments = self.ast.args(args.clone());
                let Some(index) = arguments
                    .iter()
                    .position(|arg| arg.value == Some(placeholder))
                else {
                    return Err(self.invalid_placeholder(placeholder));
                };
                if arguments[index].name.is_some() {
                    Placement::Named(index)
                } else {
                    Placement::Unnamed(index)
                }
            }
        } else {
            if let Node::Sym(name) = self.ast.node(func)
                && pipe_forbids_insertion(name)
            {
                return Err(ParseError {
                    message: format!("function '{name}' not supported in RHS call of a pipe"),
                    span: rhs_span,
                });
            }
            Placement::First
        };
        Ok(PipeTarget::Call {
            func,
            args,
            placement,
        })
    }

    fn extraction_target(&self, rhs: NodeId, placeholder: NodeId) -> Option<PipeTarget> {
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
                return Some(PipeTarget::Extraction {
                    argument: args.start,
                    chain,
                });
            }
            node = target;
        }
        None
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

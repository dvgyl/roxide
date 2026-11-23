//! Format calls, control flow, and function definitions.

use roxide_gc::SexpId;
use roxide_syntax::ops::{self, Assoc, Prec, prec};

use crate::api::{ValueApi, ValueError};
use crate::object::SexpType;
use crate::session::SEXP_MISSING;

use super::{Arg, Deparser, symbol};

impl Deparser<'_> {
    pub(super) fn precedence(
        &self,
        x: SexpId,
        ty: SexpType,
        sequence: bool,
    ) -> Result<Prec, ValueError> {
        if sequence {
            return Ok(prec::SEQ);
        }
        if ty == SexpType::Lang {
            let (head, tail, _) = self.session.cons_parts(x)?;
            if let Ok(op) = self.session.sym_name_of(head) {
                let mut args = self.session.pairlist_iter(tail).take(3);
                let count = args.by_ref().count();
                return Ok(match (op, count) {
                    ("[" | "[[", 1..) => prec::SUBSET,
                    ("if" | "while" | "for" | "repeat" | "function", _) => Prec(0),
                    (_, 1) => ops::unary(op).filter(|_| op != "?").unwrap_or(Prec(19)),
                    (_, 2) => display_binary(op).map_or(Prec(19), |(p, _)| p),
                    _ => Prec(19),
                });
            }
        }
        if ty == SexpType::Cplx {
            let view = self.session.cplx(x)?;
            if view.len() == 1 {
                let z = view.get(0);
                if !z.re.is_nan() && z.im.is_finite() {
                    return Ok(prec::ADD);
                }
            }
        }
        Ok(Prec(20))
    }

    pub(super) fn arguments(
        &mut self,
        args: impl Iterator<Item = Arg>,
        depth: usize,
        indent: usize,
    ) -> Result<(), ValueError> {
        for (i, (tag, value)) in args.enumerate() {
            self.separator(i, indent + 4);
            if let Some(tag) = tag {
                self.push(&symbol(self.session.sym_name(tag)?));
                self.push(" = ");
            }
            self.value(value, depth + 1, indent, Prec(0))?;
        }
        Ok(())
    }

    pub(super) fn function(
        &mut self,
        formals: SexpId,
        body: SexpId,
        closure: bool,
        depth: usize,
        indent: usize,
    ) -> Result<(), ValueError> {
        self.push(if closure { "function (" } else { "function(" });
        for (i, (tag, value)) in self.session.pairlist_iter(formals).enumerate() {
            self.separator(i, indent + 4);
            if let Some(tag) = tag {
                self.push(&symbol(self.session.sym_name(tag)?));
                if value != SEXP_MISSING {
                    self.push(" = ");
                }
            }
            self.value(value, depth + 1, indent, Prec(0))?;
        }
        self.push(") ");
        if closure {
            self.newline(indent);
        }
        self.value(body, depth + 1, indent, Prec(0))
    }

    pub(super) fn call(
        &mut self,
        x: SexpId,
        depth: usize,
        indent: usize,
    ) -> Result<(), ValueError> {
        let (head, tail, _) = self.session.cons_parts(x)?;
        let op = self.session.sym_name_of(head).unwrap_or("");
        let args: Vec<_> = self.session.pairlist_iter(tail).collect();
        let next = depth + 1;
        match (op, args.as_slice()) {
            ("{", args) => {
                self.push("{");
                for &(_, expr) in args {
                    self.newline(indent + 4);
                    self.value(expr, next, indent + 4, Prec(0))?;
                }
                self.newline(indent);
                self.push("}");
            }
            ("(", [(_, arg)]) => {
                self.push("(");
                self.value(*arg, next, indent, Prec(0))?;
                self.push(")");
            }
            ("function", [(_, formals), (_, body)] | [(_, formals), (_, body), _]) => {
                self.function(*formals, *body, false, depth, indent)?;
            }
            ("if", [(_, cond), (_, yes)] | [(_, cond), (_, yes), _]) => {
                self.push("if (");
                self.value(*cond, next, indent, Prec(0))?;
                self.push(") ");
                let block = self.is_block(*yes)?;
                if indent > 0 && !block {
                    self.newline(indent + 4);
                    self.value(*yes, next, indent + 4, Prec(0))?;
                } else {
                    self.value(*yes, next, indent, Prec(0))?;
                }
                if let Some(&(_, no)) = args.get(2) {
                    if indent > 0 && !(block && self.is_block(no)?) {
                        self.newline(indent);
                        self.push("else ");
                    } else {
                        self.push(" else ");
                    }
                    self.value(no, next, indent, Prec(0))?;
                }
            }
            ("for", [(_, var), (_, range), (_, body)]) => {
                self.push("for (");
                self.value(*var, next, indent, Prec(0))?;
                self.push(" in ");
                self.value(*range, next, indent, Prec(0))?;
                self.push(") ");
                self.value(*body, next, indent, Prec(0))?;
            }
            ("while", [(_, cond), (_, body)]) => {
                self.push("while (");
                self.value(*cond, next, indent, Prec(0))?;
                self.push(") ");
                self.value(*body, next, indent, Prec(0))?;
            }
            ("repeat", [(_, body)]) => {
                self.push("repeat ");
                self.value(*body, next, indent, Prec(0))?;
            }
            ("break" | "next", []) => self.push(op),
            ("[" | "[[", [(_, target), subs @ ..]) => {
                self.value(*target, next, indent, prec::EXTRACT)?;
                self.push(op);
                self.arguments(subs.iter().copied(), depth, indent)?;
                self.push(if op == "[" { "]" } else { "]]" });
            }
            (_, [(None, arg)]) if op != "?" && ops::unary(op).is_some() => {
                self.push(op);
                self.value(*arg, next, indent, ops::unary(op).unwrap_or(Prec(0)))?;
            }
            (_, [(None, lhs), (None, rhs)]) if display_binary(op).is_some() => {
                let (p, assoc) = display_binary(op).unwrap_or((Prec(0), Assoc::NonAssoc));
                let (op, lhs, rhs, assoc) = match op {
                    "->" => ("<-", rhs, lhs, Assoc::Right),
                    "->>" => ("<<-", rhs, lhs, Assoc::Right),
                    _ => (op, lhs, rhs, assoc),
                };
                let compact = matches!(
                    op,
                    "^" | "**" | ":" | "/" | "$" | "@" | "::" | ":::" | "%%" | "%/%"
                );
                let lhs_min = Prec(p.0 + u8::from(assoc != Assoc::Left));
                let rhs_min = Prec(p.0 + u8::from(assoc != Assoc::Right));
                self.value(*lhs, next, indent, lhs_min)?;
                if !compact {
                    self.push(" ");
                }
                self.push(op);
                if !compact {
                    self.push(" ");
                    self.wrap(indent + 4);
                }
                let rhs_min = if self.is_control(*rhs)? {
                    Prec(0)
                } else {
                    rhs_min
                };
                self.value(*rhs, next, indent, rhs_min)?;
            }
            _ => {
                self.value(head, next, indent, prec::EXTRACT)?;
                self.push("(");
                if !args.is_empty() {
                    self.wrap(indent + 4);
                }
                self.arguments(args.into_iter(), depth, indent)?;
                self.push(")");
            }
        }
        Ok(())
    }

    fn is_control(&self, id: SexpId) -> Result<bool, ValueError> {
        Ok(self.session.type_of(id)? == SexpType::Lang
            && matches!(
                self.session.sym_name_of(self.session.cons_parts(id)?.0),
                Ok("if" | "while" | "for" | "repeat" | "function")
            ))
    }

    fn is_block(&self, id: SexpId) -> Result<bool, ValueError> {
        Ok(self.session.type_of(id)? == SexpType::Lang
            && self.session.sym_name_of(self.session.cons_parts(id)?.0) == Ok("{"))
    }
}

fn display_binary(op: &str) -> Option<(Prec, Assoc)> {
    match op {
        "?" | ":=" => None,
        "->" | "->>" => Some((prec::LEFT_ASSIGN, Assoc::Right)),
        _ => ops::binary(op),
    }
}

//! Deparse R objects as R source.

use roxide_gc::SexpId;
use roxide_syntax::ops::Prec;

use crate::api::{ValueApi, ValueError, VecView};
use crate::format::encode_string;
use crate::na::NA_INTEGER;
use crate::object::{CharId, SexpType, Sym};
use crate::session::{RSession, SEXP_MISSING};

mod atomic;
mod language;

pub(crate) use atomic::deparse_number;

/// Controls for [`deparse`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeparseOpts {
    /// Soft line-width cutoff in bytes (default `60`).
    pub width_cutoff: usize,
    /// Emit `L` suffix on integer constants (`keepInteger`).
    pub keep_integer: bool,
    /// Emit typed `NA_*` constants when needed (`keepNA`).
    pub keep_na: bool,
    /// Use high-precision double formatting (`digits17`).
    pub digits17: bool,
}

impl Default for DeparseOpts {
    fn default() -> Self {
        Self {
            width_cutoff: 60,
            keep_integer: true,
            keep_na: true,
            digits17: false,
        }
    }
}

/// Deparse an R object into source lines with the default R deparse quoting rules.
pub fn deparse(
    session: &RSession,
    x: SexpId,
    opts: DeparseOpts,
) -> Result<Vec<String>, ValueError> {
    if x != SEXP_MISSING && session.type_of(x)? == SexpType::Sym {
        return Ok(vec![session.sym_name_of(x)?.to_owned()]);
    }
    render(session, x, opts, false)
}

/// Deparse an R object with the quoting rules of R dput.
pub fn dput(session: &RSession, x: SexpId, opts: DeparseOpts) -> Result<Vec<String>, ValueError> {
    render(session, x, opts, true)
}

fn render(
    session: &RSession,
    x: SexpId,
    opts: DeparseOpts,
    dput: bool,
) -> Result<Vec<String>, ValueError> {
    let mut out = Deparser {
        session,
        opts,
        dput,
        lines: Vec::new(),
        line: String::new(),
    };
    out.value(x, 0, 0, Prec(0))?;
    out.lines.push(out.line);
    Ok(out.lines)
}

const MAX_DEPARSE_DEPTH: usize = 128;

type Arg = (Option<Sym>, SexpId);

struct Deparser<'a> {
    session: &'a RSession,
    opts: DeparseOpts,
    dput: bool,
    lines: Vec<String>,
    line: String,
}

impl Deparser<'_> {
    fn push(&mut self, text: &str) {
        self.line.push_str(text);
    }

    fn newline(&mut self, indent: usize) {
        self.lines.push(std::mem::take(&mut self.line));
        self.line.push_str(&" ".repeat(indent));
    }

    fn wrap(&mut self, indent: usize) {
        if self.line.len() > self.opts.width_cutoff {
            self.newline(indent);
        }
    }

    fn separator(&mut self, i: usize, indent: usize) {
        if i != 0 {
            self.push(", ");
            self.wrap(indent);
        }
    }

    fn value(
        &mut self,
        x: SexpId,
        depth: usize,
        indent: usize,
        min: Prec,
    ) -> Result<(), ValueError> {
        if depth >= MAX_DEPARSE_DEPTH {
            return Err(ValueError::InvalidAttribute(
                "deparse depth limit exceeded".into(),
            ));
        }
        if x == SEXP_MISSING {
            return Ok(());
        }
        let ty = self.session.type_of(x)?;
        let attrs = self.session.attrs(x)?;
        let sequence = if ty == SexpType::Int {
            int_sequence(self.session, x)?
        } else {
            None
        };
        let mut names = &[][..];
        if sequence.is_none()
            && matches!(
                ty,
                SexpType::Lgl
                    | SexpType::Int
                    | SexpType::Real
                    | SexpType::Cplx
                    | SexpType::Raw
                    | SexpType::Str
                    | SexpType::Vec
                    | SexpType::Expr
            )
        {
            for (key, val) in &attrs {
                if self.session.sym_name(*key)? == "names" {
                    let slice = self.session.str_slice(*val)?;
                    if !slice.contains(&CharId::NA)
                        && slice.iter().any(|name| *name != CharId::BLANK)
                    {
                        names = slice;
                    }
                }
            }
        }
        let mut wrapper = false;
        for (key, _) in &attrs {
            let key = self.session.sym_name(*key)?;
            if keep_attribute(key, names) {
                wrapper = true;
                break;
            }
        }
        let own = if wrapper || !names.is_empty() {
            Prec(20)
        } else {
            self.precedence(x, ty, sequence.is_some())?
        };
        let parens = own < min;
        if parens {
            self.push("(");
        }
        if wrapper {
            self.push("structure(");
        }
        if let Some((first, last)) = sequence {
            self.push(&format!("{first}:{last}"));
        } else {
            match ty {
                SexpType::Nil => self.push("NULL"),
                SexpType::Sym => self.push(&symbol(self.session.sym_name_of(x)?)),
                SexpType::Char => self.push(&self.string(CharId(x))?),
                SexpType::Lgl
                | SexpType::Int
                | SexpType::Real
                | SexpType::Cplx
                | SexpType::Str
                | SexpType::Raw => self.atomic(x, ty, names, indent)?,
                SexpType::Vec | SexpType::Expr => {
                    let label = if ty == SexpType::Vec {
                        "list("
                    } else {
                        "expression("
                    };
                    self.push(label);
                    let items = self.session.list_slice(x)?;
                    if !items.is_empty() {
                        self.wrap(indent + 4);
                    }
                    for (i, &item) in items.iter().enumerate() {
                        self.separator(i, indent + 4);
                        self.vector_name(names, i)?;
                        self.value(item, depth + 1, indent, Prec(0))?;
                    }
                    self.push(")");
                }
                SexpType::List | SexpType::Dot => {
                    self.push("pairlist(");
                    self.arguments(self.session.pairlist_iter(x), depth, indent)?;
                    self.push(")");
                }
                SexpType::Lang => self.call(x, depth, indent)?,
                SexpType::Clo => {
                    let (formals, body, _) = self.session.closure_parts(x)?;
                    self.function(formals, body, true, depth, indent)?;
                }
                SexpType::Prom => {
                    let (expr, _, _, _) = self.session.promise_parts(x)?;
                    self.value(expr, depth + 1, indent, Prec(0))?;
                }
                _ => self.push(&format!("<{}>", ty.type_name(self.session.is_s4(x)?))),
            }
        }
        if wrapper {
            for (key, val) in attrs {
                let key = self.session.sym_name(key)?;
                if keep_attribute(key, names) {
                    self.push(", ");
                    let name = symbol(key);
                    if name == key {
                        self.push(key);
                    } else {
                        let quoted = if self.dput { &name } else { key };
                        self.push(&encode_string(quoted.as_bytes()));
                    }
                    self.push(" = ");
                    self.value(val, depth + 1, indent, Prec(0))?;
                }
            }
            self.push(")");
        }
        if parens {
            self.push(")");
        }
        Ok(())
    }
}

fn keep_attribute(key: &str, names: &[CharId]) -> bool {
    !(key == "names" && !names.is_empty()) && key != "srcref"
}

fn int_sequence(session: &RSession, id: SexpId) -> Result<Option<(i32, i32)>, ValueError> {
    let v = session.int(id)?;
    if v.len() < 2 {
        return Ok(None);
    }
    if let VecView::Alt { class, heap, .. } = &v
        && let Some(bounds) = class.unit_sequence_bounds(heap)
    {
        return Ok(Some(bounds));
    }
    let first = v.get(0);
    let last = v.get(v.len() - 1);
    let delta = i64::from(last) - i64::from(first);
    if delta.unsigned_abs() != (v.len() - 1) as u64 || first == NA_INTEGER || last == NA_INTEGER {
        return Ok(None);
    }
    let step = delta.signum();
    if v.iter()
        .enumerate()
        .all(|(i, n)| i64::from(n) == i64::from(first) + i as i64 * step)
    {
        Ok(Some((first, last)))
    } else {
        Ok(None)
    }
}

fn symbol(name: &str) -> String {
    let mut chars = name.chars();
    let first = chars.next();
    let syntactic = first.is_some_and(|c| {
        c.is_alphabetic() || (c == '.' && !chars.clone().next().is_some_and(|c| c.is_ascii_digit()))
    }) && chars.all(|c| c.is_alphanumeric() || matches!(c, '.' | '_'))
        && !matches!(
            name,
            "if" | "else"
                | "repeat"
                | "while"
                | "function"
                | "for"
                | "in"
                | "next"
                | "break"
                | "TRUE"
                | "FALSE"
                | "NULL"
                | "Inf"
                | "NaN"
                | "NA"
                | "NA_integer_"
                | "NA_real_"
                | "NA_complex_"
                | "NA_character_"
        );
    if syntactic {
        return name.to_owned();
    }
    let escaped = encode_string(name.as_bytes());
    let inner = escaped[1..escaped.len() - 1]
        .replace("\\\"", "\"")
        .replace('`', "\\`");
    format!("`{inner}`")
}

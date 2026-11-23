//! Lowering from heap-free [`roxide_syntax::Ast`] into R `SEXP` language objects.

use roxide_syntax::{ArgName, Ast, Const, Node, NodeId, StrEncoding};

use roxide_gc::SexpId;

use crate::api::{ValueApi, ValueError};
use crate::na::{RComplex, RLogical};
use crate::object::{CharId, Encoding};
use crate::session::{RSession, SEXP_MISSING, SEXP_NIL};

/// Whether lowering attaches `srcref` metadata.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeepSource {
    /// Do not attach `srcref` (default for `Rscript`).
    No,
    /// Attach `srcref` metadata (interactive / `parse(keep.source = TRUE)`).
    Yes,
}

/// Lower a single [`NodeId`] in `ast` into an R language object (`LANGSXP`/`SYMSXP`/literal).
pub fn lower(
    ast: &Ast,
    root: NodeId,
    session: &mut RSession,
    _keep_source: KeepSource,
) -> Result<SexpId, ValueError> {
    match ast.node(root) {
        Node::Const(c) => match c {
            Const::Null => Ok(SEXP_NIL),
            Const::Lgl(opt) => session.scalar_lgl(RLogical::from_option(*opt)),
            Const::Int(n) => session.scalar_int(*n),
            Const::Real(bits) => session.scalar_real(f64::from_bits(*bits)),
            Const::Complex { re_bits, im_bits } => session.alloc_cplx_from(&[RComplex {
                re: f64::from_bits(*re_bits),
                im: f64::from_bits(*im_bits),
            }]),
            Const::Str { bytes, enc } => {
                let enc = match enc {
                    StrEncoding::Native => Encoding::Native,
                    StrEncoding::Utf8 => Encoding::Utf8,
                    StrEncoding::Bytes => Encoding::Bytes,
                };
                let cid = session.mk_char(bytes, enc)?;
                session.alloc_str_from(&[cid])
            }
            Const::NaChar => session.alloc_str_from(&[CharId::NA]),
        },
        Node::Sym(name) => Ok(session.intern(name)?.as_sexp()),
        Node::Empty => Ok(SEXP_MISSING),
        Node::Call { func, args } => {
            let func_sexp = lower(ast, *func, session, _keep_source)?;
            let mut tail = SEXP_NIL;
            for arg in ast.args(args.clone()).iter().rev() {
                let val = match arg.value {
                    Some(v) => lower(ast, v, session, _keep_source)?,
                    None => SEXP_MISSING,
                };
                let tag = match &arg.name {
                    None => SEXP_NIL,
                    Some(ArgName::Sym(s)) | Some(ArgName::Str(s)) => session.intern(s)?.as_sexp(),
                    Some(ArgName::Null) => session.intern("NULL")?.as_sexp(),
                };
                tail = session.cons(val, tail, tag)?;
            }
            session.lcons(func_sexp, tail, SEXP_NIL)
        }
        Node::Function { formals, body, .. } => {
            let fn_sym = session.intern("function")?.as_sexp();
            let mut formal_list = SEXP_NIL;
            for f in ast.args(formals.clone()).iter().rev() {
                let default = match f.value {
                    Some(v) => lower(ast, v, session, _keep_source)?,
                    None => SEXP_MISSING,
                };
                let tag = match &f.name {
                    Some(ArgName::Sym(s)) | Some(ArgName::Str(s)) => session.intern(s)?.as_sexp(),
                    _ => SEXP_NIL,
                };
                formal_list = session.cons(default, formal_list, tag)?;
            }
            let body_sexp = lower(ast, *body, session, _keep_source)?;
            let cdr3 = session.cons(SEXP_NIL, SEXP_NIL, SEXP_NIL)?;
            let cdr2 = session.cons(body_sexp, cdr3, SEXP_NIL)?;
            let cdr1 = session.cons(formal_list, cdr2, SEXP_NIL)?;
            session.lcons(fn_sym, cdr1, SEXP_NIL)
        }
    }
}

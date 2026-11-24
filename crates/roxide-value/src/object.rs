//! Core [`RObject`], [`SexpType`], [`Sym`], [`CharId`], and internal heap body definitions.

use std::any::Any;
use std::collections::HashMap;
use std::fmt;

use roxide_gc::{HeapSize, SexpId, Trace, Tracer};

use crate::alt::AltObj;
use crate::na::{RComplex, RLogical};

/// Numeric R `SEXPTYPE` codes (used by `typeof()`, serialization, and M4 `TYPEOF()`).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SexpType {
    /// `NILSXP` (`NULL`)
    Nil = 0,
    /// `SYMSXP` (symbol / name)
    Sym = 1,
    /// `LISTSXP` (dotted pairlist)
    List = 2,
    /// `CLOSXP` (closure)
    Clo = 3,
    /// `ENVSXP` (environment)
    Env = 4,
    /// `PROMSXP` (lazy promise)
    Prom = 5,
    /// `LANGSXP` (language call)
    Lang = 6,
    /// `SPECIALSXP` (unevaluated-arg primitive)
    Special = 7,
    /// `BUILTINSXP` (evaluated-arg primitive)
    Builtin = 8,
    /// `CHARSXP` (internal interned character string)
    Char = 9,
    /// `LGLSXP` (logical vector)
    Lgl = 10,
    /// `INTSXP` (integer vector)
    Int = 13,
    /// `REALSXP` (double vector)
    Real = 14,
    /// `CPLXSXP` (complex vector)
    Cplx = 15,
    /// `STRSXP` (character vector)
    Str = 16,
    /// `DOTSXP` (`...` argument list)
    Dot = 17,
    /// `ANYSXP`
    Any = 18,
    /// `VECSXP` (generic vector / list)
    Vec = 19,
    /// `EXPRSXP` (expression vector)
    Expr = 20,
    /// `BCODESXP` (bytecode, M4)
    Bcode = 21,
    /// `EXTPTRSXP` (external pointer)
    ExtPtr = 22,
    /// `WEAKREFSXP` (weak reference)
    WeakRef = 23,
    /// `RAWSXP` (raw byte vector)
    Raw = 24,
    /// `OBJSXP` / `S4SXP` (object / S4 instance)
    Obj = 25,
}

impl SexpType {
    /// Return R's `typeof()` string for this type (given the S4 flag for `Obj`).
    #[must_use]
    pub const fn type_name(self, is_s4: bool) -> &'static str {
        match self {
            Self::Nil => "NULL",
            Self::Sym => "symbol",
            Self::List => "pairlist",
            Self::Clo => "closure",
            Self::Env => "environment",
            Self::Prom => "promise",
            Self::Lang => "language",
            Self::Special => "special",
            Self::Builtin => "builtin",
            Self::Char => "char",
            Self::Lgl => "logical",
            Self::Int => "integer",
            Self::Real => "double",
            Self::Cplx => "complex",
            Self::Str => "character",
            Self::Dot => "...",
            Self::Any => "any",
            Self::Vec => "list",
            Self::Expr => "expression",
            Self::Bcode => "bytecode",
            Self::ExtPtr => "externalptr",
            Self::WeakRef => "weakref",
            Self::Raw => "raw",
            Self::Obj => {
                if is_s4 {
                    "S4"
                } else {
                    "object"
                }
            }
        }
    }
}

/// Typed handle to a permanent interned `SYMSXP`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Sym(pub(crate) SexpId);

impl Sym {
    /// Underlying [`SexpId`].
    #[inline]
    #[must_use]
    pub const fn as_sexp(self) -> SexpId {
        self.0
    }
}

/// Typed handle to an interned `CHARSXP`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct CharId(pub(crate) SexpId);

impl CharId {
    /// Permanent `NA_STRING` sentinel (index 3, distinct from `"NA"`).
    pub const NA: Self = Self(SexpId::permanent(3));
    /// Permanent empty string `""` (index 4).
    pub const BLANK: Self = Self(SexpId::permanent(4));

    /// Underlying [`SexpId`].
    #[inline]
    #[must_use]
    pub const fn as_sexp(self) -> SexpId {
        self.0
    }
}

/// Character string encoding mark (`Encoding()`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Encoding {
    /// Native / unknown encoding (pure ASCII is always marked `Native`).
    Native,
    /// UTF-8 encoding.
    Utf8,
    /// Latin-1 encoding.
    Latin1,
    /// Raw byte string (`"bytes"`).
    Bytes,
}

impl Encoding {
    /// Return R's `Encoding()` display string.
    #[must_use]
    pub const fn as_r_str(self) -> &'static str {
        match self {
            Self::Native => "unknown",
            Self::Utf8 => "UTF-8",
            Self::Latin1 => "latin1",
            Self::Bytes => "bytes",
        }
    }
}

/// Interned `CHARSXP` payload (stores bytes with a trailing NUL for M4 `CHAR()`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RChar {
    /// UTF-8 / raw bytes plus trailing `0u8` terminator.
    pub(crate) bytes_nul: Box<[u8]>,
    /// Encoding tag.
    pub(crate) enc: Encoding,
    /// Cached ASCII flag.
    pub(crate) ascii: bool,
}

impl RChar {
    /// Construct a new [`RChar`] with a trailing NUL byte.
    #[must_use]
    pub(crate) fn new(bytes: &[u8], enc: Encoding) -> Self {
        let mut v = Vec::with_capacity(bytes.len() + 1);
        v.extend_from_slice(bytes);
        v.push(0);
        let ascii = bytes.is_ascii();
        let enc = if ascii && enc != Encoding::Bytes {
            Encoding::Native
        } else {
            enc
        };
        Self {
            bytes_nul: v.into_boxed_slice(),
            enc,
            ascii,
        }
    }

    /// Borrow string bytes without the trailing NUL.
    #[inline]
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes_nul[..self.bytes_nul.len() - 1]
    }

    /// Borrow NUL-terminated bytes for C-API `R_CHAR()` access.
    #[inline]
    #[must_use]
    pub fn bytes_nul(&self) -> &[u8] {
        &self.bytes_nul
    }

    /// Encoding mark.
    #[inline]
    #[must_use]
    pub const fn encoding(&self) -> Encoding {
        self.enc
    }

    /// True if all bytes are 7-bit ASCII.
    #[inline]
    #[must_use]
    pub const fn is_ascii(&self) -> bool {
        self.ascii
    }
}

/// Index into `roxide-eval`'s primitive/internal function registry.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PrimId(pub u16);

/// Four-state lazy promise lifecycle (ADR-0006).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromiseState {
    /// Not yet forced.
    Unforced,
    /// Currently evaluating (re-entry raises recursive-promise error).
    Evaluating,
    /// Prior evaluation unwound via error; retry emits restart warning.
    Interrupted,
    /// Forced and memoized (`env` released, `expr` retained for `substitute()`).
    Forced,
}

/// Fixed-capacity vector backing buffer whose address stays stable across the object lifetime (ADR-0002).
#[derive(Clone, Debug)]
pub struct RVec<T> {
    pub(crate) buf: Box<[T]>,
    pub(crate) len: usize,
    pub(crate) resizable: bool,
}

impl<T> RVec<T> {
    /// Create from a `Vec<T>` boxed to fixed capacity.
    #[must_use]
    pub(crate) fn from_vec(v: Vec<T>) -> Self {
        let len = v.len();
        Self {
            buf: v.into_boxed_slice(),
            len,
            resizable: false,
        }
    }

    /// Create a resizable vector with logical length `len` and fixed pinned capacity `max_len`.
    #[must_use]
    pub(crate) fn resizable(buf: Box<[T]>, len: usize) -> Self {
        debug_assert!(len <= buf.len());
        Self {
            buf,
            len,
            resizable: true,
        }
    }

    /// Logical length.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Allocated backing capacity (`TRUELENGTH` for resizable vectors).
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Return true if allocated via `R_allocResizableVector`.
    #[inline]
    #[must_use]
    pub const fn is_resizable(&self) -> bool {
        self.resizable
    }

    /// Return true if empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Borrow active elements immutably.
    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.buf[..self.len]
    }

    /// Borrow active elements mutably.
    #[inline]
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.buf[..self.len]
    }
}

/// One binding inside an R environment frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    /// Bound value or promise handle.
    pub value: SexpId,
    /// Whether `lockBinding()` has been applied.
    pub locked: bool,
    /// Whether this binding is an active binding (`makeActiveBinding`).
    pub active: bool,
    /// Missing-argument status (`0` = normal, `1` = missing default, `2` = propagated missing).
    pub missing: u8,
}

/// Environment frame storage (insertion order preserved; O(1) lookup via hash index).
#[derive(Clone, Debug)]
pub(crate) struct EnvData {
    pub(crate) entries: Vec<(Sym, Binding)>,
    pub(crate) index: HashMap<Sym, usize>,
    pub(crate) enclos: SexpId,
    pub(crate) locked: bool,
    /// Monotonic shape version bumped on binding insertion/removal for VM inline caches (ADR-0020).
    pub(crate) shape_version: u32,
}

impl EnvData {
    pub(crate) fn new(enclos: SexpId) -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
            enclos,
            locked: false,
            shape_version: 0,
        }
    }
}

/// Cons cell for `LISTSXP`, `LANGSXP`, and `DOTSXP`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cons {
    pub(crate) car: SexpId,
    pub(crate) cdr: SexpId,
    pub(crate) tag: SexpId,
}

/// External pointer payload trait for Rust-owned external objects.
pub trait ExtPayload: Any + fmt::Debug + 'static {}
impl<T: Any + fmt::Debug + 'static> ExtPayload for T {}

/// External pointer body (`EXTPTRSXP`, used by connections and Milestone 4 C API).
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct ExtPtrData {
    pub(crate) addr: usize,
    pub(crate) rust: Option<Box<dyn ExtPayload>>,
    pub(crate) tag: SexpId,
    pub(crate) prot: SexpId,
}

/// Weak reference body (`WEAKREFSXP`, ephemeron-managed by `roxide-gc`, ADR-0019).
#[derive(Debug)]
pub(crate) struct WeakRefData {
    pub(crate) key: SexpId,
    pub(crate) value: SexpId,
    pub(crate) finalizer: SexpId,
}

impl WeakRefData {
    pub(crate) fn clear(&mut self) {
        self.key = crate::session::SEXP_NIL;
        self.value = crate::session::SEXP_NIL;
        self.finalizer = crate::session::SEXP_NIL;
    }
}

/// Object header flags.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct ObjFlags {
    /// True iff a non-NIL `"class"` attribute is attached (S3 dispatch fast path).
    pub(crate) object: bool,
    /// True iff this object is an S4 instance.
    pub(crate) s4: bool,
}

/// Ordered attribute map preserving insertion order.
#[derive(Clone, Default, Debug)]
pub(crate) struct Attrs(pub(crate) Option<Box<[(Sym, SexpId)]>>);

impl Attrs {
    pub(crate) fn get(&self, key: Sym) -> Option<SexpId> {
        self.0
            .as_deref()
            .and_then(|slice| slice.iter().find(|(k, _)| *k == key).map(|(_, v)| *v))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (Sym, SexpId)> + '_ {
        self.0.as_deref().unwrap_or(&[]).iter().copied()
    }
}

/// Tagged payload variants for [`RObject`].
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) enum Body {
    Nil,
    Sym {
        name: CharId,
    },
    List(Cons),
    Lang(Cons),
    Dots(Cons),
    Closure {
        formals: SexpId,
        body: SexpId,
        env: SexpId,
    },
    Env(Box<EnvData>),
    Promise {
        expr: SexpId,
        env: SexpId,
        value: SexpId,
        state: PromiseState,
    },
    Builtin(PrimId),
    Special(PrimId),
    Char(RChar),
    Lgl(RVec<RLogical>),
    Int(RVec<i32>),
    Real(RVec<f64>),
    Cplx(RVec<RComplex>),
    Str(RVec<CharId>),
    Raw(RVec<u8>),
    Vec(RVec<SexpId>),
    Expr(RVec<SexpId>),
    ExtPtr(Box<ExtPtrData>),
    WeakRef(Box<WeakRefData>),
    Obj,
    Alt(Box<AltObj>),
}

/// GC-managed R object stored in [`crate::RHeap`].
#[derive(Debug)]
pub struct RObject {
    pub(crate) gp: u16,
    pub(crate) flags: ObjFlags,
    pub(crate) attrs: Attrs,
    pub(crate) body: Body,
}

impl RObject {
    pub(crate) fn new(body: Body) -> Self {
        Self {
            gp: 0,
            flags: ObjFlags::default(),
            attrs: Attrs::default(),
            body,
        }
    }

    /// Derive the [`SexpType`] directly from the body variant (no tag/body desync possible).
    #[must_use]
    pub fn sexp_type(&self) -> SexpType {
        match &self.body {
            Body::Nil => SexpType::Nil,
            Body::Sym { .. } => SexpType::Sym,
            Body::List(_) => SexpType::List,
            Body::Lang(_) => SexpType::Lang,
            Body::Dots(_) => SexpType::Dot,
            Body::Closure { .. } => SexpType::Clo,
            Body::Env(_) => SexpType::Env,
            Body::Promise { .. } => SexpType::Prom,
            Body::Builtin(_) => SexpType::Builtin,
            Body::Special(_) => SexpType::Special,
            Body::Char(_) => SexpType::Char,
            Body::Lgl(_) => SexpType::Lgl,
            Body::Int(_) => SexpType::Int,
            Body::Real(_) => SexpType::Real,
            Body::Cplx(_) => SexpType::Cplx,
            Body::Str(_) => SexpType::Str,
            Body::Raw(_) => SexpType::Raw,
            Body::Vec(_) => SexpType::Vec,
            Body::Expr(_) => SexpType::Expr,
            Body::ExtPtr(_) => SexpType::ExtPtr,
            Body::WeakRef(_) => SexpType::WeakRef,
            Body::Obj => SexpType::Obj,
            Body::Alt(alt) => alt.base_type(),
        }
    }
}

impl Trace for RObject {
    fn trace(&self, t: &mut Tracer<'_>) {
        for (sym, val) in self.attrs.iter() {
            t.edge(sym.as_sexp());
            t.edge(val);
        }
        match &self.body {
            Body::Nil
            | Body::Builtin(_)
            | Body::Special(_)
            | Body::Char(_)
            | Body::Lgl(_)
            | Body::Int(_)
            | Body::Real(_)
            | Body::Cplx(_)
            | Body::Raw(_)
            | Body::Obj => {}
            Body::Sym { name } => t.edge(name.as_sexp()),
            Body::List(c) | Body::Lang(c) | Body::Dots(c) => {
                t.edge(c.car);
                t.edge(c.cdr);
                t.edge(c.tag);
            }
            Body::Closure { formals, body, env } => {
                t.edge(*formals);
                t.edge(*body);
                t.edge(*env);
            }
            Body::Env(e) => {
                t.edge(e.enclos);
                for (sym, bind) in &e.entries {
                    t.edge(sym.as_sexp());
                    t.edge(bind.value);
                }
            }
            Body::Promise {
                expr, env, value, ..
            } => {
                t.edge(*expr);
                t.edge(*env);
                t.edge(*value);
            }
            Body::Str(v) => {
                for c in v.as_slice() {
                    t.edge(c.as_sexp());
                }
            }
            Body::Vec(v) | Body::Expr(v) => {
                t.edges(v.as_slice());
            }
            Body::ExtPtr(ep) => {
                t.edge(ep.tag);
                t.edge(ep.prot);
            }
            // WeakRef edges are traced strictly through roxide-gc's ephemeron fixpoint (ADR-0019).
            Body::WeakRef(_) => {}
            Body::Alt(alt) => alt.trace(t),
        }
    }
}

impl HeapSize for RObject {
    fn heap_bytes(&self) -> usize {
        let base = std::mem::size_of::<Self>();
        let attr_bytes = self.attrs.0.as_deref().map_or(0, std::mem::size_of_val);
        let body_bytes = match &self.body {
            Body::Char(c) => c.bytes_nul.len(),
            Body::Lgl(v) => std::mem::size_of_val(&*v.buf),
            Body::Int(v) => std::mem::size_of_val(&*v.buf),
            Body::Real(v) => std::mem::size_of_val(&*v.buf),
            Body::Cplx(v) => std::mem::size_of_val(&*v.buf),
            Body::Str(v) => std::mem::size_of_val(&*v.buf),
            Body::Raw(v) => v.buf.len(),
            Body::Vec(v) | Body::Expr(v) => std::mem::size_of_val(&*v.buf),
            Body::Env(e) => e.entries.len() * 48 + 64,
            Body::Alt(alt) => 128 + alt.buffer_bytes(),
            _ => 0,
        };
        base + attr_bytes + body_bytes
    }
}

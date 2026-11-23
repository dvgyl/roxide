//! Worker-facing [`ValueApi`] and [`VecView`] abstraction over [`RSession`].

use std::fmt;

use roxide_gc::{HeapError, SexpId};

use crate::alt::{AltKind, AltMaterialized, AltVector};
use crate::na::{Elem, RComplex, RLogical};
use crate::object::{
    Attrs, Binding, Body, CharId, Cons, EnvData, ObjFlags, PrimId, PromiseState, RObject, RVec,
    SexpType, Sym,
};
use crate::session::{BASE_ENV, BASE_NAMESPACE, RHeap, RSession, SEXP_NIL};

/// Coercion warning flags returned by [`ValueApi::coerce`].
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct CoerceWarn {
    /// True if coercing strings or values introduced new `NA` values.
    pub na_introduced: bool,
    /// True if complex coercion discarded nonzero imaginary parts.
    pub imaginary_discarded: bool,
}

/// Errors returned by [`RSession`] / [`ValueApi`] operations (translated to R conditions by `roxide-eval`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ValueError {
    /// Underlying GC heap error.
    Heap(HeapError),
    /// Unexpected `SEXPTYPE`.
    TypeMismatch {
        /// Expected type name.
        expected: &'static str,
        /// Actual type name.
        actual: &'static str,
    },
    /// Attempted to mutate a shared object without calling [`ValueApi::unshare`] first.
    Shared(SexpId),
    /// Embedded NUL byte in a character string or symbol.
    EmbeddedNul,
    /// Attempted to modify a locked binding or locked environment.
    LockedBinding(String),
    /// Invalid attribute value (e.g. `names` length mismatch).
    InvalidAttribute(String),
    /// Index out of bounds.
    IndexOutOfBounds {
        /// Requested index.
        index: usize,
        /// Container length.
        len: usize,
    },
}

impl From<HeapError> for ValueError {
    fn from(value: HeapError) -> Self {
        Self::Heap(value)
    }
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Heap(e) => write!(f, "{e}"),
            Self::TypeMismatch { expected, actual } => {
                write!(f, "expected '{expected}', got '{actual}'")
            }
            Self::Shared(_) => f.write_str("cannot mutate shared R object in place"),
            Self::EmbeddedNul => f.write_str("embedded nul in string"),
            Self::LockedBinding(name) => {
                write!(f, "cannot change value of locked binding for '{name}'")
            }
            Self::InvalidAttribute(msg) => f.write_str(msg),
            Self::IndexOutOfBounds { index, len } => {
                write!(f, "subscript out of bounds ({index} >= {len})")
            }
        }
    }
}

impl std::error::Error for ValueError {}

/// Read-only view over either a dense vector slice or a lazy ALTREP vector.
pub enum VecView<'h, T: Elem> {
    /// Direct dense memory slice.
    Slice(&'h [T]),
    /// Lazy ALTREP vector view.
    Alt {
        /// ALTREP implementation.
        class: &'h dyn AltVector<T>,
        /// Heap reference for element lookup.
        heap: &'h RHeap,
        /// Cached length.
        len: usize,
    },
}

impl<'h, T: Elem> VecView<'h, T> {
    /// Vector length.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Slice(s) => s.len(),
            Self::Alt { len, .. } => *len,
        }
    }

    /// True if length is zero.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Read element `i`.
    #[inline]
    #[must_use]
    pub fn get(&self, i: usize) -> T {
        match self {
            Self::Slice(s) => s[i],
            Self::Alt { class, heap, .. } => class.elt(heap, i),
        }
    }

    /// Return the direct backing slice if dense (never forces ALTREP materialization).
    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> Option<&'h [T]> {
        match self {
            Self::Slice(s) => Some(s),
            Self::Alt { class, heap, .. } => class.dataptr_or_null(heap),
        }
    }

    /// Iterate over all elements.
    pub fn iter(&self) -> impl Iterator<Item = T> + Clone + '_ {
        (0..self.len()).map(|i| self.get(i))
    }

    /// Copy all elements into a [`Vec<T>`].
    #[must_use]
    pub fn to_vec(&self) -> Vec<T> {
        match self {
            Self::Slice(s) => s.to_vec(),
            Self::Alt { .. } => self.iter().collect(),
        }
    }
}

/// Iterator over a `LISTSXP`, `LANGSXP`, or `DOTSXP` cons chain yielding `(Option<Sym>, SexpId)`.
pub struct PairlistIter<'h> {
    heap: &'h RHeap,
    cur: SexpId,
}

impl Iterator for PairlistIter<'_> {
    type Item = (Option<Sym>, SexpId);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cur == SEXP_NIL {
            return None;
        }
        let obj = self.heap.get(self.cur).ok()?;
        let cons = match &obj.body {
            Body::List(c) | Body::Lang(c) | Body::Dots(c) => *c,
            _ => {
                self.cur = SEXP_NIL;
                return None;
            }
        };
        let tag = if cons.tag == SEXP_NIL {
            None
        } else {
            Some(Sym(cons.tag))
        };
        self.cur = cons.cdr;
        Some((tag, cons.car))
    }
}

/// Stable worker-facing API over [`RSession`] (ADR-0007).
pub trait ValueApi {
    /// Return the [`SexpType`] of `x`.
    fn type_of(&self, x: SexpId) -> Result<SexpType, ValueError>;
    /// Return true iff `x` has the S3 `OBJECT` bit set.
    fn is_object(&self, x: SexpId) -> Result<bool, ValueError>;
    /// Return true iff `x` has the S4 bit set.
    fn is_s4(&self, x: SexpId) -> Result<bool, ValueError>;
    /// Return the vector or pairlist length of `x`.
    fn xlength(&self, x: SexpId) -> Result<usize, ValueError>;

    /// Read-only view of a logical vector.
    fn lgl(&self, x: SexpId) -> Result<VecView<'_, RLogical>, ValueError>;
    /// Read-only view of an integer vector.
    fn int(&self, x: SexpId) -> Result<VecView<'_, i32>, ValueError>;
    /// Read-only view of a double vector.
    fn real(&self, x: SexpId) -> Result<VecView<'_, f64>, ValueError>;
    /// Read-only view of a complex vector.
    fn cplx(&self, x: SexpId) -> Result<VecView<'_, RComplex>, ValueError>;
    /// Read-only view of a raw vector.
    fn raw(&self, x: SexpId) -> Result<VecView<'_, u8>, ValueError>;
    /// Read-only slice of a character vector (materialized `CharId` slice).
    fn str_slice(&self, x: SexpId) -> Result<&[CharId], ValueError>;
    /// Read-only slice of a generic list (`VECSXP`) or expression (`EXPRSXP`).
    fn list_slice(&self, x: SexpId) -> Result<&[SexpId], ValueError>;

    /// Writable slice of an unshared double vector (materializes ALTREP & applies wrapper guard).
    fn real_mut(&mut self, x: SexpId) -> Result<&mut [f64], ValueError>;
    /// Writable slice of an unshared integer vector.
    fn int_mut(&mut self, x: SexpId) -> Result<&mut [i32], ValueError>;
    /// Writable slice of an unshared logical vector.
    fn lgl_mut(&mut self, x: SexpId) -> Result<&mut [RLogical], ValueError>;

    /// Return `x` if unshared, otherwise return a shallow duplicate (`copy-on-modify`).
    fn unshare(&mut self, x: SexpId) -> Result<SexpId, ValueError>;

    /// Allocate a logical vector from a slice.
    fn alloc_lgl_from(&mut self, vals: &[RLogical]) -> Result<SexpId, ValueError>;
    /// Allocate an integer vector from a slice.
    fn alloc_int_from(&mut self, vals: &[i32]) -> Result<SexpId, ValueError>;
    /// Allocate a double vector from a slice.
    fn alloc_real_from(&mut self, vals: &[f64]) -> Result<SexpId, ValueError>;
    /// Allocate a complex vector from a slice.
    fn alloc_cplx_from(&mut self, vals: &[RComplex]) -> Result<SexpId, ValueError>;
    /// Allocate a character vector from [`CharId`] values.
    fn alloc_str_from(&mut self, vals: &[CharId]) -> Result<SexpId, ValueError>;
    /// Allocate a raw vector from bytes.
    fn alloc_raw_from(&mut self, vals: &[u8]) -> Result<SexpId, ValueError>;
    /// Allocate a generic list (`VECSXP`) filled with `SEXP_NIL`.
    fn alloc_list(&mut self, n: usize) -> Result<SexpId, ValueError>;
    /// Allocate an expression vector (`EXPRSXP`) from elements.
    fn alloc_expr_from(&mut self, vals: &[SexpId]) -> Result<SexpId, ValueError>;
    /// Allocate a scalar double.
    fn scalar_real(&mut self, v: f64) -> Result<SexpId, ValueError>;
    /// Allocate a scalar integer.
    fn scalar_int(&mut self, v: i32) -> Result<SexpId, ValueError>;
    /// Allocate a scalar logical.
    fn scalar_lgl(&mut self, v: RLogical) -> Result<SexpId, ValueError>;
    /// Allocate a scalar UTF-8/native string.
    fn scalar_str(&mut self, s: &str) -> Result<SexpId, ValueError>;
    /// Allocate an S4 object (`Body::Obj` with `flags.s4 = true`; ADR-0012).
    fn alloc_s4_object(&mut self) -> Result<SexpId, ValueError>;

    /// Read element `i` of a `VECSXP` or `EXPRSXP`.
    fn list_get(&self, x: SexpId, i: usize) -> Result<SexpId, ValueError>;
    /// Write element `i` of a `VECSXP` or `EXPRSXP` with refcount write barrier.
    fn list_set(&mut self, x: SexpId, i: usize, v: SexpId) -> Result<(), ValueError>;

    /// Allocate a `LISTSXP` cons cell with write barrier.
    fn cons(&mut self, car: SexpId, cdr: SexpId, tag: SexpId) -> Result<SexpId, ValueError>;
    /// Allocate a `LANGSXP` call cell with write barrier.
    fn lcons(&mut self, car: SexpId, cdr: SexpId, tag: SexpId) -> Result<SexpId, ValueError>;
    /// Allocate a `DOTSXP` cell with write barrier.
    fn dcons(&mut self, car: SexpId, cdr: SexpId, tag: SexpId) -> Result<SexpId, ValueError>;
    /// Return `(car, cdr, tag)` of a `LISTSXP`, `LANGSXP`, or `DOTSXP`.
    fn cons_parts(&self, x: SexpId) -> Result<(SexpId, SexpId, SexpId), ValueError>;
    /// Iterate over a pairlist / language call / dots chain.
    fn pairlist_iter(&self, x: SexpId) -> PairlistIter<'_>;

    /// Allocate a `CLOSXP`.
    fn alloc_closure(
        &mut self,
        formals: SexpId,
        body: SexpId,
        env: SexpId,
    ) -> Result<SexpId, ValueError>;
    /// Read `(formals, body, env)` of a `CLOSXP`.
    fn closure_parts(&self, x: SexpId) -> Result<(SexpId, SexpId, SexpId), ValueError>;
    /// Allocate a `BUILTINSXP` or `SPECIALSXP`.
    fn alloc_prim(&mut self, id: PrimId, special: bool) -> Result<SexpId, ValueError>;
    /// Read `(PrimId, is_special)` of a primitive function.
    fn prim_parts(&self, x: SexpId) -> Result<(PrimId, bool), ValueError>;

    /// Allocate a `PROMSXP` in [`PromiseState::Unforced`].
    fn alloc_promise(&mut self, expr: SexpId, env: SexpId) -> Result<SexpId, ValueError>;
    /// Inspect promise fields `(expr, env, value, state)`.
    fn promise_parts(
        &self,
        x: SexpId,
    ) -> Result<(SexpId, SexpId, SexpId, PromiseState), ValueError>;
    /// Update promise state and cached value/env with refcount maintenance.
    fn set_promise_state(
        &mut self,
        x: SexpId,
        state: PromiseState,
        env: SexpId,
        value: SexpId,
    ) -> Result<(), ValueError>;

    /// Allocate a new environment enclosed by `enclos`.
    fn alloc_env(&mut self, enclos: SexpId) -> Result<SexpId, ValueError>;
    /// Return the enclosing environment of `env`.
    fn env_enclos(&self, env: SexpId) -> Result<SexpId, ValueError>;
    /// Look up `sym` in a single frame `env` (does not force promises).
    fn env_get(&self, env: SexpId, sym: Sym) -> Option<Binding>;
    /// Define or update `sym` in `env` with refcount write barrier and lock checks.
    fn env_define(&mut self, env: SexpId, sym: Sym, val: SexpId) -> Result<(), ValueError>;
    /// Define a binding with explicit missingness flag (used for formal argument binding).
    fn env_define_binding(
        &mut self,
        env: SexpId,
        sym: Sym,
        binding: Binding,
    ) -> Result<(), ValueError>;

    /// Look up attribute `sym` on `x` (`SEXP_NIL` when absent).
    fn attr(&self, x: SexpId, sym: Sym) -> SexpId;
    /// Return all attributes in insertion order.
    fn attrs(&self, x: SexpId) -> Result<Vec<(Sym, SexpId)>, ValueError>;
    /// Set or remove (`val == SEXP_NIL`) attribute `sym` on `x`, maintaining the `OBJECT` bit.
    fn set_attrib(&mut self, x: SexpId, sym: Sym, val: SexpId) -> Result<(), ValueError>;

    /// Coerce `x` to atomic vector type `to`.
    fn coerce(&mut self, x: SexpId, to: SexpType) -> Result<(SexpId, CoerceWarn), ValueError>;
}

impl ValueApi for RSession {
    fn type_of(&self, x: SexpId) -> Result<SexpType, ValueError> {
        Ok(self.heap.get(x)?.sexp_type())
    }

    fn is_object(&self, x: SexpId) -> Result<bool, ValueError> {
        Ok(self.heap.get(x)?.flags.object)
    }

    fn is_s4(&self, x: SexpId) -> Result<bool, ValueError> {
        Ok(self.heap.get(x)?.flags.s4)
    }

    fn xlength(&self, x: SexpId) -> Result<usize, ValueError> {
        let target = if x == BASE_NAMESPACE { BASE_ENV } else { x };
        let obj = self.heap.get(target)?;
        Ok(match &obj.body {
            Body::Nil => 0,
            Body::Lgl(v) => v.len(),
            Body::Int(v) => v.len(),
            Body::Real(v) => v.len(),
            Body::Cplx(v) => v.len(),
            Body::Str(v) => v.len(),
            Body::Raw(v) => v.len(),
            Body::Vec(v) | Body::Expr(v) => v.len(),
            Body::Alt(a) => a.len(&self.heap),
            Body::List(_) | Body::Lang(_) | Body::Dots(_) => self.pairlist_iter(x).count(),
            Body::Env(e) => e.entries.len(),
            _ => 1,
        })
    }

    fn lgl(&self, x: SexpId) -> Result<VecView<'_, RLogical>, ValueError> {
        let obj = self.heap.get(x)?;
        match &obj.body {
            Body::Lgl(v) => Ok(VecView::Slice(v.as_slice())),
            Body::Alt(alt) => match (&alt.materialized, &alt.kind) {
                (Some(AltMaterialized::Lgl(v)), _) => Ok(VecView::Slice(v.as_slice())),
                (None, AltKind::Lgl(c)) => Ok(VecView::Alt {
                    class: c.as_ref(),
                    heap: &self.heap,
                    len: c.length(&self.heap),
                }),
                _ => Err(ValueError::TypeMismatch {
                    expected: "logical",
                    actual: obj.sexp_type().type_name(obj.flags.s4),
                }),
            },
            _ => Err(ValueError::TypeMismatch {
                expected: "logical",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn int(&self, x: SexpId) -> Result<VecView<'_, i32>, ValueError> {
        let obj = self.heap.get(x)?;
        match &obj.body {
            Body::Int(v) => Ok(VecView::Slice(v.as_slice())),
            Body::Alt(alt) => match (&alt.materialized, &alt.kind) {
                (Some(AltMaterialized::Int(v)), _) => Ok(VecView::Slice(v.as_slice())),
                (None, AltKind::Int(c)) => Ok(VecView::Alt {
                    class: c.as_ref(),
                    heap: &self.heap,
                    len: c.length(&self.heap),
                }),
                _ => Err(ValueError::TypeMismatch {
                    expected: "integer",
                    actual: obj.sexp_type().type_name(obj.flags.s4),
                }),
            },
            _ => Err(ValueError::TypeMismatch {
                expected: "integer",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn real(&self, x: SexpId) -> Result<VecView<'_, f64>, ValueError> {
        let obj = self.heap.get(x)?;
        match &obj.body {
            Body::Real(v) => Ok(VecView::Slice(v.as_slice())),
            Body::Alt(alt) => match (&alt.materialized, &alt.kind) {
                (Some(AltMaterialized::Real(v)), _) => Ok(VecView::Slice(v.as_slice())),
                (None, AltKind::Real(c)) => Ok(VecView::Alt {
                    class: c.as_ref(),
                    heap: &self.heap,
                    len: c.length(&self.heap),
                }),
                _ => Err(ValueError::TypeMismatch {
                    expected: "double",
                    actual: obj.sexp_type().type_name(obj.flags.s4),
                }),
            },
            _ => Err(ValueError::TypeMismatch {
                expected: "double",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn cplx(&self, x: SexpId) -> Result<VecView<'_, RComplex>, ValueError> {
        let obj = self.heap.get(x)?;
        match &obj.body {
            Body::Cplx(v) => Ok(VecView::Slice(v.as_slice())),
            Body::Alt(alt) => match (&alt.materialized, &alt.kind) {
                (Some(AltMaterialized::Cplx(v)), _) => Ok(VecView::Slice(v.as_slice())),
                (None, AltKind::Cplx(c)) => Ok(VecView::Alt {
                    class: c.as_ref(),
                    heap: &self.heap,
                    len: c.length(&self.heap),
                }),
                _ => Err(ValueError::TypeMismatch {
                    expected: "complex",
                    actual: obj.sexp_type().type_name(obj.flags.s4),
                }),
            },
            _ => Err(ValueError::TypeMismatch {
                expected: "complex",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn raw(&self, x: SexpId) -> Result<VecView<'_, u8>, ValueError> {
        let obj = self.heap.get(x)?;
        match &obj.body {
            Body::Raw(v) => Ok(VecView::Slice(v.as_slice())),
            Body::Alt(alt) => match (&alt.materialized, &alt.kind) {
                (Some(AltMaterialized::Raw(v)), _) => Ok(VecView::Slice(v.as_slice())),
                (None, AltKind::Raw(c)) => Ok(VecView::Alt {
                    class: c.as_ref(),
                    heap: &self.heap,
                    len: c.length(&self.heap),
                }),
                _ => Err(ValueError::TypeMismatch {
                    expected: "raw",
                    actual: obj.sexp_type().type_name(obj.flags.s4),
                }),
            },
            _ => Err(ValueError::TypeMismatch {
                expected: "raw",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn str_slice(&self, x: SexpId) -> Result<&[CharId], ValueError> {
        let obj = self.heap.get(x)?;
        match &obj.body {
            Body::Str(v) => Ok(v.as_slice()),
            Body::Alt(alt) => match &alt.materialized {
                Some(AltMaterialized::Str(v)) => Ok(v.as_slice()),
                _ => Err(ValueError::TypeMismatch {
                    expected: "materialized character",
                    actual: obj.sexp_type().type_name(obj.flags.s4),
                }),
            },
            _ => Err(ValueError::TypeMismatch {
                expected: "character",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn list_slice(&self, x: SexpId) -> Result<&[SexpId], ValueError> {
        let obj = self.heap.get(x)?;
        match &obj.body {
            Body::Vec(v) | Body::Expr(v) => Ok(v.as_slice()),
            _ => Err(ValueError::TypeMismatch {
                expected: "list",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn real_mut(&mut self, x: SexpId) -> Result<&mut [f64], ValueError> {
        self.real_dataptr_mut(x)
    }

    fn int_mut(&mut self, x: SexpId) -> Result<&mut [i32], ValueError> {
        self.int_dataptr_mut(x)
    }

    fn lgl_mut(&mut self, x: SexpId) -> Result<&mut [RLogical], ValueError> {
        self.lgl_dataptr_mut(x)
    }

    fn unshare(&mut self, x: SexpId) -> Result<SexpId, ValueError> {
        if self.heap.maybe_shared(x) {
            return self.shallow_duplicate(x);
        }
        Ok(x)
    }

    fn alloc_lgl_from(&mut self, vals: &[RLogical]) -> Result<SexpId, ValueError> {
        Ok(self
            .heap
            .alloc(RObject::new(Body::Lgl(RVec::from_vec(vals.to_vec()))))?)
    }

    fn alloc_int_from(&mut self, vals: &[i32]) -> Result<SexpId, ValueError> {
        Ok(self
            .heap
            .alloc(RObject::new(Body::Int(RVec::from_vec(vals.to_vec()))))?)
    }

    fn alloc_real_from(&mut self, vals: &[f64]) -> Result<SexpId, ValueError> {
        Ok(self
            .heap
            .alloc(RObject::new(Body::Real(RVec::from_vec(vals.to_vec()))))?)
    }

    fn alloc_cplx_from(&mut self, vals: &[RComplex]) -> Result<SexpId, ValueError> {
        Ok(self
            .heap
            .alloc(RObject::new(Body::Cplx(RVec::from_vec(vals.to_vec()))))?)
    }

    fn alloc_str_from(&mut self, vals: &[CharId]) -> Result<SexpId, ValueError> {
        Ok(self
            .heap
            .alloc(RObject::new(Body::Str(RVec::from_vec(vals.to_vec()))))?)
    }

    fn alloc_raw_from(&mut self, vals: &[u8]) -> Result<SexpId, ValueError> {
        Ok(self
            .heap
            .alloc(RObject::new(Body::Raw(RVec::from_vec(vals.to_vec()))))?)
    }

    fn alloc_list(&mut self, n: usize) -> Result<SexpId, ValueError> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(n)
            .map_err(|_| HeapError::OutOfMemory {
                requested: n.saturating_mul(size_of::<SexpId>()),
            })?;
        values.resize(n, SEXP_NIL);
        Ok(self
            .heap
            .alloc(RObject::new(Body::Vec(RVec::from_vec(values))))?)
    }

    fn alloc_expr_from(&mut self, vals: &[SexpId]) -> Result<SexpId, ValueError> {
        for &v in vals {
            self.heap.inc_ref(v);
        }
        Ok(self
            .heap
            .alloc(RObject::new(Body::Expr(RVec::from_vec(vals.to_vec()))))?)
    }

    fn scalar_real(&mut self, v: f64) -> Result<SexpId, ValueError> {
        self.alloc_real_from(&[v])
    }

    fn scalar_int(&mut self, v: i32) -> Result<SexpId, ValueError> {
        self.alloc_int_from(&[v])
    }

    fn scalar_lgl(&mut self, v: RLogical) -> Result<SexpId, ValueError> {
        self.alloc_lgl_from(&[v])
    }

    fn scalar_str(&mut self, s: &str) -> Result<SexpId, ValueError> {
        let enc = if s.is_ascii() {
            crate::object::Encoding::Native
        } else {
            crate::object::Encoding::Utf8
        };
        let cid = self.mk_char(s.as_bytes(), enc)?;
        self.alloc_str_from(&[cid])
    }

    fn alloc_s4_object(&mut self) -> Result<SexpId, ValueError> {
        let mut obj = RObject::new(Body::Obj);
        obj.flags = ObjFlags {
            object: false,
            s4: true,
        };
        Ok(self.heap.alloc(obj)?)
    }

    fn list_get(&self, x: SexpId, i: usize) -> Result<SexpId, ValueError> {
        let slice = self.list_slice(x)?;
        slice.get(i).copied().ok_or(ValueError::IndexOutOfBounds {
            index: i,
            len: slice.len(),
        })
    }

    fn list_set(&mut self, x: SexpId, i: usize, v: SexpId) -> Result<(), ValueError> {
        let old = {
            let obj = self.heap.get_mut(x)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            let slice = match &mut obj.body {
                Body::Vec(vec) | Body::Expr(vec) => vec.as_mut_slice(),
                _ => {
                    return Err(ValueError::TypeMismatch {
                        expected: "list",
                        actual: ty,
                    });
                }
            };
            let len = slice.len();
            let slot = slice
                .get_mut(i)
                .ok_or(ValueError::IndexOutOfBounds { index: i, len })?;
            let old = *slot;
            *slot = v;
            old
        };
        self.heap.inc_ref(v);
        self.heap.dec_ref(old);
        Ok(())
    }

    fn cons(&mut self, car: SexpId, cdr: SexpId, tag: SexpId) -> Result<SexpId, ValueError> {
        self.heap.inc_ref(car);
        self.heap.inc_ref(cdr);
        self.heap.inc_ref(tag);
        Ok(self
            .heap
            .alloc(RObject::new(Body::List(Cons { car, cdr, tag })))?)
    }

    fn lcons(&mut self, car: SexpId, cdr: SexpId, tag: SexpId) -> Result<SexpId, ValueError> {
        self.heap.inc_ref(car);
        self.heap.inc_ref(cdr);
        self.heap.inc_ref(tag);
        Ok(self
            .heap
            .alloc(RObject::new(Body::Lang(Cons { car, cdr, tag })))?)
    }

    fn dcons(&mut self, car: SexpId, cdr: SexpId, tag: SexpId) -> Result<SexpId, ValueError> {
        self.heap.inc_ref(car);
        self.heap.inc_ref(cdr);
        self.heap.inc_ref(tag);
        Ok(self
            .heap
            .alloc(RObject::new(Body::Dots(Cons { car, cdr, tag })))?)
    }

    fn cons_parts(&self, x: SexpId) -> Result<(SexpId, SexpId, SexpId), ValueError> {
        let obj = self.heap.get(x)?;
        match &obj.body {
            Body::List(c) | Body::Lang(c) | Body::Dots(c) => Ok((c.car, c.cdr, c.tag)),
            _ => Err(ValueError::TypeMismatch {
                expected: "pairlist/language",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn pairlist_iter(&self, x: SexpId) -> PairlistIter<'_> {
        PairlistIter {
            heap: &self.heap,
            cur: x,
        }
    }

    fn alloc_closure(
        &mut self,
        formals: SexpId,
        body: SexpId,
        env: SexpId,
    ) -> Result<SexpId, ValueError> {
        self.heap.inc_ref(formals);
        self.heap.inc_ref(body);
        self.heap.inc_ref(env);
        Ok(self
            .heap
            .alloc(RObject::new(Body::Closure { formals, body, env }))?)
    }

    fn closure_parts(&self, x: SexpId) -> Result<(SexpId, SexpId, SexpId), ValueError> {
        let obj = self.heap.get(x)?;
        match obj.body {
            Body::Closure { formals, body, env } => Ok((formals, body, env)),
            _ => Err(ValueError::TypeMismatch {
                expected: "closure",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn alloc_prim(&mut self, id: PrimId, special: bool) -> Result<SexpId, ValueError> {
        let body = if special {
            Body::Special(id)
        } else {
            Body::Builtin(id)
        };
        Ok(self.heap.alloc_permanent(RObject::new(body))?)
    }

    fn prim_parts(&self, x: SexpId) -> Result<(PrimId, bool), ValueError> {
        let obj = self.heap.get(x)?;
        match obj.body {
            Body::Builtin(id) => Ok((id, false)),
            Body::Special(id) => Ok((id, true)),
            _ => Err(ValueError::TypeMismatch {
                expected: "primitive",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn alloc_promise(&mut self, expr: SexpId, env: SexpId) -> Result<SexpId, ValueError> {
        self.heap.inc_ref(expr);
        self.heap.inc_ref(env);
        Ok(self.heap.alloc(RObject::new(Body::Promise {
            expr,
            env,
            value: crate::session::SEXP_UNBOUND,
            state: PromiseState::Unforced,
        }))?)
    }

    fn promise_parts(
        &self,
        x: SexpId,
    ) -> Result<(SexpId, SexpId, SexpId, PromiseState), ValueError> {
        let obj = self.heap.get(x)?;
        match obj.body {
            Body::Promise {
                expr,
                env,
                value,
                state,
            } => Ok((expr, env, value, state)),
            _ => Err(ValueError::TypeMismatch {
                expected: "promise",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn set_promise_state(
        &mut self,
        x: SexpId,
        new_state: PromiseState,
        new_env: SexpId,
        new_value: SexpId,
    ) -> Result<(), ValueError> {
        let (old_env, old_val) = {
            let obj = self.heap.get_mut(x)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            let Body::Promise {
                env, value, state, ..
            } = &mut obj.body
            else {
                return Err(ValueError::TypeMismatch {
                    expected: "promise",
                    actual: ty,
                });
            };
            let old = (*env, *value);
            *state = new_state;
            *env = new_env;
            *value = new_value;
            old
        };
        self.heap.inc_ref(new_env);
        self.heap.inc_ref(new_value);
        self.heap.dec_ref(old_env);
        self.heap.dec_ref(old_val);
        Ok(())
    }

    fn alloc_env(&mut self, enclos: SexpId) -> Result<SexpId, ValueError> {
        self.heap.inc_ref(enclos);
        Ok(self
            .heap
            .alloc(RObject::new(Body::Env(Box::new(EnvData::new(enclos)))))?)
    }

    fn env_enclos(&self, env: SexpId) -> Result<SexpId, ValueError> {
        let obj = self.heap.get(env)?;
        match &obj.body {
            Body::Env(e) => Ok(e.enclos),
            _ => Err(ValueError::TypeMismatch {
                expected: "environment",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    fn env_get(&self, env: SexpId, sym: Sym) -> Option<Binding> {
        let target = if env == BASE_NAMESPACE { BASE_ENV } else { env };
        let obj = self.heap.get(target).ok()?;
        let Body::Env(e) = &obj.body else {
            return None;
        };
        let &idx = e.index.get(&sym)?;
        Some(e.entries[idx].1)
    }

    fn env_define(&mut self, env: SexpId, sym: Sym, val: SexpId) -> Result<(), ValueError> {
        let target = if env == BASE_NAMESPACE { BASE_ENV } else { env };
        if self
            .env_get(target, sym)
            .is_some_and(|binding| binding.active)
        {
            return Err(ValueError::LockedBinding(self.sym_name(sym)?.to_owned()));
        }
        self.env_define_binding(
            target,
            sym,
            Binding {
                value: val,
                locked: false,
                active: false,
                missing: 0,
            },
        )
    }

    fn env_define_binding(
        &mut self,
        env: SexpId,
        sym: Sym,
        binding: Binding,
    ) -> Result<(), ValueError> {
        let target = if env == BASE_NAMESPACE { BASE_ENV } else { env };
        let sym_name = self.sym_name(sym).unwrap_or("").to_owned();
        let old_val = {
            let obj = self.heap.get_mut(target)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            let Body::Env(e) = &mut obj.body else {
                return Err(ValueError::TypeMismatch {
                    expected: "environment",
                    actual: ty,
                });
            };
            if let Some(&idx) = e.index.get(&sym) {
                if e.entries[idx].1.locked {
                    return Err(ValueError::LockedBinding(sym_name));
                }
                let prev = e.entries[idx].1.value;
                e.entries[idx].1 = binding;
                Some(prev)
            } else {
                if e.locked {
                    return Err(ValueError::LockedBinding(sym_name));
                }
                let idx = e.entries.len();
                e.entries.push((sym, binding));
                e.index.insert(sym, idx);
                None
            }
        };
        self.heap.inc_ref(binding.value);
        if let Some(old) = old_val {
            self.heap.dec_ref(old);
        }
        Ok(())
    }

    fn attr(&self, x: SexpId, sym: Sym) -> SexpId {
        self.heap
            .get(x)
            .ok()
            .and_then(|o| o.attrs.get(sym))
            .unwrap_or(SEXP_NIL)
    }

    fn attrs(&self, x: SexpId) -> Result<Vec<(Sym, SexpId)>, ValueError> {
        Ok(self.heap.get(x)?.attrs.iter().collect())
    }

    fn set_attrib(&mut self, x: SexpId, sym: Sym, val: SexpId) -> Result<(), ValueError> {
        let name = self.sym_name(sym)?;
        let is_class = name == "class";
        let val = if name == "names" && val != SEXP_NIL {
            self.normalize_names(x, val)?
        } else {
            self.heap.get(val)?;
            val
        };
        let old_val = {
            let obj = self.heap.get_mut(x)?;
            let mut entries: Vec<(Sym, SexpId)> = obj.attrs.iter().collect();
            let mut old = None;
            if val == SEXP_NIL {
                if let Some(pos) = entries.iter().position(|(k, _)| *k == sym) {
                    old = Some(entries.remove(pos).1);
                }
            } else if let Some(slot) = entries.iter_mut().find(|(k, _)| *k == sym) {
                old = Some(slot.1);
                slot.1 = val;
            } else {
                entries.push((sym, val));
            }
            if is_class {
                obj.flags.object = val != SEXP_NIL;
            }
            obj.attrs = if entries.is_empty() {
                Attrs(None)
            } else {
                Attrs(Some(entries.into_boxed_slice()))
            };
            old
        };
        self.heap.update_size(x)?;
        if val != SEXP_NIL {
            self.heap.inc_ref(val);
        }
        if let Some(old) = old_val {
            self.heap.dec_ref(old);
        }
        Ok(())
    }

    fn coerce(&mut self, x: SexpId, to: SexpType) -> Result<(SexpId, CoerceWarn), ValueError> {
        crate::coerce::coerce(self, x, to)
    }
}

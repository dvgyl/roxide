//! Safe low-level [`RSession`] accessors supporting `roxide-capi` and `roxide-eval::vm`
//! while keeping `roxide-value` strictly `#![forbid(unsafe_code)]` (ADRs 0017–0020).

use roxide_gc::{FinalizerKind, HeapError, SexpId, WeakRefEntry};

use crate::alt::AltMaterialized;
use crate::api::ValueError;
use crate::na::{RComplex, RLogical};
use crate::object::{Attrs, Body, CharId, ExtPtrData, RObject, RVec, SexpType, Sym, WeakRefData};
use crate::session::{BASE_ENV, BASE_NAMESPACE, RSession, SEXP_NIL};

impl RSession {
    /// Return `(raw_address, element_len)` for read-only direct buffer access (`DATAPTR_RO`, ADR-0017).
    ///
    /// Works for materialized atomic/list/expression vectors and `CHARSXP` (pointing to NUL-terminated bytes).
    /// Returns `Err(ValueError::TypeMismatch)` for unmaterialized ALTREP or non-vector types.
    pub fn data_ptr_ro(&self, x: SexpId) -> Result<(usize, usize), ValueError> {
        let obj = self.heap.get(x)?;
        match &obj.body {
            Body::Lgl(v) => Ok((v.as_slice().as_ptr() as usize, v.len())),
            Body::Int(v) => Ok((v.as_slice().as_ptr() as usize, v.len())),
            Body::Real(v) => Ok((v.as_slice().as_ptr() as usize, v.len())),
            Body::Cplx(v) => Ok((v.as_slice().as_ptr() as usize, v.len())),
            Body::Str(v) => Ok((v.as_slice().as_ptr() as usize, v.len())),
            Body::Raw(v) => Ok((v.as_slice().as_ptr() as usize, v.len())),
            Body::Vec(v) | Body::Expr(v) => Ok((v.as_slice().as_ptr() as usize, v.len())),
            Body::Char(c) => Ok((c.bytes_nul().as_ptr() as usize, c.bytes().len())),
            Body::Alt(alt) => match &alt.materialized {
                Some(AltMaterialized::Lgl(v)) => Ok((v.as_slice().as_ptr() as usize, v.len())),
                Some(AltMaterialized::Int(v)) => Ok((v.as_slice().as_ptr() as usize, v.len())),
                Some(AltMaterialized::Real(v)) => Ok((v.as_slice().as_ptr() as usize, v.len())),
                Some(AltMaterialized::Cplx(v)) => Ok((v.as_slice().as_ptr() as usize, v.len())),
                Some(AltMaterialized::Raw(v)) => Ok((v.as_slice().as_ptr() as usize, v.len())),
                Some(AltMaterialized::Str(v)) => Ok((v.as_slice().as_ptr() as usize, v.len())),
                None => Err(ValueError::TypeMismatch {
                    expected: "materialized vector",
                    actual: obj.sexp_type().type_name(obj.flags.s4),
                }),
            },
            _ => Err(ValueError::TypeMismatch {
                expected: "vector",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    /// Return `(raw_address, element_len)` for writable direct buffer access (`INTEGER`, `REAL`,
    /// `LOGICAL`, `COMPLEX`, `RAW`).
    ///
    /// Strictly restricted to atomic numeric/logical/raw vectors (ADR-0017); `STRSXP`/`VECSXP`/
    /// `EXPRSXP` must be read via [`RSession::data_ptr_ro`] and mutated via `str_set`/`list_set`
    /// so GC refcounts and `CharId` invariants are never bypassed.
    pub fn data_ptr_rw(&mut self, x: SexpId) -> Result<(usize, usize), ValueError> {
        let obj = self.heap.get(x)?;
        if let Body::Alt(alt) = &obj.body
            && alt.materialized.is_none()
        {
            match alt.base_type() {
                SexpType::Lgl => {
                    let s = self.lgl_dataptr_ro(x)?;
                    return Ok((s.as_ptr() as usize, s.len()));
                }
                SexpType::Int => {
                    let s = self.int_dataptr_ro(x)?;
                    return Ok((s.as_ptr() as usize, s.len()));
                }
                SexpType::Real => {
                    let s = self.real_dataptr_ro(x)?;
                    return Ok((s.as_ptr() as usize, s.len()));
                }
                SexpType::Cplx => {
                    let s = self.cplx_dataptr_ro(x)?;
                    return Ok((s.as_ptr() as usize, s.len()));
                }
                SexpType::Raw => {
                    let s = self.raw_dataptr_ro(x)?;
                    return Ok((s.as_ptr() as usize, s.len()));
                }
                _ => {}
            }
        }
        let obj = self.heap.get_mut(x)?;
        match &mut obj.body {
            Body::Lgl(v) => Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len())),
            Body::Int(v) => Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len())),
            Body::Real(v) => Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len())),
            Body::Cplx(v) => Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len())),
            Body::Raw(v) => Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len())),
            Body::Alt(alt) => match &mut alt.materialized {
                Some(AltMaterialized::Lgl(v)) => {
                    Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len()))
                }
                Some(AltMaterialized::Int(v)) => {
                    Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len()))
                }
                Some(AltMaterialized::Real(v)) => {
                    Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len()))
                }
                Some(AltMaterialized::Cplx(v)) => {
                    Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len()))
                }
                Some(AltMaterialized::Raw(v)) => {
                    Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len()))
                }
                _ => Err(ValueError::TypeMismatch {
                    expected: "atomic vector",
                    actual: obj.sexp_type().type_name(obj.flags.s4),
                }),
            },
            _ => Err(ValueError::TypeMismatch {
                expected: "atomic vector",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    /// Return `(cstr_ptr_addr, byte_len)` for a `CHARSXP` (`R_CHAR()`).
    pub fn char_cstr_ptr(&self, c: CharId) -> Result<(usize, usize), ValueError> {
        let obj = self.heap.get(c.as_sexp())?;
        let Body::Char(rc) = &obj.body else {
            return Err(ValueError::TypeMismatch {
                expected: "char",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            });
        };
        Ok((rc.bytes_nul().as_ptr() as usize, rc.bytes().len()))
    }

    /// Allocate a resizable vector with logical length `len` and pinned capacity `max_len`
    /// (`R_allocResizableVector`, R 4.6.2).
    pub fn alloc_resizable(
        &mut self,
        ty: SexpType,
        len: usize,
        max_len: usize,
    ) -> Result<SexpId, ValueError> {
        if len > max_len {
            return Err(ValueError::InvalidAttribute(
                "resizable vector length exceeds max_len".into(),
            ));
        }
        fn alloc_boxed<T: Clone>(n: usize, fill: T) -> Result<Box<[T]>, ValueError> {
            let mut v = Vec::new();
            v.try_reserve_exact(n).map_err(|_| HeapError::OutOfMemory {
                requested: n.saturating_mul(std::mem::size_of::<T>()),
            })?;
            v.resize(n, fill);
            Ok(v.into_boxed_slice())
        }
        let body = match ty {
            SexpType::Lgl => {
                Body::Lgl(RVec::resizable(alloc_boxed(max_len, RLogical::FALSE)?, len))
            }
            SexpType::Int => Body::Int(RVec::resizable(alloc_boxed(max_len, 0i32)?, len)),
            SexpType::Real => Body::Real(RVec::resizable(alloc_boxed(max_len, 0.0f64)?, len)),
            SexpType::Cplx => Body::Cplx(RVec::resizable(
                alloc_boxed(max_len, RComplex { re: 0.0, im: 0.0 })?,
                len,
            )),
            SexpType::Raw => Body::Raw(RVec::resizable(alloc_boxed(max_len, 0u8)?, len)),
            SexpType::Str => Body::Str(RVec::resizable(alloc_boxed(max_len, CharId::BLANK)?, len)),
            SexpType::Vec => Body::Vec(RVec::resizable(alloc_boxed(max_len, SEXP_NIL)?, len)),
            SexpType::Expr => Body::Expr(RVec::resizable(alloc_boxed(max_len, SEXP_NIL)?, len)),
            other => {
                return Err(ValueError::TypeMismatch {
                    expected: "resizable vector type",
                    actual: other.type_name(false),
                });
            }
        };
        Ok(self.heap.alloc(RObject::new(body))?)
    }

    /// Return true iff `x` was allocated via [`RSession::alloc_resizable`].
    pub fn is_resizable(&self, x: SexpId) -> Result<bool, ValueError> {
        let obj = self.heap.get(x)?;
        Ok(match &obj.body {
            Body::Lgl(v) => v.is_resizable(),
            Body::Int(v) => v.is_resizable(),
            Body::Real(v) => v.is_resizable(),
            Body::Cplx(v) => v.is_resizable(),
            Body::Raw(v) => v.is_resizable(),
            Body::Str(v) => v.is_resizable(),
            Body::Vec(v) | Body::Expr(v) => v.is_resizable(),
            _ => false,
        })
    }

    /// Return the allocated backing capacity (`R_maxLength` / `TRUELENGTH`) of a vector.
    pub fn vec_truelength(&self, x: SexpId) -> Result<usize, ValueError> {
        let obj = self.heap.get(x)?;
        Ok(match &obj.body {
            Body::Lgl(v) => v.capacity(),
            Body::Int(v) => v.capacity(),
            Body::Real(v) => v.capacity(),
            Body::Cplx(v) => v.capacity(),
            Body::Raw(v) => v.capacity(),
            Body::Str(v) => v.capacity(),
            Body::Vec(v) | Body::Expr(v) => v.capacity(),
            _ => 0,
        })
    }

    /// Resize a resizable vector in-place within its pinned capacity (`R_resizeVector`).
    pub fn resize_vector(&mut self, x: SexpId, new_len: usize) -> Result<(), ValueError> {
        let mut dropped_sexps: Vec<SexpId> = Vec::new();
        {
            let obj = self.heap.get_mut(x)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            let check_and_grow = |resizable: bool, cap: usize| -> Result<(), ValueError> {
                if !resizable {
                    return Err(ValueError::InvalidAttribute(
                        "vector is not resizable".into(),
                    ));
                }
                if new_len > cap {
                    return Err(ValueError::IndexOutOfBounds {
                        index: new_len,
                        len: cap,
                    });
                }
                Ok(())
            };
            match &mut obj.body {
                Body::Lgl(v) => {
                    check_and_grow(v.resizable, v.buf.len())?;
                    if new_len > v.len {
                        v.buf[v.len..new_len].fill(RLogical::FALSE);
                    }
                    v.len = new_len;
                }
                Body::Int(v) => {
                    check_and_grow(v.resizable, v.buf.len())?;
                    if new_len > v.len {
                        v.buf[v.len..new_len].fill(0);
                    }
                    v.len = new_len;
                }
                Body::Real(v) => {
                    check_and_grow(v.resizable, v.buf.len())?;
                    if new_len > v.len {
                        v.buf[v.len..new_len].fill(0.0);
                    }
                    v.len = new_len;
                }
                Body::Cplx(v) => {
                    check_and_grow(v.resizable, v.buf.len())?;
                    if new_len > v.len {
                        v.buf[v.len..new_len].fill(RComplex { re: 0.0, im: 0.0 });
                    }
                    v.len = new_len;
                }
                Body::Raw(v) => {
                    check_and_grow(v.resizable, v.buf.len())?;
                    if new_len > v.len {
                        v.buf[v.len..new_len].fill(0);
                    }
                    v.len = new_len;
                }
                Body::Str(v) => {
                    check_and_grow(v.resizable, v.buf.len())?;
                    if new_len > v.len {
                        v.buf[v.len..new_len].fill(CharId::BLANK);
                    } else if new_len < v.len {
                        v.buf[new_len..v.len].fill(CharId::BLANK);
                    }
                    v.len = new_len;
                }
                Body::Vec(v) | Body::Expr(v) => {
                    check_and_grow(v.resizable, v.buf.len())?;
                    if new_len > v.len {
                        v.buf[v.len..new_len].fill(SEXP_NIL);
                    } else if new_len < v.len {
                        dropped_sexps.extend_from_slice(&v.buf[new_len..v.len]);
                        v.buf[new_len..v.len].fill(SEXP_NIL);
                    }
                    v.len = new_len;
                }
                _ => {
                    return Err(ValueError::TypeMismatch {
                        expected: "resizable vector",
                        actual: ty,
                    });
                }
            }
        }
        for old in dropped_sexps {
            self.heap.dec_ref(old);
        }
        Ok(())
    }

    /// Allocate an external pointer (`EXTPTRSXP`, `R_MakeExternalPtr`).
    pub fn alloc_extptr(
        &mut self,
        addr: usize,
        tag: SexpId,
        prot: SexpId,
    ) -> Result<SexpId, ValueError> {
        self.heap.inc_ref(tag);
        self.heap.inc_ref(prot);
        Ok(self
            .heap
            .alloc(RObject::new(Body::ExtPtr(Box::new(ExtPtrData {
                addr,
                rust: None,
                tag,
                prot,
            }))))?)
    }

    /// Read the raw address of an `EXTPTRSXP` (`R_ExternalPtrAddr`).
    pub fn extptr_addr(&self, x: SexpId) -> Result<usize, ValueError> {
        let obj = self.heap.get(x)?;
        let Body::ExtPtr(ep) = &obj.body else {
            return Err(ValueError::TypeMismatch {
                expected: "externalptr",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            });
        };
        Ok(ep.addr)
    }

    /// Update the raw address of an `EXTPTRSXP` (`R_SetExternalPtrAddr` / `R_ClearExternalPtr`).
    pub fn set_extptr_addr(&mut self, x: SexpId, addr: usize) -> Result<(), ValueError> {
        let obj = self.heap.get_mut(x)?;
        let ty = obj.sexp_type().type_name(obj.flags.s4);
        let Body::ExtPtr(ep) = &mut obj.body else {
            return Err(ValueError::TypeMismatch {
                expected: "externalptr",
                actual: ty,
            });
        };
        ep.addr = addr;
        Ok(())
    }

    /// Read the tag `SEXP` of an `EXTPTRSXP` (`R_ExternalPtrTag`).
    pub fn extptr_tag(&self, x: SexpId) -> Result<SexpId, ValueError> {
        let obj = self.heap.get(x)?;
        let Body::ExtPtr(ep) = &obj.body else {
            return Err(ValueError::TypeMismatch {
                expected: "externalptr",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            });
        };
        Ok(ep.tag)
    }

    /// Update the tag `SEXP` of an `EXTPTRSXP` (`R_SetExternalPtrTag`).
    pub fn set_extptr_tag(&mut self, x: SexpId, tag: SexpId) -> Result<(), ValueError> {
        let old = {
            let obj = self.heap.get_mut(x)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            let Body::ExtPtr(ep) = &mut obj.body else {
                return Err(ValueError::TypeMismatch {
                    expected: "externalptr",
                    actual: ty,
                });
            };
            std::mem::replace(&mut ep.tag, tag)
        };
        self.heap.inc_ref(tag);
        self.heap.dec_ref(old);
        Ok(())
    }

    /// Read the protected `SEXP` of an `EXTPTRSXP` (`R_ExternalPtrProtected`).
    pub fn extptr_prot(&self, x: SexpId) -> Result<SexpId, ValueError> {
        let obj = self.heap.get(x)?;
        let Body::ExtPtr(ep) = &obj.body else {
            return Err(ValueError::TypeMismatch {
                expected: "externalptr",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            });
        };
        Ok(ep.prot)
    }

    /// Update the protected `SEXP` of an `EXTPTRSXP` (`R_SetExternalPtrProtected`).
    pub fn set_extptr_prot(&mut self, x: SexpId, prot: SexpId) -> Result<(), ValueError> {
        let old = {
            let obj = self.heap.get_mut(x)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            let Body::ExtPtr(ep) = &mut obj.body else {
                return Err(ValueError::TypeMismatch {
                    expected: "externalptr",
                    actual: ty,
                });
            };
            std::mem::replace(&mut ep.prot, prot)
        };
        self.heap.inc_ref(prot);
        self.heap.dec_ref(old);
        Ok(())
    }

    /// Allocate an ephemeron weak reference (`WEAKREFSXP`, `R_MakeWeakRef` / `R_MakeWeakRefC`).
    ///
    /// Matches GNU R: `key` must be `NULL`, an environment, or an external pointer. Weakref
    /// entries remain tracked globally so registered finalizers still run when `key` becomes
    /// unreachable even if the `WEAKREFSXP` handle itself is no longer referenced.
    pub fn alloc_weakref(
        &mut self,
        key: SexpId,
        value: SexpId,
        finalizer: Option<FinalizerKind>,
        onexit: bool,
    ) -> Result<SexpId, ValueError> {
        let key_obj = self.heap.get(key)?;
        if !matches!(
            key_obj.sexp_type(),
            SexpType::Nil | SexpType::Env | SexpType::ExtPtr
        ) {
            return Err(ValueError::TypeMismatch {
                expected: "NULL, environment, or externalptr",
                actual: key_obj.sexp_type().type_name(key_obj.flags.s4),
            });
        }
        let fin_sexp = match finalizer {
            Some(FinalizerKind::RClosure(f)) => f,
            _ => SEXP_NIL,
        };
        let wr = self
            .heap
            .alloc(RObject::new(Body::WeakRef(Box::new(WeakRefData {
                key,
                value,
                finalizer: fin_sexp,
            }))))?;
        if key != SEXP_NIL {
            self.heap.register_weakref(WeakRefEntry {
                weakref: wr,
                key,
                value,
                finalizer,
                onexit,
            });
        }
        Ok(wr)
    }

    /// Read the key of a `WEAKREFSXP` (`R_WeakRefKey`; `SEXP_NIL` once cleared).
    pub fn weakref_key(&self, x: SexpId) -> Result<SexpId, ValueError> {
        let obj = self.heap.get(x)?;
        let Body::WeakRef(wr) = &obj.body else {
            return Err(ValueError::TypeMismatch {
                expected: "weakref",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            });
        };
        Ok(wr.key)
    }

    /// Read the value of a `WEAKREFSXP` (`R_WeakRefValue`; `SEXP_NIL` once cleared).
    pub fn weakref_value(&self, x: SexpId) -> Result<SexpId, ValueError> {
        let obj = self.heap.get(x)?;
        let Body::WeakRef(wr) = &obj.body else {
            return Err(ValueError::TypeMismatch {
                expected: "weakref",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            });
        };
        Ok(wr.value)
    }

    /// Read the general-purpose 16-bit header field (`LEVELS(x)`).
    pub fn gp(&self, x: SexpId) -> Result<u16, ValueError> {
        Ok(self.heap.get(x)?.gp)
    }

    /// Write the general-purpose 16-bit header field (`SETLEVELS(x, v)`).
    pub fn set_gp(&mut self, x: SexpId, gp: u16) -> Result<(), ValueError> {
        self.heap.get_mut(x)?.gp = gp;
        Ok(())
    }

    /// Set or clear the S3 object bit directly (`SET_OBJECT`).
    pub fn set_object_flag(&mut self, x: SexpId, object: bool) -> Result<(), ValueError> {
        self.heap.get_mut(x)?.flags.object = object;
        Ok(())
    }

    /// Return the monotonic binding shape version of an `ENVSXP` (ADR-0020).
    pub fn env_shape_version(&self, env: SexpId) -> Result<u32, ValueError> {
        let target = if env == BASE_NAMESPACE { BASE_ENV } else { env };
        let obj = self.heap.get(target)?;
        match &obj.body {
            Body::Env(e) => Ok(e.shape_version),
            _ => Err(ValueError::TypeMismatch {
                expected: "environment",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    /// Mutate `CAR` of a cons cell (`SETCAR`).
    pub fn set_car(&mut self, x: SexpId, car: SexpId) -> Result<(), ValueError> {
        let old = {
            let obj = self.heap.get_mut(x)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            match &mut obj.body {
                Body::List(c) | Body::Lang(c) | Body::Dots(c) => std::mem::replace(&mut c.car, car),
                _ => {
                    return Err(ValueError::TypeMismatch {
                        expected: "pairlist/language",
                        actual: ty,
                    });
                }
            }
        };
        self.heap.inc_ref(car);
        self.heap.dec_ref(old);
        Ok(())
    }

    /// Mutate `CDR` of a cons cell (`SETCDR`).
    pub fn set_cdr(&mut self, x: SexpId, cdr: SexpId) -> Result<(), ValueError> {
        let old = {
            let obj = self.heap.get_mut(x)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            match &mut obj.body {
                Body::List(c) | Body::Lang(c) | Body::Dots(c) => std::mem::replace(&mut c.cdr, cdr),
                _ => {
                    return Err(ValueError::TypeMismatch {
                        expected: "pairlist/language",
                        actual: ty,
                    });
                }
            }
        };
        self.heap.inc_ref(cdr);
        self.heap.dec_ref(old);
        Ok(())
    }

    /// Mutate `TAG` of a cons cell (`SET_TAG`).
    pub fn set_tag(&mut self, x: SexpId, tag: SexpId) -> Result<(), ValueError> {
        let old = {
            let obj = self.heap.get_mut(x)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            match &mut obj.body {
                Body::List(c) | Body::Lang(c) | Body::Dots(c) => std::mem::replace(&mut c.tag, tag),
                _ => {
                    return Err(ValueError::TypeMismatch {
                        expected: "pairlist/language",
                        actual: ty,
                    });
                }
            }
        };
        self.heap.inc_ref(tag);
        self.heap.dec_ref(old);
        Ok(())
    }

    /// Mutate a closure's formals (`SET_FORMALS`).
    pub fn set_closure_formals(&mut self, x: SexpId, formals: SexpId) -> Result<(), ValueError> {
        let old = {
            let obj = self.heap.get_mut(x)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            let Body::Closure { formals: f, .. } = &mut obj.body else {
                return Err(ValueError::TypeMismatch {
                    expected: "closure",
                    actual: ty,
                });
            };
            std::mem::replace(f, formals)
        };
        self.heap.inc_ref(formals);
        self.heap.dec_ref(old);
        Ok(())
    }

    /// Mutate a closure's body (`SET_BODY`).
    pub fn set_closure_body(&mut self, x: SexpId, body: SexpId) -> Result<(), ValueError> {
        let old = {
            let obj = self.heap.get_mut(x)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            let Body::Closure { body: b, .. } = &mut obj.body else {
                return Err(ValueError::TypeMismatch {
                    expected: "closure",
                    actual: ty,
                });
            };
            std::mem::replace(b, body)
        };
        self.heap.inc_ref(body);
        self.heap.dec_ref(old);
        Ok(())
    }

    /// Mutate a closure's enclosing environment (`SET_CLOENV`).
    pub fn set_closure_env(&mut self, x: SexpId, env: SexpId) -> Result<(), ValueError> {
        let old = {
            let obj = self.heap.get_mut(x)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            let Body::Closure { env: e, .. } = &mut obj.body else {
                return Err(ValueError::TypeMismatch {
                    expected: "closure",
                    actual: ty,
                });
            };
            std::mem::replace(e, env)
        };
        self.heap.inc_ref(env);
        self.heap.dec_ref(old);
        Ok(())
    }

    /// Validate that `x` is a `SYMSXP` and return it as a typed [`Sym`] without re-interning.
    pub fn sexp_as_sym(&self, x: SexpId) -> Result<Sym, ValueError> {
        let obj = self.heap.get(x)?;
        match obj.body {
            Body::Sym { .. } => Ok(Sym(x)),
            _ => Err(ValueError::TypeMismatch {
                expected: "symbol",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    /// Reset an environment frame to empty unlocked state with a new parent (`METHODS_NAMESPACE` reload).
    pub fn env_reset(&mut self, env: SexpId, enclos: SexpId) -> Result<(), ValueError> {
        let (old_enclos, old_entries, old_attrs) = {
            let obj = self.heap.get_mut(env)?;
            let ty = obj.sexp_type().type_name(obj.flags.s4);
            let old_attrs: Vec<SexpId> = obj.attrs.iter().map(|(_, v)| v).collect();
            let Body::Env(frame) = &mut obj.body else {
                return Err(ValueError::TypeMismatch {
                    expected: "environment",
                    actual: ty,
                });
            };
            let old_enclos = std::mem::replace(&mut frame.enclos, enclos);
            let old_entries = std::mem::take(&mut frame.entries);
            frame.index.clear();
            frame.locked = false;
            frame.shape_version = frame.shape_version.wrapping_add(1);
            obj.attrs = Attrs::default();
            obj.flags.object = false;
            obj.flags.s4 = false;
            (old_enclos, old_entries, old_attrs)
        };
        self.heap.inc_ref(enclos);
        self.heap.dec_ref(old_enclos);
        for (_, b) in old_entries {
            self.heap.dec_ref(b.value);
        }
        for v in old_attrs {
            self.heap.dec_ref(v);
        }
        self.heap.update_size(env)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use roxide_gc::{GcPolicy, RootSet, Tracer};

    use super::*;
    use crate::api::ValueApi;
    use crate::session::{
        BLANK_SCALAR_STRING, EMPTY_ENV, METHODS_NAMESPACE, NAMESPACE_REGISTRY, SYM_BASE, SYM_CLASS,
        SYM_DOTS,
    };

    struct EmptyRoots;
    impl RootSet for EmptyRoots {
        fn trace_roots(&self, _t: &mut Tracer<'_>) {}
    }

    #[test]
    fn permanent_capi_singletons_and_symbols() {
        let mut s = RSession::new(GcPolicy::default()).unwrap();
        assert_eq!(s.type_of(BLANK_SCALAR_STRING).unwrap(), SexpType::Str);
        assert_eq!(s.xlength(BLANK_SCALAR_STRING).unwrap(), 1);
        assert_eq!(s.type_of(NAMESPACE_REGISTRY).unwrap(), SexpType::Env);
        assert_eq!(s.type_of(METHODS_NAMESPACE).unwrap(), SexpType::Env);
        assert_eq!(
            s.env_get(NAMESPACE_REGISTRY, SYM_BASE).unwrap().value,
            BASE_NAMESPACE
        );
        assert_eq!(s.intern("class").unwrap(), SYM_CLASS);
        assert_eq!(s.intern("...").unwrap(), SYM_DOTS);
        assert_eq!(s.intern("base").unwrap(), SYM_BASE);
    }

    #[test]
    fn resizable_vector_keeps_pinned_address() {
        let mut s = RSession::new(GcPolicy::default()).unwrap();
        let v = s.alloc_resizable(SexpType::Int, 2, 8).unwrap();
        assert!(s.is_resizable(v).unwrap());
        assert_eq!(s.xlength(v).unwrap(), 2);
        assert_eq!(s.vec_truelength(v).unwrap(), 8);
        let (p0, l0) = s.data_ptr_ro(v).unwrap();
        assert_eq!(l0, 2);
        s.resize_vector(v, 5).unwrap();
        let (p1, l1) = s.data_ptr_rw(v).unwrap();
        assert_eq!(p0, p1);
        assert_eq!(l1, 5);
        assert!(s.resize_vector(v, 9).is_err());
    }

    #[test]
    fn extptr_weakref_and_env_shape_version() {
        let mut s = RSession::new(GcPolicy::Torture).unwrap();
        let env = s.alloc_env(EMPTY_ENV).unwrap();
        let v0 = s.env_shape_version(env).unwrap();
        let k = s.intern("x").unwrap();
        let val = s.scalar_int(42).unwrap();
        s.env_define(env, k, val).unwrap();
        let v1 = s.env_shape_version(env).unwrap();
        assert_eq!(v1, v0 + 1);
        // Overwriting existing binding preserves shape version.
        s.env_define(env, k, val).unwrap();
        assert_eq!(s.env_shape_version(env).unwrap(), v1);
        // Removing binding bumps shape version.
        assert!(s.env_remove(env, k).unwrap());
        assert_eq!(s.env_shape_version(env).unwrap(), v1 + 1);

        let ep = s.alloc_extptr(0x1234, SYM_CLASS.as_sexp(), env).unwrap();
        assert_eq!(s.extptr_addr(ep).unwrap(), 0x1234);
        assert_eq!(s.extptr_tag(ep).unwrap(), SYM_CLASS.as_sexp());
        assert_eq!(s.extptr_prot(ep).unwrap(), env);
        s.set_extptr_addr(ep, 0).unwrap();
        assert_eq!(s.extptr_addr(ep).unwrap(), 0);

        let key = s.alloc_env(EMPTY_ENV).unwrap();
        let wval = s.scalar_int(7).unwrap();
        let wr = s.alloc_weakref(key, wval, None, false).unwrap();
        let m = s.heap.protect_mark();
        s.heap.protect(wr);
        s.heap.protect(key);
        s.collect(&EmptyRoots);
        assert_eq!(s.weakref_key(wr).unwrap(), key);
        assert_eq!(s.weakref_value(wr).unwrap(), wval);
        // Drop key while keeping weakref live: key/value clear to SEXP_NIL.
        s.heap.unprotect(1).unwrap();
        s.collect(&EmptyRoots);
        assert_eq!(s.weakref_key(wr).unwrap(), SEXP_NIL);
        assert_eq!(s.weakref_value(wr).unwrap(), SEXP_NIL);
        s.heap.unprotect_to(m);
    }
}

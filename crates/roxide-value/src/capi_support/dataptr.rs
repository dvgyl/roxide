use roxide_gc::{HeapError, SexpId};

use crate::alt::{AltKind, AltMaterialized, Sortedness};
use crate::api::ValueError;
use crate::na::{RComplex, RLogical};
use crate::object::{Body, CharId, RObject, RVec, SexpType};
use crate::session::{RSession, SEXP_NIL};

impl RSession {
    /// Return `(raw_address, element_len)` for read-only direct buffer access (`DATAPTR_RO`, ADR-0017).
    ///
    /// Works for materialized atomic/list/expression vectors and `CHARSXP` (pointing to NUL-terminated bytes).
    /// A lazy foreign ALTREP vector requires a payload before this call can return a pointer.
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
                Some(AltMaterialized::List(v)) => Ok((v.as_slice().as_ptr() as usize, v.len())),
                None => {
                    alt.require_payload(x)?;
                    Err(obj.type_mismatch("materialized vector"))
                }
            },
            _ => Err(obj.type_mismatch("vector")),
        }
    }

    /// Return `(raw_address, element_len)` for writable direct buffer access (`INTEGER`, `REAL`,
    /// `LOGICAL`, `COMPLEX`, `RAW`).
    ///
    /// This also exposes owned foreign ALTREP buffers. Use `str_set` and `list_set` to
    /// change character and list elements so the heap can update reference counts.
    pub fn data_ptr_rw(&mut self, x: SexpId) -> Result<(usize, usize), ValueError> {
        let obj = self.heap.get(x)?;
        if let Body::Alt(alt) = &obj.body
            && alt.materialized.is_none()
        {
            alt.require_payload(x)?;
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
        let mismatch = obj.type_mismatch("atomic vector");
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
                Some(AltMaterialized::Str(v)) if matches!(alt.kind, AltKind::Foreign(_)) => {
                    Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len()))
                }
                Some(AltMaterialized::List(v)) => {
                    Ok((v.as_mut_slice().as_mut_ptr() as usize, v.len()))
                }
                _ => Err(mismatch),
            },
            _ => Err(mismatch),
        }
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
            let mismatch = obj.type_mismatch("resizable vector");
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
                        dropped_sexps.extend(v.buf[new_len..v.len].iter().map(|c| c.as_sexp()));
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
                    return Err(mismatch);
                }
            }
        }
        for old in dropped_sexps {
            self.heap.dec_ref(old);
        }
        Ok(())
    }

    /// Return known ALTREP sortedness metadata (`*_IS_SORTED`; `Sortedness::Unknown` for non-ALTREP).
    pub fn alt_is_sorted(&self, x: SexpId) -> Result<Sortedness, ValueError> {
        let obj = self.heap.get(x)?;
        Ok(match &obj.body {
            Body::Alt(alt) if alt.materialized.is_none() => match &alt.kind {
                AltKind::Lgl(c) => c.is_sorted(&self.heap),
                AltKind::Int(c) => c.is_sorted(&self.heap),
                AltKind::Real(c) => c.is_sorted(&self.heap),
                AltKind::Cplx(c) => c.is_sorted(&self.heap),
                AltKind::Raw(c) => c.is_sorted(&self.heap),
                AltKind::Str(c) => c.is_sorted(),
                AltKind::Foreign(_) => Sortedness::Unknown,
            },
            _ => Sortedness::Unknown,
        })
    }

    /// Return known ALTREP no-NA guarantee (`*_NO_NA`; `false` for non-ALTREP).
    pub fn alt_no_na(&self, x: SexpId) -> Result<bool, ValueError> {
        let obj = self.heap.get(x)?;
        Ok(match &obj.body {
            Body::Alt(alt) if alt.materialized.is_none() => match &alt.kind {
                AltKind::Lgl(c) => c.no_na(&self.heap),
                AltKind::Int(c) => c.no_na(&self.heap),
                AltKind::Real(c) => c.no_na(&self.heap),
                AltKind::Cplx(c) => c.no_na(&self.heap),
                AltKind::Raw(c) => c.no_na(&self.heap),
                AltKind::Str(c) => c.no_na(),
                AltKind::Foreign(_) => false,
            },
            _ => false,
        })
    }
}

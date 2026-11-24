use std::rc::Rc;

use roxide_gc::{HeapError, SexpId};

use crate::alt::{AltKind, AltMaterialized};
use crate::api::ValueError;
use crate::object::{Body, CharId, RObject, RVec};
use crate::session::RSession;

impl RSession {
    /// Return `(cstr_ptr_addr, byte_len)` for a `CHARSXP` (`R_CHAR()`).
    pub fn char_cstr_ptr(&self, c: CharId) -> Result<(usize, usize), ValueError> {
        let obj = self.heap.get(c.as_sexp())?;
        let Body::Char(rc) = &obj.body else {
            return Err(obj.type_mismatch("char"));
        };
        Ok((rc.bytes_nul().as_ptr() as usize, rc.bytes().len()))
    }

    /// Validate that `x` is a `CHARSXP` and return it as a typed [`CharId`].
    pub fn sexp_as_char(&self, x: SexpId) -> Result<CharId, ValueError> {
        let obj = self.heap.get(x)?;
        match obj.body {
            Body::Char(_) => Ok(CharId(x)),
            _ => Err(obj.type_mismatch("char")),
        }
    }

    /// Read character element `i` from a dense or `AltString` `STRSXP` (`STRING_ELT`).
    pub fn str_get(&mut self, x: SexpId, i: usize) -> Result<CharId, ValueError> {
        let alt_class = {
            let obj = self.heap.get(x)?;
            match &obj.body {
                Body::Str(v) => {
                    let slice = v.as_slice();
                    return slice.get(i).copied().ok_or(ValueError::IndexOutOfBounds {
                        index: i,
                        len: slice.len(),
                    });
                }
                Body::Alt(alt) => match (&alt.materialized, &alt.kind) {
                    (Some(AltMaterialized::Str(v)), _) => {
                        let slice = v.as_slice();
                        return slice.get(i).copied().ok_or(ValueError::IndexOutOfBounds {
                            index: i,
                            len: slice.len(),
                        });
                    }
                    (None, AltKind::Str(c)) => Rc::clone(c),
                    _ => {
                        return Err(obj.type_mismatch("character"));
                    }
                },
                _ => {
                    return Err(obj.type_mismatch("character"));
                }
            }
        };
        alt_class.elt(self, i)
    }

    /// Write character element `i` in a dense or `AltString` `STRSXP` (`SET_STRING_ELT`),
    /// maintaining `CHARSXP` reference counts cleanly.
    pub fn str_set(&mut self, x: SexpId, i: usize, val: CharId) -> Result<(), ValueError> {
        self.sexp_as_char(val.as_sexp())?;
        let alt_class = {
            let obj = self.heap.get(x)?;
            if let Body::Alt(alt) = &obj.body
                && alt.materialized.is_none()
            {
                if let AltKind::Str(c) = &alt.kind {
                    Some(Rc::clone(c))
                } else {
                    return Err(obj.type_mismatch("character"));
                }
            } else {
                None
            }
        };
        if let Some(class) = alt_class {
            return class.set_elt(self, i, val);
        }
        let old = {
            let obj = self.heap.get_mut(x)?;
            let mismatch = obj.type_mismatch("character");
            let slice = match &mut obj.body {
                Body::Str(v) => v.as_mut_slice(),
                Body::Alt(alt) => match &mut alt.materialized {
                    Some(AltMaterialized::Str(v)) => v.as_mut_slice(),
                    _ => {
                        return Err(mismatch);
                    }
                },
                _ => {
                    return Err(mismatch);
                }
            };
            let len = slice.len();
            let slot = slice
                .get_mut(i)
                .ok_or(ValueError::IndexOutOfBounds { index: i, len })?;
            std::mem::replace(slot, val)
        };
        self.heap.inc_ref(val.as_sexp());
        self.heap.dec_ref(old.as_sexp());
        Ok(())
    }

    /// Return a contiguous read-only `&[CharId]` slice for a `STRSXP` (`STRING_PTR_RO`),
    /// materializing `AltString` vectors on demand into a pinned buffer.
    pub fn str_dataptr_ro(&mut self, x: SexpId) -> Result<&[CharId], ValueError> {
        let to_materialize = {
            let obj = self.heap.get(x)?;
            match &obj.body {
                Body::Str(_) => None,
                Body::Alt(alt) => match (&alt.materialized, &alt.kind) {
                    (Some(AltMaterialized::Str(_)), _) => None,
                    (None, AltKind::Str(c)) => Some(Rc::clone(c)),
                    _ => {
                        return Err(obj.type_mismatch("character"));
                    }
                },
                _ => {
                    return Err(obj.type_mismatch("character"));
                }
            }
        };
        if let Some(class) = to_materialize {
            let len = class.length(&self.heap);
            let mut buf = Vec::new();
            buf.try_reserve_exact(len)
                .map_err(|_| HeapError::OutOfMemory {
                    requested: len.saturating_mul(std::mem::size_of::<CharId>()),
                })?;
            buf.resize(len, CharId::BLANK);
            let mark = self.heap.protect_mark();
            self.heap.protect(x);
            let tmp = match self
                .heap
                .alloc(RObject::new(Body::Str(RVec::from_vec(buf))))
            {
                Ok(id) => id,
                Err(err) => {
                    self.heap.unprotect_to(mark);
                    return Err(err.into());
                }
            };
            self.heap.protect(tmp);
            for i in 0..len {
                let cid = match class.elt(self, i) {
                    Ok(c) => c,
                    Err(err) => {
                        self.heap.unprotect_to(mark);
                        return Err(err);
                    }
                };
                if let Body::Str(v) = &mut self.heap.get_mut(tmp)?.body {
                    v.as_mut_slice()[i] = cid;
                }
            }
            let rvec = match std::mem::replace(&mut self.heap.get_mut(tmp)?.body, Body::Nil) {
                Body::Str(v) => v,
                _ => RVec::from_vec(Vec::new()),
            };
            let _ = self.heap.update_size(tmp);
            self.heap.unprotect_to(mark);
            if let Body::Alt(alt) = &mut self.heap.get_mut(x)?.body
                && alt.materialized.is_none()
            {
                alt.materialized = Some(AltMaterialized::Str(rvec));
            }
            self.heap.update_size(x)?;
        }
        let obj = self.heap.get(x)?;
        match &obj.body {
            Body::Str(v) => Ok(v.as_slice()),
            Body::Alt(alt) => match &alt.materialized {
                Some(AltMaterialized::Str(v)) => Ok(v.as_slice()),
                _ => Err(obj.type_mismatch("character")),
            },
            _ => Err(obj.type_mismatch("character")),
        }
    }
}

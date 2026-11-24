use roxide_gc::{FinalizerKind, SexpId, WeakRefEntry};

use crate::api::ValueError;
use crate::object::{Body, RObject, SexpType, WeakRefData};
use crate::session::{RSession, SEXP_NIL};

impl RSession {
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
            return Err(key_obj.type_mismatch("NULL, environment, or externalptr"));
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
            return Err(obj.type_mismatch("weakref"));
        };
        Ok(wr.key)
    }

    /// Read the value of a `WEAKREFSXP` (`R_WeakRefValue`; `SEXP_NIL` once cleared).
    pub fn weakref_value(&self, x: SexpId) -> Result<SexpId, ValueError> {
        let obj = self.heap.get(x)?;
        let Body::WeakRef(wr) = &obj.body else {
            return Err(obj.type_mismatch("weakref"));
        };
        Ok(wr.value)
    }

    /// Clear a `WEAKREFSXP` key/value to `SEXP_NIL` and unregister it from the collector.
    pub fn clear_weakref(&mut self, x: SexpId) -> Result<(), ValueError> {
        let obj = self.heap.get_mut(x)?;
        let mismatch = obj.type_mismatch("weakref");
        let Body::WeakRef(wr) = &mut obj.body else {
            return Err(mismatch);
        };
        wr.key = SEXP_NIL;
        wr.value = SEXP_NIL;
        self.heap.remove_weakref(x);
        Ok(())
    }
}

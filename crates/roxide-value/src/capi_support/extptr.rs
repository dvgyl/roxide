use roxide_gc::SexpId;

use crate::api::ValueError;
use crate::object::{Body, ExtPtrData, RObject};
use crate::session::RSession;

impl RSession {
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
                tag,
                prot,
            }))))?)
    }

    /// Read the raw address of an `EXTPTRSXP` (`R_ExternalPtrAddr`).
    pub fn extptr_addr(&self, x: SexpId) -> Result<usize, ValueError> {
        let obj = self.heap.get(x)?;
        let Body::ExtPtr(ep) = &obj.body else {
            return Err(obj.type_mismatch("externalptr"));
        };
        Ok(ep.addr)
    }

    /// Update the raw address of an `EXTPTRSXP` (`R_SetExternalPtrAddr` / `R_ClearExternalPtr`).
    pub fn set_extptr_addr(&mut self, x: SexpId, addr: usize) -> Result<(), ValueError> {
        let obj = self.heap.get_mut(x)?;
        let mismatch = obj.type_mismatch("externalptr");
        let Body::ExtPtr(ep) = &mut obj.body else {
            return Err(mismatch);
        };
        ep.addr = addr;
        Ok(())
    }

    /// Read the tag `SEXP` of an `EXTPTRSXP` (`R_ExternalPtrTag`).
    pub fn extptr_tag(&self, x: SexpId) -> Result<SexpId, ValueError> {
        let obj = self.heap.get(x)?;
        let Body::ExtPtr(ep) = &obj.body else {
            return Err(obj.type_mismatch("externalptr"));
        };
        Ok(ep.tag)
    }

    /// Update the tag `SEXP` of an `EXTPTRSXP` (`R_SetExternalPtrTag`).
    pub fn set_extptr_tag(&mut self, x: SexpId, tag: SexpId) -> Result<(), ValueError> {
        self.replace_edge(x, tag, "externalptr", |body| match body {
            Body::ExtPtr(ptr) => Some(&mut ptr.tag),
            _ => None,
        })
    }

    /// Read the protected `SEXP` of an `EXTPTRSXP` (`R_ExternalPtrProtected`).
    pub fn extptr_prot(&self, x: SexpId) -> Result<SexpId, ValueError> {
        let obj = self.heap.get(x)?;
        let Body::ExtPtr(ep) = &obj.body else {
            return Err(obj.type_mismatch("externalptr"));
        };
        Ok(ep.prot)
    }

    /// Update the protected `SEXP` of an `EXTPTRSXP` (`R_SetExternalPtrProtected`).
    pub fn set_extptr_prot(&mut self, x: SexpId, prot: SexpId) -> Result<(), ValueError> {
        self.replace_edge(x, prot, "externalptr", |body| match body {
            Body::ExtPtr(ptr) => Some(&mut ptr.prot),
            _ => None,
        })
    }
}

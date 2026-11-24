use roxide_gc::SexpId;

use crate::api::ValueError;
use crate::object::{Attrs, Body, Sym};
use crate::session::{BASE_ENV, BASE_NAMESPACE, RSession};

impl RSession {
    pub(crate) fn replace_edge(
        &mut self,
        owner: SexpId,
        new: SexpId,
        expected: &'static str,
        select: impl FnOnce(&mut Body) -> Option<&mut SexpId>,
    ) -> Result<(), ValueError> {
        let obj = self.heap.get_mut(owner)?;
        let mismatch = obj.type_mismatch(expected);
        let edge = select(&mut obj.body).ok_or(mismatch)?;
        let old = std::mem::replace(edge, new);
        self.heap.inc_ref(new);
        self.heap.dec_ref(old);
        Ok(())
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
    pub fn env_shape_version(&self, env: SexpId) -> Result<u64, ValueError> {
        let target = if env == BASE_NAMESPACE { BASE_ENV } else { env };
        let obj = self.heap.get(target)?;
        match &obj.body {
            Body::Env(e) => Ok(e.shape_version),
            _ => Err(obj.type_mismatch("environment")),
        }
    }

    /// Mutate `CAR` of a cons cell (`SETCAR`).
    pub fn set_car(&mut self, x: SexpId, car: SexpId) -> Result<(), ValueError> {
        self.replace_edge(x, car, "pairlist/language", |body| match body {
            Body::List(cell) | Body::Lang(cell) | Body::Dots(cell) => Some(&mut cell.car),
            _ => None,
        })
    }

    /// Mutate `CDR` of a cons cell (`SETCDR`).
    pub fn set_cdr(&mut self, x: SexpId, cdr: SexpId) -> Result<(), ValueError> {
        self.replace_edge(x, cdr, "pairlist/language", |body| match body {
            Body::List(cell) | Body::Lang(cell) | Body::Dots(cell) => Some(&mut cell.cdr),
            _ => None,
        })
    }

    /// Mutate `TAG` of a cons cell (`SET_TAG`).
    pub fn set_tag(&mut self, x: SexpId, tag: SexpId) -> Result<(), ValueError> {
        self.replace_edge(x, tag, "pairlist/language", |body| match body {
            Body::List(cell) | Body::Lang(cell) | Body::Dots(cell) => Some(&mut cell.tag),
            _ => None,
        })
    }

    /// Mutate a closure's formals (`SET_FORMALS`).
    pub fn set_closure_formals(&mut self, x: SexpId, formals: SexpId) -> Result<(), ValueError> {
        self.replace_edge(x, formals, "closure", |body| match body {
            Body::Closure { formals, .. } => Some(formals),
            _ => None,
        })
    }

    /// Mutate a closure's body (`SET_BODY`).
    pub fn set_closure_body(&mut self, x: SexpId, body: SexpId) -> Result<(), ValueError> {
        self.replace_edge(x, body, "closure", |body| match body {
            Body::Closure { body, .. } => Some(body),
            _ => None,
        })
    }

    /// Mutate a closure's enclosing environment (`SET_CLOENV`).
    pub fn set_closure_env(&mut self, x: SexpId, env: SexpId) -> Result<(), ValueError> {
        self.replace_edge(x, env, "closure", |body| match body {
            Body::Closure { env, .. } => Some(env),
            _ => None,
        })
    }

    /// Validate that `x` is a `SYMSXP` and return it as a typed [`Sym`] without re-interning.
    pub fn sexp_as_sym(&self, x: SexpId) -> Result<Sym, ValueError> {
        let obj = self.heap.get(x)?;
        match obj.body {
            Body::Sym { .. } => Ok(Sym(x)),
            _ => Err(obj.type_mismatch("symbol")),
        }
    }

    /// Reset an environment frame to empty unlocked state with a new parent (`METHODS_NAMESPACE` reload).
    pub fn env_reset(&mut self, env: SexpId, enclos: SexpId) -> Result<(), ValueError> {
        let (old_enclos, old_entries, old_attrs) = {
            let obj = self.heap.get_mut(env)?;
            let mismatch = obj.type_mismatch("environment");
            let old_attrs: Vec<SexpId> = obj.attrs.iter().map(|(_, v)| v).collect();
            let Body::Env(frame) = &mut obj.body else {
                return Err(mismatch);
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

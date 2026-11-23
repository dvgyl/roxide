//! Attribute normalization and environment frame operations.

use roxide_gc::{HeapError, SexpId};

use crate::api::{ValueApi, ValueError};
use crate::na::NA_INTEGER;
use crate::object::{Binding, Body, CharId, EnvData, SexpType, Sym};
use crate::session::RSession;

pub(crate) fn compact_row_count(
    session: &RSession,
    value: SexpId,
) -> Result<Option<u32>, ValueError> {
    if session.type_of(value)? != SexpType::Int {
        return Ok(None);
    }
    let rows = session.int(value)?;
    Ok(
        (rows.len() == 2 && rows.get(0) == NA_INTEGER && rows.get(1) != NA_INTEGER)
            .then(|| rows.get(1).unsigned_abs()),
    )
}

impl RSession {
    pub(crate) fn normalize_names(&mut self, x: SexpId, val: SexpId) -> Result<SexpId, ValueError> {
        let len = self.xlength(x)?;
        let (names, _) = self.coerce(val, SexpType::Str)?;
        let slice = self.str_slice(names)?;
        if slice.len() > len {
            return Err(ValueError::InvalidAttribute(format!(
                "'names' attribute [{}] must be the same length as the vector [{len}]",
                slice.len()
            )));
        }
        if slice.len() == len {
            return Ok(names);
        }
        let mut padded = Vec::new();
        padded
            .try_reserve_exact(len)
            .map_err(|_| HeapError::OutOfMemory {
                requested: len.saturating_mul(size_of::<CharId>()),
            })?;
        padded.extend_from_slice(slice);
        padded.resize(len, CharId::NA);
        self.alloc_str_from(&padded)
    }

    /// Get an attribute. Expand compact row names to the integer sequence starting at 1.
    pub fn get_attrib_expanded(&mut self, x: SexpId, sym: Sym) -> Result<SexpId, ValueError> {
        self.heap.get(x)?;
        let value = self.attr(x, sym);
        if self.sym_name(sym)? == "row.names"
            && let Some(len) = compact_row_count(self, value)?
        {
            return self.alloc_compact_intseq(1, len as usize, 1);
        }
        Ok(value)
    }

    /// Get the row names attribute with compact row names expanded.
    pub fn attr_row_names(&mut self, x: SexpId) -> Result<SexpId, ValueError> {
        let sym = self.intern("row.names")?;
        self.get_attrib_expanded(x, sym)
    }

    /// Remove all attributes and clear the class flag.
    pub fn clear_attrs(&mut self, x: SexpId) -> Result<(), ValueError> {
        let obj = self.heap.get_mut(x)?;
        let old = std::mem::take(&mut obj.attrs);
        obj.flags.object = false;
        for (_, value) in old.iter() {
            self.heap.dec_ref(value);
        }
        self.heap.update_size(x)?;
        Ok(())
    }

    /// Set or clear the S4 flag on an object.
    pub fn set_s4_flag(&mut self, x: SexpId, s4: bool) -> Result<(), ValueError> {
        self.heap.get_mut(x)?.flags.s4 = s4;
        Ok(())
    }

    /// Copy the bindings in an environment frame in insertion order.
    pub fn env_entries(&self, env: SexpId) -> Result<Vec<(Sym, Binding)>, ValueError> {
        let obj = self.heap.get(env)?;
        match &obj.body {
            Body::Env(frame) => Ok(frame.entries.clone()),
            _ => Err(ValueError::TypeMismatch {
                expected: "environment",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    /// Remove a binding from an unlocked frame. Return whether the binding existed.
    pub fn env_remove(&mut self, env: SexpId, sym: Sym) -> Result<bool, ValueError> {
        let name = self.sym_name(sym)?.to_owned();
        let frame = self.env_frame_mut(env)?;
        let Some(&index) = frame.index.get(&sym) else {
            return Ok(false);
        };
        if frame.locked {
            return Err(ValueError::LockedBinding(name));
        }
        let (_, binding) = frame.entries.remove(index);
        frame.index.remove(&sym);
        for (offset, (key, _)) in frame.entries[index..].iter().enumerate() {
            frame.index.insert(*key, index + offset);
        }
        self.heap.dec_ref(binding.value);
        Ok(true)
    }

    /// Set or clear the frame lock. Keep the individual binding locks.
    pub fn env_set_locked(&mut self, env: SexpId, locked: bool) -> Result<(), ValueError> {
        self.env_frame_mut(env)?.locked = locked;
        Ok(())
    }

    /// Set the enclosing environment and update its reference count.
    pub fn env_set_enclos(&mut self, env: SexpId, enclos: SexpId) -> Result<(), ValueError> {
        self.env_enclos(enclos)?;
        let old = std::mem::replace(&mut self.env_frame_mut(env)?.enclos, enclos);
        self.heap.inc_ref(enclos);
        self.heap.dec_ref(old);
        Ok(())
    }

    fn env_frame_mut(&mut self, env: SexpId) -> Result<&mut EnvData, ValueError> {
        let obj = self.heap.get_mut(env)?;
        let actual = obj.sexp_type().type_name(obj.flags.s4);
        match &mut obj.body {
            Body::Env(frame) => Ok(frame),
            _ => Err(ValueError::TypeMismatch {
                expected: "environment",
                actual,
            }),
        }
    }
}

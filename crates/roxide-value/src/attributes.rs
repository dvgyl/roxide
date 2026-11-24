//! Attribute normalization and environment frame operations.

use std::collections::HashMap;

use roxide_gc::{HeapError, SexpId};

use crate::api::{ValueApi, ValueError};
use crate::na::NA_INTEGER;
use crate::object::{Attrs, Binding, Body, CharId, EnvData, SexpType, Sym};
use crate::session::{RSession, SEXP_NIL};

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

    /// Replace all attributes in insertion order and update the class flag and reference counts.
    ///
    /// A NIL value removes its key. A later value for that key starts a new entry.
    pub fn replace_attributes(
        &mut self,
        x: SexpId,
        mut entries: Vec<(Sym, SexpId)>,
    ) -> Result<(), ValueError> {
        self.heap.get(x)?;
        let names = self.intern("names")?;
        let class = self.intern("class")?;
        let mut positions: HashMap<Sym, usize> = HashMap::new();
        for index in 0..entries.len() {
            let (key, value) = entries[index];
            self.sym_name(key)?;
            if value == SEXP_NIL {
                if let Some(old) = positions.remove(&key) {
                    entries[old].1 = SEXP_NIL;
                }
                continue;
            }
            self.heap.get(value)?;
            let value = if key == names {
                self.normalize_names(x, value)?
            } else {
                value
            };
            if let Some(&old) = positions.get(&key) {
                entries[old] = (key, value);
                entries[index].1 = SEXP_NIL;
            } else {
                positions.insert(key, index);
                entries[index].1 = value;
            }
        }
        entries.retain(|&(_, value)| value != SEXP_NIL);
        for &(_, value) in &entries {
            self.heap.inc_ref(value);
        }
        let obj = self.heap.get_mut(x)?;
        obj.flags.object = positions.contains_key(&class);
        let attrs = Attrs((!entries.is_empty()).then(|| entries.into_boxed_slice()));
        let old = std::mem::replace(&mut obj.attrs, attrs);
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
        let target = if env == crate::session::BASE_NAMESPACE {
            crate::session::BASE_ENV
        } else {
            env
        };
        let obj = self.heap.get(target)?;
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
        let target = if env == crate::session::BASE_NAMESPACE {
            crate::session::BASE_ENV
        } else {
            env
        };
        let name = self.sym_name(sym)?.to_owned();
        let frame = self.env_frame_mut(target)?;
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
        frame.shape_version = frame.shape_version.wrapping_add(1);
        self.heap.dec_ref(binding.value);
        Ok(true)
    }

    /// Set or clear the frame lock. Keep the individual binding locks.
    pub fn env_set_locked(&mut self, env: SexpId, locked: bool) -> Result<(), ValueError> {
        let target = if env == crate::session::BASE_NAMESPACE {
            crate::session::BASE_ENV
        } else {
            env
        };
        let frame = self.env_frame_mut(target)?;
        if frame.locked != locked {
            frame.locked = locked;
            frame.shape_version = frame.shape_version.wrapping_add(1);
        }
        Ok(())
    }

    /// Return whether an environment frame is locked.
    pub fn env_is_locked(&self, env: SexpId) -> Result<bool, ValueError> {
        let target = if env == crate::session::BASE_NAMESPACE {
            crate::session::BASE_ENV
        } else {
            env
        };
        let obj = self.heap.get(target)?;
        match &obj.body {
            Body::Env(frame) => Ok(frame.locked),
            _ => Err(ValueError::TypeMismatch {
                expected: "environment",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    /// Lock or unlock a single binding inside an environment frame.
    pub fn env_lock_binding(
        &mut self,
        env: SexpId,
        sym: Sym,
        locked: bool,
    ) -> Result<bool, ValueError> {
        let target = if env == crate::session::BASE_NAMESPACE {
            crate::session::BASE_ENV
        } else {
            env
        };
        let frame = self.env_frame_mut(target)?;
        let Some(&index) = frame.index.get(&sym) else {
            return Ok(false);
        };
        if frame.entries[index].1.locked != locked {
            frame.entries[index].1.locked = locked;
            frame.shape_version = frame.shape_version.wrapping_add(1);
        }
        Ok(true)
    }

    /// Return whether a binding exists and is locked in an environment frame.
    pub fn env_binding_is_locked(&self, env: SexpId, sym: Sym) -> Result<Option<bool>, ValueError> {
        let target = if env == crate::session::BASE_NAMESPACE {
            crate::session::BASE_ENV
        } else {
            env
        };
        let obj = self.heap.get(target)?;
        match &obj.body {
            Body::Env(frame) => Ok(frame
                .index
                .get(&sym)
                .map(|&idx| frame.entries[idx].1.locked)),
            _ => Err(ValueError::TypeMismatch {
                expected: "environment",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    /// Return whether a binding exists and is active in an environment frame.
    pub fn env_binding_is_active(&self, env: SexpId, sym: Sym) -> Result<Option<bool>, ValueError> {
        let target = if env == crate::session::BASE_NAMESPACE {
            crate::session::BASE_ENV
        } else {
            env
        };
        let obj = self.heap.get(target)?;
        match &obj.body {
            Body::Env(frame) => Ok(frame
                .index
                .get(&sym)
                .map(|&idx| frame.entries[idx].1.active)),
            _ => Err(ValueError::TypeMismatch {
                expected: "environment",
                actual: obj.sexp_type().type_name(obj.flags.s4),
            }),
        }
    }

    /// Lock all current bindings in an environment frame.
    pub fn env_lock_all_bindings(&mut self, env: SexpId) -> Result<(), ValueError> {
        let target = if env == crate::session::BASE_NAMESPACE {
            crate::session::BASE_ENV
        } else {
            env
        };
        let frame = self.env_frame_mut(target)?;
        for (_, binding) in &mut frame.entries {
            binding.locked = true;
        }
        frame.shape_version = frame.shape_version.wrapping_add(1);
        Ok(())
    }

    /// Set the enclosing environment and update its reference count.
    pub fn env_set_enclos(&mut self, env: SexpId, enclos: SexpId) -> Result<(), ValueError> {
        self.env_enclos(enclos)?;
        let frame = self.env_frame_mut(env)?;
        let old = std::mem::replace(&mut frame.enclos, enclos);
        frame.shape_version = frame.shape_version.wrapping_add(1);
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

//! Track loaded libraries, registered routines, and finalizers for a session.

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;

use roxide_eval::{Ctx, CtxExt, EvalResult};
use roxide_gc::{FinalizerKind, SexpId, Trace, Tracer};
use roxide_value::SEXP_NIL;

use super::loader::{Image, close, image_owner, image_path};
use super::{Interface, Routine};
use crate::dotcall;

pub(super) struct RegisteredSymbol {
    pub(super) routine: Routine,
    pub(super) with_registration: bool,
    pub(super) pointer: SexpId,
}

pub(crate) struct CallableEntry {
    pub address: usize,
    pub owner: u32,
}

struct ExitFinalizer {
    owner: u32,
    reference: SexpId,
}

pub(crate) struct Dll {
    pub id: u32,
    pub name: String,
    pub dynamic: bool,
    pub force: bool,
    pub routines: Vec<Routine>,
    pub(super) image: Option<Image>,
    pub(super) info: SexpId,
    pub(super) references: [SexpId; 2],
    pub(super) symbols: Vec<RegisteredSymbol>,
    pub(super) unloading: bool,
}

impl Dll {
    pub(crate) fn info(&self) -> SexpId {
        self.info
    }

    pub(crate) fn handle(&self) -> *mut c_void {
        std::ptr::without_provenance_mut(self.id as usize + 1)
    }

    pub(super) fn entry(&self, prefix: &str) -> Option<usize> {
        let path = &self.image.as_ref()?.owner.path;
        let find = |name: &str| {
            let address = self.symbol(&format!("{prefix}_{name}"))?;
            (image_path(address)? == *path).then_some(address)
        };
        find(self.name.as_str()).or_else(|| {
            self.name
                .contains('.')
                .then(|| self.name.replace('.', "_"))
                .and_then(|name| find(&name))
        })
    }

    fn symbol(&self, name: &str) -> Option<usize> {
        if name.as_bytes().contains(&0) {
            return None;
        }
        // SAFETY: The address is only called after the caller selects its C ABI.
        unsafe {
            self.image
                .as_ref()?
                .library
                .get::<*mut c_void>(name.as_bytes())
        }
        .ok()
        .and_then(|symbol| (!symbol.is_null()).then(|| symbol.expose_provenance()))
    }
}

#[derive(Default)]
pub(crate) struct Registry {
    pub dlls: Vec<Dll>,
    pub callables: HashMap<(String, String), CallableEntry>,
    pub(super) active_owner: u32,
    exit_finalizers: Vec<ExitFinalizer>,
    finalizer_owners: HashMap<(SexpId, usize), u32>,
    finalizer_prune_at: usize,
    pub(super) session: Arc<()>,
}

impl Registry {
    pub(crate) fn get(&mut self, id: u32) -> Option<&mut Dll> {
        self.dlls.iter_mut().find(|dll| dll.id == id)
    }

    pub(super) fn by_path(&mut self, path: &Path) -> Option<&mut Dll> {
        self.dlls.iter_mut().find(|dll| {
            dll.image
                .as_ref()
                .is_some_and(|image| image.owner.path == path)
        })
    }

    pub(crate) fn lookup(
        &self,
        name: &str,
        package: Option<&str>,
        kind: Option<Interface>,
        string_call: bool,
    ) -> Option<Routine> {
        let candidates = || {
            self.dlls.iter().rev().filter(|dll| {
                package.is_none_or(|package| package == dll.name) && (!string_call || !dll.force)
            })
        };
        for dll in candidates() {
            if let Some(routine) = dll.routines.iter().find(|routine| {
                let Some((registered, _)) = routine.registration else {
                    return false;
                };
                kind.is_none_or(|kind| kind == registered)
                    && (routine.name == name
                        || (registered == Interface::Fortran
                            && routine.name == name.to_ascii_lowercase()))
            }) {
                return Some(routine.clone());
            }
        }
        for dll in candidates() {
            if !dll.dynamic || dll.force || dll.routines.iter().any(|entry| entry.name == name) {
                continue;
            }
            let addr = match kind {
                Some(Interface::Fortran) => dll.symbol(&format!("{}_", name.to_ascii_lowercase())),
                None => dll
                    .symbol(name)
                    .or_else(|| dll.symbol(&format!("{}_", name.to_ascii_lowercase()))),
                _ => dll.symbol(name),
            };
            if let Some(addr) = addr
                && !dll.routines.iter().any(|entry| entry.addr == addr)
            {
                return Some(Routine {
                    name: name.into(),
                    addr,
                    registration: None,
                    owner: dll.id,
                    code_owner: image_owner(addr).unwrap_or(dll.id),
                });
            }
        }
        None
    }

    pub(crate) fn pointer_routine(&self, object: SexpId) -> Option<Routine> {
        self.dlls
            .iter()
            .flat_map(|dll| &dll.symbols)
            .find(|entry| entry.pointer == object)
            .map(|entry| entry.routine.clone())
    }
}

impl CtxExt for Registry {}
impl Trace for Registry {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for dll in &self.dlls {
            tracer.edge(dll.info);
            tracer.edges(&dll.references);
            for entry in &dll.symbols {
                tracer.edge(entry.pointer);
            }
        }
        for entry in &self.exit_finalizers {
            tracer.edge(entry.reference);
        }
    }
}

pub(crate) fn code_owner(ctx: &mut Ctx, address: usize, owner: u32) -> u32 {
    image_owner(address).unwrap_or_else(|| {
        let active = ctx.ext::<Registry>().active_owner;
        if active == 0 { owner } else { active }
    })
}

pub(crate) fn with_owner<T>(
    ctx: &mut Ctx,
    owner: u32,
    run: impl FnOnce(&mut Ctx) -> EvalResult<T>,
) -> EvalResult<T> {
    if owner == 0 {
        return run(ctx);
    }
    let previous = std::mem::replace(&mut ctx.ext::<Registry>().active_owner, owner);
    let result = run(ctx);
    ctx.ext::<Registry>().active_owner = previous;
    result
}

pub(crate) fn track_exit_finalizer(ctx: &mut Ctx, reference: SexpId, kind: FinalizerKind) {
    if let FinalizerKind::Native { dll, .. } = kind
        && dll != 0
    {
        ctx.ext::<Registry>().exit_finalizers.push(ExitFinalizer {
            owner: dll,
            reference,
        });
    }
}

pub(crate) fn remember_finalizer(
    ctx: &mut Ctx,
    target: SexpId,
    kind: FinalizerKind,
) -> EvalResult<()> {
    let FinalizerKind::Native { fun, dll } = kind else {
        return Ok(());
    };
    if target == SEXP_NIL {
        return Ok(());
    }
    let registry = ctx.ext::<Registry>();
    if registry.finalizer_owners.len() >= registry.finalizer_prune_at {
        let mut owners = std::mem::take(&mut registry.finalizer_owners);
        owners.retain(|(target, _), _| ctx.session().heap.get(*target).is_ok());
        let mut exit = std::mem::take(&mut ctx.ext::<Registry>().exit_finalizers);
        exit.retain(|entry| {
            ctx.session()
                .weakref_key(entry.reference)
                .is_ok_and(|key| key != SEXP_NIL)
        });
        let registry = ctx.ext::<Registry>();
        registry.exit_finalizers = exit;
        registry.finalizer_prune_at = owners.len().saturating_mul(2).max(256);
        registry.finalizer_owners = owners;
    }
    let owners = &mut ctx.ext::<Registry>().finalizer_owners;
    // The evaluator passes only the target and address, even for a batch it has
    // already taken out of the heap. Keep the owner until the DLL is unloaded.
    if owners.get(&(target, fun)).is_some_and(|&old| old != dll) {
        return Err(ctx.stop(
            SEXP_NIL,
            "cannot reuse a native finalizer address for a target from an unloaded library",
        ));
    }
    owners.insert((target, fun), dll);
    Ok(())
}

pub(crate) fn run_finalizer(ctx: &mut Ctx, address: usize, target: SexpId) -> EvalResult<()> {
    let registry = ctx.ext::<Registry>();
    let Some(&owner) = registry.finalizer_owners.get(&(target, address)) else {
        return Ok(());
    };
    if (owner != 0 && !registry.dlls.iter().any(|dll| dll.id == owner))
        || image_path(address).is_none()
    {
        return Ok(());
    }
    ctx.scoped(|ctx| {
        ctx.session_mut()
            .heap
            .try_protect(target)
            .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))?;
        with_owner(ctx, owner, |ctx| {
            dotcall::void_entry(
                ctx,
                address,
                crate::abi::Sexp::from_id(target).0,
                "finalizer",
            )
        })
    })
}

pub(super) fn run_dll_exit_finalizers(ctx: &mut Ctx, owner: u32) {
    let references: Vec<_> = ctx
        .ext::<Registry>()
        .exit_finalizers
        .extract_if(.., |entry| entry.owner == owner)
        .map(|entry| entry.reference)
        .collect();
    let visible = ctx.is_visible();
    let _ = ctx.scoped(|ctx| {
        let mut callbacks = Vec::new();
        for reference in references {
            let Some(entry) = ctx.session_mut().heap.remove_weakref(reference) else {
                continue;
            };
            let _ = ctx.session_mut().clear_weakref(reference);
            if let Some(FinalizerKind::Native { fun, .. }) = entry.finalizer {
                ctx.protect(entry.key);
                callbacks.push((fun, entry.key));
            }
        }
        for (address, target) in callbacks.into_iter().rev() {
            let _ = run_finalizer(ctx, address, target);
        }
        Ok(())
    });
    ctx.set_visible(visible);
}

fn purge_callbacks(ctx: &mut Ctx, id: u32) {
    ctx.session_mut()
        .heap
        .retain_finalizers(|kind| !matches!(kind, FinalizerKind::Native { dll, .. } if dll == id));
    let registry = ctx.ext::<Registry>();
    registry.callables.retain(|_, entry| entry.owner != id);
    registry.finalizer_owners.retain(|_, owner| *owner != id);
    registry.exit_finalizers.retain(|entry| entry.owner != id);
}

pub(super) fn remove(ctx: &mut Ctx, id: u32) -> EvalResult<()> {
    purge_callbacks(ctx, id);
    let registry = ctx.ext::<Registry>();
    let mut pointers = Vec::new();
    for dll in &mut registry.dlls {
        if dll.id == id {
            pointers.extend(dll.references);
        }
        dll.routines.retain(|routine| routine.code_owner != id);
        dll.symbols.retain(|entry| {
            if dll.id == id || entry.routine.code_owner == id {
                pointers.push(entry.pointer);
                false
            } else {
                true
            }
        });
    }
    let mut status = Ok(());
    for pointer in pointers {
        if let Err(error) = ctx.session_mut().set_extptr_addr(pointer, 0) {
            status = Err(error.into());
        }
    }
    let registry = ctx.ext::<Registry>();
    if let Some(index) = registry.dlls.iter().position(|dll| dll.id == id)
        && let Some(image) = registry.dlls.remove(index).image
    {
        let closed = close(ctx, image);
        // A destructor can register callbacks while dlclose is active.
        purge_callbacks(ctx, id);
        closed?;
    }
    status
}

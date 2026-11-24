use std::collections::HashMap;
use std::ffi::c_void;

use roxide_eval::{Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{ForeignAltInfo, SEXP_NIL, SexpType, Sym};

use super::callbacks::Methods;
use crate::dll::{Registry, code_owner};

#[derive(Clone, Copy)]
pub(super) struct Class {
    pub(super) descriptor: SexpId,
    pub(super) ty: SexpType,
    pub(super) methods: Methods,
    pub(super) owner: u32,
}

#[derive(Default)]
pub(super) struct AltrepRegistry {
    classes: Vec<Class>,
    registered: HashMap<(Sym, Sym), Vec<usize>>,
    builtin: HashMap<(Sym, Sym), SexpId>,
}

impl CtxExt for AltrepRegistry {}

impl Trace for AltrepRegistry {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for class in &self.classes {
            tracer.edge(class.descriptor);
        }
        for descriptor in self.builtin.values() {
            tracer.edge(*descriptor);
        }
    }
}

pub(super) fn register(
    ctx: &mut Ctx,
    name: Sym,
    package: Sym,
    ty: SexpType,
    dll: *mut c_void,
) -> EvalResult<SexpId> {
    let owner = if dll.is_null() {
        code_owner(ctx, 0, 0)
    } else {
        let Some(dll) = ctx
            .ext::<Registry>()
            .dlls
            .iter()
            .find(|d| d.handle() == dll)
        else {
            return Err(ctx.stop(SEXP_NIL, "invalid ALTREP DLL handle"));
        };
        dll.id
    };
    let index = ctx.ext::<AltrepRegistry>().classes.len();
    let descriptor = ctx.session_mut().alloc_alt_class(name, package, index)?;
    let registry = ctx.ext::<AltrepRegistry>();
    registry.classes.push(Class {
        descriptor,
        ty,
        methods: Methods::default(),
        owner,
    });
    registry
        .registered
        .entry((name, package))
        .or_default()
        .push(index);
    Ok(descriptor)
}

pub(super) fn active(ctx: &mut Ctx, owner: u32) -> bool {
    owner == 0 || ctx.ext::<Registry>().get(owner).is_some()
}

pub(super) fn class_index(ctx: &mut Ctx, descriptor: SexpId) -> EvalResult<usize> {
    if let Some(index) = ctx.session().alt_class_index(descriptor)?
        && ctx
            .ext::<AltrepRegistry>()
            .classes
            .get(index)
            .is_some_and(|class| class.descriptor == descriptor)
    {
        return Ok(index);
    }
    Err(ctx.stop(SEXP_NIL, "invalid foreign ALTREP class descriptor"))
}

pub(super) fn class(ctx: &mut Ctx, descriptor: SexpId) -> EvalResult<Class> {
    let index = class_index(ctx, descriptor)?;
    let class = ctx.ext::<AltrepRegistry>().classes[index];
    if !active(ctx, class.owner) {
        return Err(ctx.stop(SEXP_NIL, "ALTREP class library was unloaded"));
    }
    Ok(class)
}

pub(super) fn methods_mut(ctx: &mut Ctx, index: usize) -> &mut Methods {
    &mut ctx.ext::<AltrepRegistry>().classes[index].methods
}

pub(super) fn latest(ctx: &mut Ctx, name: Sym, package: Sym) -> Option<Class> {
    let registry = ctx.ext::<AltrepRegistry>();
    let indices = registry.registered.get(&(name, package))?;
    let candidates: Vec<_> = indices
        .iter()
        .rev()
        .map(|&index| registry.classes[index])
        .collect();
    candidates
        .into_iter()
        .find(|class| active(ctx, class.owner))
}

pub(super) fn foreign(ctx: &mut Ctx, x: SexpId) -> EvalResult<ForeignAltInfo> {
    ctx.session()
        .foreign_alt(x)?
        .ok_or_else(|| ctx.stop(SEXP_NIL, "expected a foreign ALTREP vector"))
}

pub(super) fn lazy_methods(ctx: &mut Ctx, x: SexpId) -> EvalResult<Option<Methods>> {
    match ctx.session().foreign_alt(x)? {
        Some(info) if !info.owned => Ok(Some(class(ctx, info.class)?.methods)),
        _ => Ok(None),
    }
}

pub(super) fn descriptor(ctx: &mut Ctx, x: SexpId) -> EvalResult<SexpId> {
    if let Some(info) = ctx.session().foreign_alt(x)? {
        return Ok(info.class);
    }
    let Some((name, package)) = ctx.session().builtin_alt_class(x)? else {
        return Ok(SEXP_NIL);
    };
    let name = ctx.session_mut().intern(name)?;
    let package = ctx.session_mut().intern(package)?;
    if let Some(&descriptor) = ctx.ext::<AltrepRegistry>().builtin.get(&(name, package)) {
        return Ok(descriptor);
    }
    const BUILTIN_CLASS_INDEX: usize = isize::MAX as usize;
    let descriptor = ctx
        .session_mut()
        .alloc_alt_class(name, package, BUILTIN_CLASS_INDEX)?;
    ctx.ext::<AltrepRegistry>()
        .builtin
        .insert((name, package), descriptor);
    Ok(descriptor)
}

pub(super) fn class_error(ctx: &mut Ctx, x: SexpId, message: &str) -> roxide_eval::Unwind {
    let s = ctx.session();
    let names = s
        .alt_class_names(x)
        .ok()
        .flatten()
        .and_then(|(name, package)| Some((s.sym_name(name).ok()?, s.sym_name(package).ok()?)));
    let message = match names {
        Some((name, package)) => format!("{message} [class: {name}, pkg: {package}]"),
        None => message.to_owned(),
    };
    ctx.stop(SEXP_NIL, message)
}

//! Epoch-invalidated S4 class and method caches stored inside [`roxide_eval::Ctx`] (ADR-0012).

use std::collections::HashMap;

use roxide_eval::{Ctx, CtxExt};
use roxide_gc::{SexpId, Trace, Tracer};

/// Cached metadata for one S4 class definition (derived from canonical `.__C__<Class>`).
#[derive(Clone, Debug)]
pub struct ClassInfo {
    /// Class name.
    pub name: String,
    /// Defining package name (e.g. `".GlobalEnv"`, `"methods"`, `"rxs4"`).
    pub package: String,
    /// Declared slot names and required class names in canonical order.
    pub slots: Vec<(String, String)>,
    /// Linearized superclasses `(superclass_name, distance, direct_parent_rank)` in priority order.
    pub superclasses: Vec<(String, usize, usize)>,
    /// Whether this class is virtual (`VIRTUAL` / slotless / class union / oldClass).
    pub is_virtual: bool,
    /// Canonical R `classRepresentation` `SEXP` (`__C__<Class>`).
    pub class_def: SexpId,
}

/// Cached S4 generic metadata (derived from canonical `.__T__<generic>:<pkg>`).
#[derive(Clone, Debug)]
pub struct GenericInfo {
    /// Generic function name.
    pub name: String,
    /// Defining package name.
    pub package: String,
    /// Ordered dispatch signature parameter names.
    pub signature: Vec<String>,
    /// Method table environment (`ENVSXP`).
    pub table: SexpId,
    /// Default fallback function (`SEXP_NIL` if none).
    pub default_fun: SexpId,
}

/// Per-session GC-traced S4 registry and dispatch caches.
#[derive(Default)]
pub struct MethodsExt {
    /// Monotonically incremented whenever classes, methods, or namespaces change.
    pub epoch: u64,
    /// Cached class metadata keyed by `(class_name, package_name)`.
    pub classes: HashMap<(String, String), ClassInfo>,
    /// Cached generic metadata keyed by `(generic_name, package_name)`.
    pub generics: HashMap<(String, String), GenericInfo>,
    /// Selected-method memo keyed by `(generic_name, package_name, target_signature)` -> `(epoch, method_fun)`.
    pub method_cache: HashMap<(String, String, Vec<String>), (u64, SexpId)>,
}

impl MethodsExt {
    /// Increment the invalidation epoch and clear derived dispatch caches.
    pub fn bump_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.method_cache.clear();
    }

    /// Look up cached [`ClassInfo`] by class name and optional package qualifier.
    #[must_use]
    pub fn find_class(&self, name: &str, package: Option<&str>) -> Option<&ClassInfo> {
        if let Some(pkg) = package
            && let Some(info) = self.classes.get(&(name.to_owned(), pkg.to_owned()))
        {
            return Some(info);
        }
        self.classes
            .iter()
            .find(|((cls, _), _)| cls == name)
            .map(|(_, info)| info)
    }

    /// Look up cached [`GenericInfo`] by generic name and optional package qualifier.
    #[must_use]
    pub fn find_generic(&self, name: &str, package: Option<&str>) -> Option<&GenericInfo> {
        if let Some(pkg) = package
            && let Some(info) = self.generics.get(&(name.to_owned(), pkg.to_owned()))
        {
            return Some(info);
        }
        self.generics
            .iter()
            .find(|((gen_name, _), _)| gen_name == name)
            .map(|(_, info)| info)
    }
}

/// Shared contract accessor (Builders A & B): return cached [`ClassInfo`] for `class` (with optional `package`).
pub fn class_info(ctx: &mut Ctx, class: &str, package: Option<&str>) -> Option<ClassInfo> {
    ctx.ext::<MethodsExt>().find_class(class, package).cloned()
}

/// Shared contract accessor (Builders A & B): return linearized superclasses `(name, distance, parent_rank)`
/// for `class` (with optional `package`).
pub fn superclasses(
    ctx: &mut Ctx,
    class: &str,
    package: Option<&str>,
) -> Vec<(String, usize, usize)> {
    ctx.ext::<MethodsExt>()
        .find_class(class, package)
        .map(|info| info.superclasses.clone())
        .unwrap_or_default()
}

impl Trace for MethodsExt {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for info in self.classes.values() {
            tracer.edge(info.class_def);
        }
        for info in self.generics.values() {
            tracer.edge(info.table);
            tracer.edge(info.default_fun);
        }
        for &(_, fun) in self.method_cache.values() {
            tracer.edge(fun);
        }
    }
}

impl CtxExt for MethodsExt {}

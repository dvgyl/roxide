//! Dynamic namespace registry, lexical chain construction, and deferred S3 method tables (ADR-0013).

use std::path::PathBuf;

use roxide_gc::{SexpId, Tracer};
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, CharId, EMPTY_ENV, GLOBAL_ENV, SEXP_NIL, SexpType, TRUE_SCALAR,
    ValueApi,
};

use crate::api::{EvalResult, NamespaceLoader};
use crate::ctx::Ctx;

/// Registered namespace entry in [`Ctx`].
#[derive(Clone, Debug)]
pub struct NsRecord {
    /// Package name (e.g. `"stats"`, `"R6"`).
    pub name: String,
    /// Namespace environment (`namespace:<name>`).
    pub ns: SexpId,
    /// Imports environment (`imports:<name>`, enclosing `BASE_NAMESPACE`).
    pub imports: SexpId,
    /// Attached search-path environment (`package:<name>`), if currently attached.
    pub attached: Option<SexpId>,
    /// Package installation directory on disk, if loaded from disk.
    pub path: Option<PathBuf>,
    /// True for statically linked built-in packages (`stats`, `utils`, `methods`, ...).
    pub builtin: bool,
}

/// Deferred S3 method registration awaiting provider namespace load.
#[derive(Clone, Debug)]
pub(crate) struct DeferredS3 {
    pub(crate) provider: String,
    pub(crate) generic: String,
    pub(crate) class: String,
    pub(crate) fun: SexpId,
}

impl NsRecord {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.edge(self.ns);
        tracer.edge(self.imports);
        if let Some(attached) = self.attached {
            tracer.edge(attached);
        }
    }
}

impl DeferredS3 {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.edge(self.fun);
    }
}

/// Built-in base generics whose cross-package S3 methods are auto-registered during built-in
/// package bootstrap (kept minimal to avoid false registrations like `t.test` or `plot.new`).
const BUILTIN_BASE_GENERICS: &[&str] = &[
    "print",
    "format",
    "summary",
    "diff",
    "as.matrix",
    "as.character",
    "as.data.frame",
    "labels",
    "[",
    "[[",
    "Ops",
    "Math",
    "Summary",
    "Complex",
    "matrixOps",
];

impl Ctx {
    /// Install the dynamic [`NamespaceLoader`] (owned by `roxide-pkg`).
    pub fn set_namespace_loader(&mut self, loader: &'static dyn NamespaceLoader) {
        self.namespace_loader = Some(loader);
    }

    /// Look up a loaded namespace by name (`"base"` resolves to [`BASE_NAMESPACE`]).
    #[must_use]
    pub fn ns_lookup(&self, name: &str) -> Option<SexpId> {
        if name == "base" {
            return Some(BASE_NAMESPACE);
        }
        self.namespaces
            .iter()
            .find(|rec| rec.name == name)
            .map(|rec| rec.ns)
    }

    /// Return the full [`NsRecord`] for a loaded non-base package.
    #[must_use]
    pub fn ns_record(&self, name: &str) -> Option<&NsRecord> {
        self.namespaces.iter().find(|rec| rec.name == name)
    }

    /// Borrow all currently loaded non-base [`NsRecord`] entries.
    #[must_use]
    pub fn ns_records(&self) -> &[NsRecord] {
        &self.namespaces
    }

    /// Return names of all loaded namespaces, including `"base"`.
    #[must_use]
    pub fn ns_loaded_names(&self) -> Vec<String> {
        let mut names = Vec::with_capacity(self.namespaces.len() + 1);
        names.push(String::from("base"));
        for rec in &self.namespaces {
            names.push(rec.name.clone());
        }
        names
    }

    /// Return the namespace name for a namespace environment (including unregistered namespaces
    /// currently being sourced that carry `.__NAMESPACE__.$spec["name"]`).
    #[must_use]
    pub fn namespace_name(&self, env: SexpId) -> Option<String> {
        if env == BASE_NAMESPACE {
            return Some(String::from("base"));
        }
        if let Some(rec) = self.namespaces.iter().find(|rec| rec.ns == env) {
            return Some(rec.name.clone());
        }
        self.unregistered_ns_name(env)
    }

    /// Return `true` iff `env` is a namespace environment (registered or currently loading).
    #[must_use]
    pub fn is_namespace_env(&self, env: SexpId) -> bool {
        if env == BASE_NAMESPACE {
            return true;
        }
        if self.namespaces.iter().any(|rec| rec.ns == env) {
            return true;
        }
        self.unregistered_ns_name(env).is_some()
    }

    fn unregistered_ns_name(&self, env: SexpId) -> Option<String> {
        let entries = self.session.env_entries(env).ok()?;
        let info_env = entries.iter().find_map(|&(sym, binding)| {
            (self.session.sym_name(sym).ok() == Some(".__NAMESPACE__.")).then_some(binding.value)
        })?;
        let info_entries = self.session.env_entries(info_env).ok()?;
        let spec = info_entries.iter().find_map(|&(sym, binding)| {
            (self.session.sym_name(sym).ok() == Some("spec")).then_some(binding.value)
        })?;
        let slice = self.session.str_slice(spec).ok()?;
        let &first = slice.first()?;
        if first == CharId::NA {
            return None;
        }
        let bytes = self.session.char_bytes(first).ok()?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    /// List registered package namespaces and their attached environments.
    pub fn package_environments(
        &self,
    ) -> impl Iterator<Item = (&str, SexpId, Option<SexpId>)> + '_ {
        self.namespaces
            .iter()
            .map(|rec| (rec.name.as_str(), rec.ns, rec.attached))
    }

    /// Look up a loaded namespace or invoke the installed [`NamespaceLoader`] on demand.
    pub fn load_namespace(&mut self, name: &str, call: SexpId) -> EvalResult<SexpId> {
        if let Some(ns) = self.ns_lookup(name) {
            return Ok(ns);
        }
        if let Some(loader) = self.namespace_loader {
            return loader.load(self, name, call);
        }
        Err(self.stop_package_not_found(call, name, SEXP_NIL))
    }

    /// Create a fresh `(ns, imports)` environment pair with canonical `.__NAMESPACE__.`,
    /// `.packageName`, and `.__S3MethodsTable__.` metadata (ADR-0013).
    ///
    /// Lexical chain: `namespace:<name> -> imports:<name> -> BASE_NAMESPACE -> .GlobalEnv -> search`.
    pub fn ns_create(
        &mut self,
        name: &str,
        version: &str,
        path: Option<&str>,
    ) -> EvalResult<(SexpId, SexpId)> {
        self.scoped(|ctx| {
            let imports = ctx.session.alloc_env(BASE_NAMESPACE)?;
            ctx.protect(imports);
            let name_sym = ctx.session.intern("name")?;
            let imports_label = ctx.session.scalar_str(&format!("imports:{name}"))?;
            ctx.protect(imports_label);
            ctx.session.set_attrib(imports, name_sym, imports_label)?;

            let ns = ctx.session.alloc_env(imports)?;
            ctx.protect(ns);

            // Bind `.packageName` in `ns` (matches GNU R `get(".packageName", asNamespace(pkg))`).
            let pkg_name_val = ctx.session.scalar_str(name)?;
            ctx.protect(pkg_name_val);
            let pkg_name_sym = ctx.session.intern(".packageName")?;
            ctx.session.env_define(ns, pkg_name_sym, pkg_name_val)?;

            // Build `.__NAMESPACE__.` environment (parent BASE_ENV, matching GNU R).
            let ns_info = ctx.session.alloc_env(BASE_ENV)?;
            ctx.protect(ns_info);

            let spec = ctx.strings([name, version])?;
            ctx.protect(spec);
            let spec_names = ctx.strings(["name", "version"])?;
            ctx.protect(spec_names);
            let names_sym = ctx.session.intern("names")?;
            ctx.session.set_attrib(spec, names_sym, spec_names)?;

            let exports = ctx.session.alloc_env(BASE_ENV)?;
            ctx.protect(exports);

            let imports_list = ctx.session.alloc_list(1)?;
            ctx.protect(imports_list);
            ctx.session.list_set(imports_list, 0, TRUE_SCALAR)?;
            let base_names = ctx.strings(["base"])?;
            ctx.protect(base_names);
            ctx.session
                .set_attrib(imports_list, names_sym, base_names)?;

            let path_val = ctx.session.scalar_str(path.unwrap_or(""))?;
            ctx.protect(path_val);

            let dynlibs = ctx.session.alloc_str_from(&[])?;
            ctx.protect(dynlibs);

            let dlls = ctx.session.alloc_list(0)?;
            ctx.protect(dlls);

            let lazydata = ctx.session.alloc_env(BASE_ENV)?;
            ctx.protect(lazydata);

            let native_routines = ctx.session.alloc_env(BASE_ENV)?;
            ctx.protect(native_routines);

            // `S3methods` is a 0x4 character matrix in GNU R.
            let s3_matrix = ctx.session.alloc_str_from(&[])?;
            ctx.protect(s3_matrix);
            let s3_dim = ctx.session.alloc_int_from(&[0, 4])?;
            ctx.protect(s3_dim);
            let dim_sym = ctx.session.intern("dim")?;
            ctx.session.set_attrib(s3_matrix, dim_sym, s3_dim)?;

            for (key, val) in [
                ("spec", spec),
                ("exports", exports),
                ("imports", imports_list),
                ("path", path_val),
                ("dynlibs", dynlibs),
                ("DLLs", dlls),
                ("lazydata", lazydata),
                ("nativeRoutines", native_routines),
                ("S3methods", s3_matrix),
            ] {
                let sym = ctx.session.intern(key)?;
                ctx.session.env_define(ns_info, sym, val)?;
            }

            let ns_info_sym = ctx.session.intern(".__NAMESPACE__.")?;
            ctx.session.env_define(ns, ns_info_sym, ns_info)?;

            let s3_table = ctx.session.alloc_env(BASE_ENV)?;
            ctx.protect(s3_table);
            let s3_sym = ctx.session.intern(".__S3MethodsTable__.")?;
            ctx.session.env_define(ns, s3_sym, s3_table)?;

            Ok((ns, imports))
        })
    }

    /// Register a namespace record and flush any deferred S3 methods targeting it.
    ///
    /// Deferred S3 entries are retained in `pending_s3` so that unloading and reloading a
    /// provider namespace re-registers its deferred methods (matching GNU R).
    pub fn ns_register(&mut self, record: NsRecord) -> EvalResult<()> {
        let name = record.name.clone();
        let ns = record.ns;
        if let Some(existing) = self.namespaces.iter_mut().find(|r| r.name == name) {
            *existing = record;
        } else {
            self.namespaces.push(record);
        }
        // Keep `self.pending_s3` intact as GC roots while flushing matching entries!
        let count = self.pending_s3.len();
        for idx in 0..count {
            if self.pending_s3[idx].provider == name {
                let generic = self.pending_s3[idx].generic.clone();
                let class = self.pending_s3[idx].class.clone();
                let fun = self.pending_s3[idx].fun;
                self.install_s3_into_ns(ns, &generic, &class, fun)?;
            }
        }
        Ok(())
    }

    /// Unregister a namespace by name, returning its previous [`NsRecord`] if present.
    ///
    /// **GC safety:** the returned `NsRecord`'s `SexpId` handles are no longer traced by `Ctx`;
    /// callers must protect them immediately before any GC-allocating operation if kept alive.
    pub fn ns_unregister(&mut self, name: &str) -> Option<NsRecord> {
        let pos = self.namespaces.iter().position(|r| r.name == name)?;
        Some(self.namespaces.remove(pos))
    }

    /// Record or clear the attached search-path environment for a loaded namespace.
    pub fn ns_set_attached(&mut self, name: &str, attached: Option<SexpId>) {
        if let Some(rec) = self.namespaces.iter_mut().find(|r| r.name == name) {
            rec.attached = attached;
        }
    }

    /// Return the `exports` environment inside `ns`'s `.__NAMESPACE__.`, if present.
    pub fn ns_exports_env(&mut self, ns: SexpId) -> EvalResult<Option<SexpId>> {
        let info_sym = self.session.intern(".__NAMESPACE__.")?;
        let Some(info_binding) = self.session.env_get(ns, info_sym) else {
            return Ok(None);
        };
        if self.session.type_of(info_binding.value)? != SexpType::Env {
            return Ok(None);
        }
        let exp_sym = self.session.intern("exports")?;
        let Some(exp_binding) = self.session.env_get(info_binding.value, exp_sym) else {
            return Ok(None);
        };
        if self.session.type_of(exp_binding.value)? == SexpType::Env {
            Ok(Some(exp_binding.value))
        } else {
            Ok(None)
        }
    }

    /// Register an S3 method `(generic, class) -> fun` in `provider`'s `.__S3MethodsTable__.`,
    /// recording it in `pending_s3` so reloads also re-apply it.
    pub fn ns_register_s3(
        &mut self,
        provider: &str,
        generic: &str,
        class: &str,
        fun: SexpId,
    ) -> EvalResult<()> {
        if let Some(existing) = self
            .pending_s3
            .iter_mut()
            .find(|e| e.provider == provider && e.generic == generic && e.class == class)
        {
            existing.fun = fun;
        } else {
            self.pending_s3.push(DeferredS3 {
                provider: provider.to_owned(),
                generic: generic.to_owned(),
                class: class.to_owned(),
                fun,
            });
        }
        if let Some(ns) = self.ns_lookup(provider) {
            self.install_s3_into_ns(ns, generic, class, fun)?;
        }
        Ok(())
    }

    fn install_s3_into_ns(
        &mut self,
        ns: SexpId,
        generic: &str,
        class: &str,
        fun: SexpId,
    ) -> EvalResult<()> {
        self.scoped(|ctx| {
            ctx.protect(ns);
            ctx.protect(fun);
            let s3_sym = ctx.session.intern(".__S3MethodsTable__.")?;
            let table = if let Some(binding) = ctx.session.env_get(ns, s3_sym)
                && ctx.session.type_of(binding.value)? == SexpType::Env
            {
                binding.value
            } else {
                let fresh = ctx.session.alloc_env(BASE_ENV)?;
                ctx.protect(fresh);
                ctx.session.env_define(ns, s3_sym, fresh)?;
                fresh
            };
            let method_sym = ctx.session.intern(&format!("{generic}.{class}"))?;
            ctx.session.env_define(table, method_sym, fun)?;
            Ok(())
        })
    }

    /// Populate `.__S3MethodsTable__.` for built-in packages registered via `register_package`.
    pub(crate) fn register_builtin_s3_methods(&mut self, ns: SexpId) -> EvalResult<()> {
        let entries = self.session.env_entries(ns)?;
        for (sym, binding) in entries {
            if !matches!(
                self.session.type_of(binding.value)?,
                SexpType::Clo | SexpType::Builtin | SexpType::Special
            ) {
                continue;
            }
            let full = self.session.sym_name(sym)?.to_owned();
            if full.starts_with('.') || full == "format.pval" || full == "boxplot.stats" {
                continue;
            }
            for &generic in BUILTIN_BASE_GENERICS {
                if let Some(rest) = full.strip_prefix(generic)
                    && let Some(class) = rest.strip_prefix('.')
                    && !class.is_empty()
                {
                    self.install_s3_into_ns(BASE_NAMESPACE, generic, class, binding.value)?;
                    break;
                }
            }
            if let Some((head, class)) = full.split_once('.')
                && !head.is_empty()
                && !class.is_empty()
            {
                let head_sym = self.session.intern(head)?;
                if self.session.env_get(ns, head_sym).is_some() {
                    self.install_s3_into_ns(ns, head, class, binding.value)?;
                }
            }
        }
        Ok(())
    }

    /// Walk the active search path from `.GlobalEnv` down to `package:base`.
    pub fn search_path(&self) -> EvalResult<Vec<SexpId>> {
        let mut envs = Vec::new();
        let mut cur = GLOBAL_ENV;
        let mut guard = 0usize;
        while cur != SEXP_NIL && cur != EMPTY_ENV && guard < 1024 {
            envs.push(cur);
            if cur == BASE_ENV {
                break;
            }
            cur = self.session.env_enclos(cur)?;
            guard += 1;
        }
        Ok(envs)
    }

    /// Splice `env` onto the search path at 1-based position `pos` (`pos >= 2`).
    ///
    /// Rejects attaching an environment already present on the search path to prevent cycles.
    pub fn search_attach(&mut self, env: SexpId, pos: usize, name: &str) -> EvalResult<()> {
        if pos < 2 {
            return Err(self.stop(SEXP_NIL, "'pos' must be at least 2"));
        }
        let path = self.search_path()?;
        if path.contains(&env) {
            return Err(self.stop(SEXP_NIL, "environment is already on the search path"));
        }
        self.scoped(|ctx| {
            ctx.protect(env);
            if !name.is_empty() {
                let name_sym = ctx.session.intern("name")?;
                let label = ctx.session.scalar_str(name)?;
                ctx.protect(label);
                ctx.session.set_attrib(env, name_sym, label)?;
            }
            // Insert before `BASE_ENV` at most.
            let max_insert = path.len().saturating_sub(1).max(1);
            let insert_idx = (pos - 1).min(max_insert);
            let prev = path[insert_idx - 1];
            let next = ctx.session.env_enclos(prev)?;
            ctx.session.env_set_enclos(env, next)?;
            ctx.session.env_set_enclos(prev, env)?;
            Ok(())
        })
    }

    /// Detach and return the environment at 1-based position `pos` on the search path.
    ///
    /// **GC safety:** the returned environment `SexpId` is no longer linked from `GLOBAL_ENV`;
    /// callers must protect it immediately before any GC-allocating operation if kept alive.
    pub fn search_detach(&mut self, pos: usize) -> EvalResult<SexpId> {
        let path = self.search_path()?;
        if pos < 2 || pos > path.len() {
            return Err(self.stop(SEXP_NIL, "invalid 'pos' argument"));
        }
        let target = path[pos - 1];
        if target == BASE_ENV {
            return Err(self.stop(SEXP_NIL, "detaching \"package:base\" is not allowed"));
        }
        let prev = path[pos - 2];
        let next = self.session.env_enclos(target)?;
        self.session.env_set_enclos(prev, next)?;
        for rec in &mut self.namespaces {
            if rec.attached == Some(target) {
                rec.attached = None;
            }
        }
        Ok(target)
    }
}

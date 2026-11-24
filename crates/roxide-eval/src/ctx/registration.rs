use std::collections::{HashMap, HashSet};

use roxide_diag::SourceMap;
use roxide_gc::{GcPolicy, SexpId};
use roxide_host::Host;
use roxide_value::{BASE_ENV, BASE_NAMESPACE, EMPTY_ENV, GLOBAL_ENV, PrimId, RSession, ValueApi};

use crate::api::{ArgEval, InternalError, Module, OptionsCache, Package, PrimKind};
use crate::builtins;
use crate::namespace::NsRecord;

use super::{Ctx, HostStack};

impl Ctx {
    /// Construct a new [`Ctx`] with the given [`Host`] and [`GcPolicy`], registering core M0 primitives.
    pub fn new(host: Box<dyn Host>, policy: GcPolicy) -> Result<Self, InternalError> {
        let mut session = RSession::new(policy)?;
        let autoloads = session.alloc_env(BASE_ENV)?;
        let name_sym = session.intern("name")?;
        let autoloads_label = session.scalar_str("Autoloads")?;
        session.set_attrib(autoloads, name_sym, autoloads_label)?;
        session.env_set_enclos(GLOBAL_ENV, autoloads)?;
        let base_s3 = session.alloc_env(BASE_ENV)?;
        let s3_sym = session.intern(".__S3MethodsTable__.")?;
        session.env_define(BASE_NAMESPACE, s3_sym, base_s3)?;
        let max_expressions = host.limits().max_expressions;
        let jit_level = host
            .env_var("ROXIDE_JIT")
            .or_else(|| host.env_var("R_ENABLE_JIT"))
            .and_then(|v| std::str::from_utf8(&v).ok()?.trim().parse::<u8>().ok())
            .map_or(3, |n| n.min(3));
        let opts = OptionsCache {
            expressions: max_expressions,
            ..OptionsCache::default()
        };
        let mut ctx = Self {
            session,
            box_muller_spare: None,
            host: HostStack::new(host),
            sources: SourceMap::new(),
            opts,
            prims: Vec::new(),
            internals: HashMap::new(),
            registered_prims: HashSet::new(),
            overridable_prims: HashSet::new(),
            overridable_internals: HashSet::new(),
            namespaces: Vec::new(),
            pending_s3: Vec::new(),
            namespace_loader: None,
            autoloads,
            frames: Vec::new(),
            loops: Vec::new(),
            conditions: Default::default(),
            show_error_messages: true,
            options: Vec::new(),
            next_frame_id: 1,
            expr_depth: 0,
            overflow_headroom: 0,
            stack_origin: None,
            stack_budget: 512 * 1024,
            calls: Vec::new(),
            replacement_call: None,
            eval_env: GLOBAL_ENV,
            visible: true,
            pending_warnings: Vec::new(),
            vm_stack: Default::default(),
            extensions: HashMap::new(),
            running_finalizers: false,
            s4_hooks: None,
            native_hooks: None,
            altrep_aware: HashSet::new(),
            jit_level,
            interactive: false,
        };
        let global = ctx.session.intern(".GlobalEnv")?;
        ctx.session.env_define(BASE_ENV, global, GLOBAL_ENV)?;
        ctx.register_module_in(&builtins::CORE_MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::control::MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::language::MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::environment::MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::fallback::MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::conditions::MODULE, BASE_ENV, true)?;
        ctx.register_module_in(&crate::dispatch::MODULE, BASE_ENV, true)?;
        Ok(ctx)
    }

    /// Register a [`Module`]'s primitives and `.Internal` entries in `BASE_ENV`.
    ///
    /// Rejects duplicate non-overridable primitive or `.Internal` registrations (ADR-0011).
    pub fn register_module(&mut self, module: &Module) -> Result<(), InternalError> {
        self.register_module_in(module, BASE_ENV, false)
    }

    /// Register a non-base R [`Package`] (`stats`, `utils`, `tools`, ...) (ADR-0011, ADR-0013).
    ///
    /// Creates `namespace:<name> -> imports:<name> -> BASE_NAMESPACE`, registers all package modules
    /// and preludes into that namespace, populates `.__NAMESPACE__.` and `.__S3MethodsTable__.`,
    /// and when `pkg.attach` is true attaches `package:<name>` at position 2 on `search()`.
    pub fn register_package(&mut self, pkg: &Package) -> Result<(), InternalError> {
        if pkg.name == "base" || self.namespaces.iter().any(|p| p.name == pkg.name) {
            return Err(InternalError {
                message: format!("package '{}' is already registered", pkg.name),
            });
        }
        let (ns, imports) = self
            .ns_create(pkg.name, "4.6.2", None)
            .map_err(|_| InternalError {
                message: format!("failed to create namespace '{}'", pkg.name),
            })?;
        let mark = self.session.heap.protect_mark();
        self.session.heap.protect(ns);
        self.session.heap.protect(imports);
        let result = (|| -> Result<(), InternalError> {
            for module in pkg.modules {
                self.register_module_in(module, ns, false)?;
            }
            let exports_env = self
                .ns_exports_env(ns)
                .map_err(|_| InternalError {
                    message: format!("missing exports env for '{}'", pkg.name),
                })?
                .ok_or_else(|| InternalError {
                    message: format!("missing exports env for '{}'", pkg.name),
                })?;
            self.session.heap.protect(exports_env);

            let mut exported_bindings = Vec::new();
            if let Some(explicit) = pkg.exports {
                for &name in explicit {
                    let sym = self.session.intern(name)?;
                    let val = self.session.scalar_str(name)?;
                    self.session.env_define(exports_env, sym, val)?;
                    if let Some(binding) = self.session.env_get(ns, sym) {
                        exported_bindings.push((sym, binding));
                    }
                }
            } else {
                for (sym, binding) in self.session.env_entries(ns)? {
                    let sym_name = self.session.sym_name(sym)?.to_owned();
                    if !sym_name.starts_with('.') {
                        let val = self.session.scalar_str(&sym_name)?;
                        self.session.env_define(exports_env, sym, val)?;
                        exported_bindings.push((sym, binding));
                    }
                }
            }

            self.register_builtin_s3_methods(ns)
                .map_err(|_| InternalError {
                    message: format!("failed to register S3 methods for '{}'", pkg.name),
                })?;

            let attached = if pkg.attach {
                let pkg_env = self.session.alloc_env(EMPTY_ENV)?;
                self.session.heap.protect(pkg_env);
                for (sym, binding) in exported_bindings {
                    self.session.env_define_binding(pkg_env, sym, binding)?;
                }
                self.search_attach(pkg_env, 2, &format!("package:{}", pkg.name))
                    .map_err(|_| InternalError {
                        message: format!("failed to attach 'package:{}'", pkg.name),
                    })?;
                Some(pkg_env)
            } else {
                None
            };

            self.ns_register(NsRecord {
                name: pkg.name.to_owned(),
                ns,
                imports,
                attached,
                path: None,
                builtin: true,
            })
            .map_err(|_| InternalError {
                message: format!("failed to register namespace '{}'", pkg.name),
            })?;
            Ok(())
        })();
        self.session.heap.unprotect_to(mark);
        result
    }

    fn register_module_in(
        &mut self,
        module: &Module,
        target_env: SexpId,
        overridable: bool,
    ) -> Result<(), InternalError> {
        for prim in module.prims {
            let id = PrimId(u16::try_from(self.prims.len()).map_err(|_| InternalError {
                message: String::from("primitive table is full"),
            })?);
            match prim.kind {
                PrimKind::Primitive => {
                    let key = (target_env, prim.name);
                    if self.registered_prims.contains(&key) && !self.overridable_prims.remove(&key)
                    {
                        return Err(InternalError {
                            message: format!("duplicate primitive registration '{}'", prim.name),
                        });
                    }
                    self.prims.push(prim);
                    self.registered_prims.insert(key);
                    if overridable {
                        self.overridable_prims.insert(key);
                    }
                    let sym = self.session.intern(prim.name)?;
                    let special = matches!(prim.args, ArgEval::Unevaluated | ArgEval::EvalDollar);
                    let pval = self.session.alloc_prim(id, special)?;
                    self.session.env_define(target_env, sym, pval)?;
                }
                PrimKind::Internal => {
                    if self.internals.contains_key(prim.name)
                        && !self.overridable_internals.remove(prim.name)
                    {
                        return Err(InternalError {
                            message: format!("duplicate .Internal registration '{}'", prim.name),
                        });
                    }
                    self.prims.push(prim);
                    if overridable {
                        self.overridable_internals.insert(prim.name);
                    }
                    self.internals.insert(prim.name, id);
                }
            }
        }
        if !module.prelude.is_empty() {
            self.run_script("<prelude>", module.prelude, target_env, false)
                .map_err(|_| InternalError {
                    message: String::from("error evaluating module prelude"),
                })?;
        }
        Ok(())
    }

    /// Return the registered name of a primitive, if the value is a primitive.
    #[must_use]
    pub fn primitive_name(&self, value: SexpId) -> Option<&'static str> {
        let (id, _) = self.session.prim_parts(value).ok()?;
        self.prims.get(usize::from(id.0)).map(|def| def.name)
    }
}

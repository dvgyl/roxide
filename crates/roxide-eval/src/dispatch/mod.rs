mod primitive;
mod s3;
mod s4;

use roxide_gc::{SexpId, Tracer};
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, ValueApi,
};

use crate::eval::ArgInput;
use crate::{Args, Arity, Call, Ctx, EvalResult, GroupGeneric, Module};

/// Primitives for S3 and S4 method dispatch.
pub(crate) const MODULE: Module = Module {
    prims: &[
        primitive!(
            "UseMethod",
            s3::use_method,
            Unevaluated,
            Arity::Range(1, 2),
            SetByFn
        ),
        primitive!(
            "NextMethod",
            s3::next_method,
            Unevaluated,
            Arity::Any,
            SetByFn
        ),
        primitive!(
            "chooseOpsMethod",
            primitive::choose_ops_method,
            Eval,
            Arity::Exact(6),
            On
        ),
        primitive!("@", s4::slot_get, EvalDollar, Arity::Exact(2), On),
        primitive!("@<-", s4::slot_set, EvalDollar, Arity::Exact(3), On),
        primitive!(
            "standardGeneric",
            s4::standard_generic,
            Eval,
            Arity::Range(1, 2),
            SetByFn
        ),
        primitive!("isS4", s4::is_s4_prim, Eval, Arity::Exact(1), On),
        primitive!("asS4", s4::as_s4_prim, Eval, Arity::Range(1, 3), On),
    ],
    prelude: "",
};

/// Method selection state for an active call.
#[derive(Clone)]
pub(crate) struct MethodFrame {
    generic: String,
    group: Option<GroupGeneric>,
    group_method: bool,
    names: Vec<String>,
    classes: Vec<String>,
    class: SexpId,
    offset: usize,
    side: usize,
    caller: SexpId,
    definition: SexpId,
    primitive: Option<Call>,
    is_default: bool,
}

impl MethodFrame {
    fn handles(&self) -> impl Iterator<Item = SexpId> {
        [self.class, self.caller, self.definition]
            .into_iter()
            .chain(
                self.primitive
                    .into_iter()
                    .flat_map(|call| [call.expr, call.env, call.user_call]),
            )
    }

    /// Trace the R values referenced by this frame.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for root in self.handles() {
            tracer.edge(root);
        }
    }

    /// Protect the R values referenced by this frame.
    pub(crate) fn protect(&self, ctx: &mut Ctx) {
        for root in self.handles() {
            ctx.protect(root);
        }
    }

    /// Bind the dispatch variables in the method environment.
    pub(crate) fn bind(&self, ctx: &mut Ctx, env: SexpId) -> EvalResult<()> {
        let generic = ctx.session.scalar_str(&self.generic)?;
        let method = ctx.strings(self.names.iter().map(String::as_str))?;
        for (name, value) in [
            (".Generic", generic),
            (".Method", method),
            (".Class", self.class),
            (".GenericCallEnv", self.caller),
            (".GenericDefEnv", self.definition),
        ] {
            let tag = ctx.session.intern(name)?;
            ctx.assign(tag, value, env)?;
        }
        if self.primitive.is_some() {
            let group = if self.group_method {
                self.group.map_or("", group_name)
            } else {
                ""
            };
            let group = ctx.session.scalar_str(group)?;
            let tag = ctx.session.intern(".Group")?;
            ctx.assign(tag, group, env)?;
        }
        Ok(())
    }

    fn select(&mut self, ctx: &mut Ctx, candidate: &Candidate, previous: SexpId) -> EvalResult<()> {
        self.offset = candidate.index;
        self.is_default = candidate.default;
        self.group_method = candidate.group_method;
        self.names[self.side] = candidate.name.clone();
        self.class = if candidate.default && previous == SEXP_NIL {
            SEXP_NIL
        } else {
            let class = ctx.strings(self.classes[self.offset..].iter().map(String::as_str))?;
            let previous = if previous == SEXP_NIL && self.offset > 0 {
                ctx.strings(self.classes.iter().map(String::as_str))?
            } else {
                previous
            };
            if previous != SEXP_NIL {
                let tag = ctx.session.intern("previous")?;
                ctx.session.set_attrib(class, tag, previous)?;
            }
            class
        };
        Ok(())
    }
}

struct Candidate {
    fun: SexpId,
    name: String,
    index: usize,
    default: bool,
    group_method: bool,
}

fn group_name(group: GroupGeneric) -> &'static str {
    match group {
        GroupGeneric::Ops => "Ops",
        GroupGeneric::Math => "Math",
        GroupGeneric::Summary => "Summary",
        GroupGeneric::Complex => "Complex",
        GroupGeneric::MatrixOps => "matrixOps",
    }
}

/// Arguments for dispatch on a primitive call.
pub(crate) enum DispatchInput<'a> {
    Values {
        args: &'a Args,
        source: Option<&'a Args>,
    },
    Subscript {
        receiver: SexpId,
        actuals: &'a Args,
    },
}

impl Ctx {
    /// Find an explicit S3 method for an object without invoking its default method.
    pub fn explicit_s3_method(
        &mut self,
        generic: &str,
        x: SexpId,
        caller: SexpId,
        definition: SexpId,
    ) -> EvalResult<Option<SexpId>> {
        for class in self.classes(x, false)? {
            if let Some(fun) = self.lookup_method(
                generic,
                &format!("{generic}.{class}"),
                caller,
                definition,
                false,
            )? {
                return Ok(Some(fun));
            }
        }
        Ok(None)
    }

    /// Return the class names used for method lookup.
    pub(crate) fn classes(&mut self, value: SexpId, implicit: bool) -> EvalResult<Vec<String>> {
        if self.session.is_s4(value)?
            && let Some(chain) = self.s4_class_chain(value)?
            && let Ok(slice) = self.session.str_slice(chain)
            && !slice.is_empty()
        {
            return slice
                .iter()
                .map(|&id| Ok(String::from_utf8_lossy(self.session.char_bytes(id)?).into_owned()))
                .collect();
        }
        let class_tag = self.session.intern("class")?;
        let explicit = self.session.attr(value, class_tag);
        if explicit != SEXP_NIL {
            return self
                .session
                .str_slice(explicit)?
                .iter()
                .map(|&id| Ok(String::from_utf8_lossy(self.session.char_bytes(id)?).into_owned()))
                .collect();
        }
        if !implicit {
            return Ok(Vec::new());
        }
        let mut classes = Vec::new();
        let dim_tag = self.session.intern("dim")?;
        let dim = self.session.attr(value, dim_tag);
        if dim != SEXP_NIL {
            if self.session.xlength(dim)? == 2 {
                classes.push("matrix");
            }
            classes.push("array");
        }
        match self.session.type_of(value)? {
            SexpType::Real => classes.extend(["double", "numeric"]),
            SexpType::Int => classes.extend(["integer", "numeric"]),
            SexpType::Builtin | SexpType::Special | SexpType::Clo => classes.push("function"),
            SexpType::Sym => classes.push("name"),
            SexpType::Lang => classes.push(self.session.call_class(value)?),
            ty => classes.push(ty.type_name(false)),
        }
        Ok(classes.into_iter().map(str::to_owned).collect())
    }

    fn optional_fun(
        &mut self,
        name: &str,
        mut env: SexpId,
        end: SexpId,
    ) -> EvalResult<Option<SexpId>> {
        let sym = self.session.intern(name)?;
        while env != SEXP_NIL && env != end {
            if !self.namespaces.iter().any(|pkg| pkg.attached == Some(env))
                && let Some(binding) = self.session.env_get(env, sym)
                && binding.value != SEXP_MISSING
                && binding.value != SEXP_UNBOUND
            {
                let value = self.read_binding(binding, env)?;
                if matches!(
                    self.session.type_of(value)?,
                    SexpType::Clo | SexpType::Builtin | SexpType::Special
                ) {
                    return Ok(Some(value));
                }
            }
            env = self.session.env_enclos(env)?;
        }
        Ok(None)
    }

    fn lookup_method(
        &mut self,
        _generic: &str,
        name: &str,
        caller: SexpId,
        definition: SexpId,
        _default: bool,
    ) -> EvalResult<Option<SexpId>> {
        if let Some(fun) = self.optional_fun(name, caller, BASE_ENV)? {
            return Ok(Some(fun));
        }
        let s3 = self.session.intern(".__S3MethodsTable__.")?;
        if let Some(table) = self.session.env_get(definition, s3) {
            let tag = self.session.intern(name)?;
            if let Some(fun) = self.session.env_get(table.value, tag) {
                return Ok(Some(fun.value));
            }
        }
        if definition != BASE_NAMESPACE && definition != BASE_ENV {
            return self.optional_fun(name, definition, self.session.env_enclos(definition)?);
        }
        if let Some(table) = self.session.env_get(BASE_NAMESPACE, s3) {
            let tag = self.session.intern(name)?;
            if let Some(fun) = self.session.env_get(table.value, tag) {
                return Ok(Some(fun.value));
            }
        }
        self.optional_fun(name, BASE_ENV, SEXP_NIL)
    }

    fn find_method(
        &mut self,
        method: &MethodFrame,
        start: usize,
        default: bool,
    ) -> EvalResult<Option<Candidate>> {
        for (index, class) in method.classes.iter().enumerate().skip(start) {
            let mut names = vec![format!("{}.{class}", method.generic)];
            if let Some(group) = method.group {
                names.push(format!("{}.{class}", group_name(group)));
            }
            for (index_in_class, name) in names.into_iter().enumerate() {
                if let Some(fun) = self.lookup_method(
                    &method.generic,
                    &name,
                    method.caller,
                    method.definition,
                    false,
                )? {
                    return Ok(Some(Candidate {
                        fun,
                        name,
                        index,
                        default: false,
                        group_method: index_in_class != 0,
                    }));
                }
            }
        }
        if default {
            let name = format!("{}.default", method.generic);
            if let Some(fun) = self.lookup_method(
                &method.generic,
                &name,
                method.caller,
                method.definition,
                true,
            )? {
                return Ok(Some(Candidate {
                    fun,
                    name,
                    index: method.classes.len(),
                    default: true,
                    group_method: false,
                }));
            }
        }
        Ok(None)
    }

    fn invoke_method(
        &mut self,
        fun: SexpId,
        name: &str,
        args: Args,
        caller: SexpId,
        method: MethodFrame,
    ) -> EvalResult {
        let head = self.session.intern(name)?.as_sexp();
        let raw = args
            .iter()
            .map(|(tag, value)| Ok((tag, self.promise_expr(value)?)))
            .collect::<EvalResult<Vec<_>>>()?;
        let call = self.make_call(head, &Args::from_pairs(raw))?;
        if self.session.type_of(fun)? == SexpType::Clo {
            self.invoke_closure(fun, args, call, caller, Some(method))
        } else {
            let (prim, _) = self.session.prim_parts(fun)?;
            self.run_primitive(
                Call {
                    expr: call,
                    user_call: call,
                    env: caller,
                    prim,
                },
                args,
                ArgInput::Expressions,
                false,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use roxide_gc::GcPolicy;
    use roxide_host::PureHost;
    use roxide_value::{GLOBAL_ENV, SEXP_NIL, ValueApi};

    use super::Ctx;

    #[test]
    fn as_character_dispatches_a_namespace_method_for_an_external_pointer() {
        let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).unwrap();
        let (ns, _) = ctx.ns_create("pointerpkg", "1.0", None).unwrap();
        ctx.protect(ns);
        ctx.run_script(
            "<S3 method>",
            "as.character.rx_pointer <- function(x, ...) 'method:externalptr'",
            ns,
            false,
        )
        .unwrap();
        let method = ctx.session.intern("as.character.rx_pointer").unwrap();
        let method = ctx.get_local(method, ns).unwrap().unwrap();
        ctx.ns_register_s3("base", "as.character", "rx_pointer", method)
            .unwrap();

        let pointer = ctx.session.alloc_extptr(0, SEXP_NIL, SEXP_NIL).unwrap();
        let class = ctx.session.scalar_str("rx_pointer").unwrap();
        let tag = ctx.session.intern("class").unwrap();
        ctx.session.set_attrib(pointer, tag, class).unwrap();
        let name = ctx.session.intern("pointer").unwrap();
        ctx.assign(name, pointer, GLOBAL_ENV).unwrap();

        ctx.run_global("<dispatch>", "result <- as.character(pointer)")
            .unwrap();
        let name = ctx.session.intern("result").unwrap();
        let result = ctx.get_var(name, GLOBAL_ENV).unwrap().unwrap();
        assert_eq!(ctx.session.str_slice(result).unwrap().len(), 1);
        let text = ctx.session.str_slice(result).unwrap()[0];
        assert_eq!(ctx.session.char_bytes(text).unwrap(), b"method:externalptr");
    }
}
